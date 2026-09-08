pub struct Availability {
    pub label: &'static str,
    pub detail: &'static str,
}

#[cfg(target_os = "macos")]
pub fn availability() -> Availability {
    if unsafe { hellas_apple_supported() } {
        Availability {
            label: "available",
            detail: "App Attest is available; enrollment has not been created",
        }
    } else {
        Availability {
            label: "unavailable",
            detail: "App Attest is unavailable to this build or machine",
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub const fn availability() -> Availability {
    Availability {
        label: "unavailable",
        detail: "App Attest is available only in a provisioned macOS Gate build",
    }
}

#[cfg(target_os = "macos")]
mod native {
    use std::ffi::{CStr, CString, c_char};

    use hellas_attestation::{
        AppleCredential, AttestationError, Attester, Binding, RootProver, apple_client_data_hash,
    };
    use hellas_rpc::pb::execute::AssuranceEvidence;
    use hellas_rpc::{APPLE_APP_ATTEST, ContentId, Digest, RootProof};

    #[derive(Clone)]
    pub struct AppleAppAttestRoot {
        key: String,
        credential: ContentId,
    }

    impl AppleAppAttestRoot {
        pub fn create(
            enrollment_statement: &[u8],
        ) -> Result<(Self, AppleCredential), AttestationError> {
            if !unsafe { super::hellas_apple_supported() } {
                return Err(AttestationError::Platform("unavailable to this app".into()));
            }
            let key = String::from_utf8(call(unsafe { hellas_apple_generate_key() })?)
                .map_err(|_| AttestationError::Platform("invalid key identifier".into()))?;
            let hash = apple_client_data_hash(enrollment_statement);
            let attestation =
                call(unsafe { hellas_apple_attest(cstring(&key)?.as_ptr(), hash.as_ptr()) })?;
            let credential = AppleCredential {
                attestation,
                client_data_hash: hash,
            };
            Ok((
                Self {
                    key,
                    credential: credential.content_id(),
                },
                credential,
            ))
        }

        pub fn load(key: String, credential: ContentId) -> Self {
            Self { key, credential }
        }

        pub fn key(&self) -> &str {
            &self.key
        }

        fn assertion(&self, hash: [u8; 32]) -> Result<Vec<u8>, AttestationError> {
            call(unsafe { hellas_apple_assert(cstring(&self.key)?.as_ptr(), hash.as_ptr()) })
        }

        async fn assertion_async(&self, hash: [u8; 32]) -> Result<Vec<u8>, AttestationError> {
            let root = self.clone();
            tokio::task::spawn_blocking(move || root.assertion(hash))
                .await
                .map_err(|error| AttestationError::Platform(error.to_string()))?
        }
    }

    impl Attester for AppleAppAttestRoot {
        async fn attest(&self, binding: Binding) -> Result<AssuranceEvidence, AttestationError> {
            Ok(AssuranceEvidence {
                codec: APPLE_APP_ATTEST.into(),
                credential: self.credential.as_bytes().to_vec(),
                proof: self.assertion_async(*binding.as_bytes()).await?,
            })
        }
    }

    impl RootProver for AppleAppAttestRoot {
        async fn prove_statement(&self, statement: &[u8]) -> Result<RootProof, AttestationError> {
            Ok(RootProof::AppleAppAttest(
                self.assertion_async(apple_client_data_hash(statement))
                    .await?,
            ))
        }

        async fn prove_open_binding(&self, binding: Digest) -> Result<RootProof, AttestationError> {
            Ok(RootProof::AppleAppAttest(
                self.assertion_async(*binding.as_bytes()).await?,
            ))
        }
    }

    #[repr(C)]
    struct NativeResult {
        bytes: *mut u8,
        len: usize,
        error: *mut c_char,
    }

    fn cstring(value: &str) -> Result<CString, AttestationError> {
        CString::new(value).map_err(|_| AttestationError::Platform("invalid key identifier".into()))
    }

    fn call(value: NativeResult) -> Result<Vec<u8>, AttestationError> {
        let result = if value.error.is_null() {
            if value.len == 0 {
                Ok(Vec::new())
            } else if value.bytes.is_null() {
                Err(AttestationError::Platform(
                    "DeviceCheck returned an invalid empty result".into(),
                ))
            } else {
                Ok(unsafe { std::slice::from_raw_parts(value.bytes, value.len) }.to_vec())
            }
        } else {
            Err(AttestationError::Platform(
                unsafe { CStr::from_ptr(value.error) }
                    .to_string_lossy()
                    .into_owned(),
            ))
        };
        unsafe { hellas_apple_result_free(value) };
        result
    }

    unsafe extern "C" {
        fn hellas_apple_generate_key() -> NativeResult;
        fn hellas_apple_attest(key: *const c_char, hash: *const u8) -> NativeResult;
        fn hellas_apple_assert(key: *const c_char, hash: *const u8) -> NativeResult;
        fn hellas_apple_result_free(value: NativeResult);
    }
}

#[cfg(target_os = "macos")]
pub use native::AppleAppAttestRoot;

#[cfg(not(target_os = "macos"))]
#[derive(Clone)]
pub struct AppleAppAttestRoot;

#[cfg(not(target_os = "macos"))]
impl AppleAppAttestRoot {
    pub fn create(
        _enrollment_statement: &[u8],
    ) -> Result<(Self, hellas_attestation::AppleCredential), hellas_attestation::AttestationError>
    {
        Err(hellas_attestation::AttestationError::Platform(
            "unavailable outside a provisioned macOS Gate build".into(),
        ))
    }

    pub fn load(_key: String, _credential: hellas_rpc::ContentId) -> Self {
        Self
    }

    pub fn key(&self) -> &str {
        ""
    }
}

#[cfg(not(target_os = "macos"))]
impl hellas_attestation::RootProver for AppleAppAttestRoot {
    async fn prove_statement(
        &self,
        _statement: &[u8],
    ) -> Result<hellas_rpc::RootProof, hellas_attestation::AttestationError> {
        Err(hellas_attestation::AttestationError::Platform(
            "Apple App Attest is unavailable".into(),
        ))
    }

    async fn prove_open_binding(
        &self,
        _binding: hellas_rpc::Digest,
    ) -> Result<hellas_rpc::RootProof, hellas_attestation::AttestationError> {
        Err(hellas_attestation::AttestationError::Platform(
            "Apple App Attest is unavailable".into(),
        ))
    }
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn hellas_apple_supported() -> bool;
}
