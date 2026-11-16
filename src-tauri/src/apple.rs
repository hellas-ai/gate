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
    use hellas_auth::Digest;
    use hellas_rpc::pb::execute::AssuranceEvidence;
    use hellas_rpc::{APPLE_APP_ATTEST, ContentId, RootProof};

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
        fn prove_statement<'a>(
            &'a self,
            statement: &'a [u8],
        ) -> hellas_attestation::RootProofFuture<'a> {
            Box::pin(async move {
                Ok(RootProof::AppleAppAttest(
                    self.assertion_async(apple_client_data_hash(statement))
                        .await?,
                ))
            })
        }

        fn prove_open_binding<'a>(
            &'a self,
            binding: Digest,
        ) -> hellas_attestation::RootProofFuture<'a> {
            Box::pin(async move {
                Ok(RootProof::AppleAppAttest(
                    self.assertion_async(*binding.as_bytes()).await?,
                ))
            })
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
    fn prove_statement<'a>(
        &'a self,
        _statement: &'a [u8],
    ) -> hellas_attestation::RootProofFuture<'a> {
        Box::pin(async move {
            Err(hellas_attestation::AttestationError::Platform(
                "Apple App Attest is unavailable".into(),
            ))
        })
    }

    fn prove_open_binding<'a>(
        &'a self,
        _binding: hellas_auth::Digest,
    ) -> hellas_attestation::RootProofFuture<'a> {
        Box::pin(async move {
            Err(hellas_attestation::AttestationError::Platform(
                "Apple App Attest is unavailable".into(),
            ))
        })
    }
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn hellas_apple_supported() -> bool;
}

#[cfg(all(test, target_os = "macos"))]
mod hardware_tests {
    use super::AppleAppAttestRoot;
    use hellas_attestation::{
        AnchorTime, ApplePolicy, RootProver, apple_app_attest_root_ca, apple_app_id_hash,
        apple_client_data_hash, register_apple, verify_apple_assertion,
    };
    use std::io::Read as _;

    #[tokio::test]
    #[ignore = "requires a provisioned macOS 27 Full Security bundle and live Apple hardware"]
    async fn provisioned_apple_root_verifies_live_assertions_after_reload() {
        let app_id = std::env::var("HELLAS_GATE_TEST_APP_ID").expect("set the exact signed App ID");
        let cd_hash: [u8; 32] = hex::decode(
            std::env::var("HELLAS_GATE_TEST_CDHASH").expect("set CandidateCDHashFull sha256"),
        )
        .unwrap()
        .try_into()
        .expect("full SHA-256 CDHash must be 32 bytes");
        let mut nonce = [0; 32];
        std::fs::File::open("/dev/urandom")
            .unwrap()
            .read_exact(&mut nonce)
            .unwrap();
        let (root, credential) = AppleAppAttestRoot::create(&nonce).unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let registered = register_apple(
            &credential,
            apple_app_id_hash(&app_id),
            apple_app_attest_root_ca(),
            AnchorTime(now),
        )
        .unwrap();
        let policy = ApplePolicy {
            expected_rp_id_hash: apple_app_id_hash(&app_id),
            allowed_cd_hashes: vec![cd_hash],
        };
        let hellas_rpc::RootProof::AppleAppAttest(first) =
            root.prove_statement(&nonce).await.unwrap()
        else {
            panic!("hardware root returned a software proof");
        };
        let first = verify_apple_assertion(
            &first,
            &apple_client_data_hash(&nonce),
            &registered,
            &policy,
        )
        .unwrap();
        // Reconstruct using the same hardware key identifier, as persisted
        // provider identity loading does; no private key leaves DeviceCheck.
        let reloaded = AppleAppAttestRoot::load(root.key().to_owned(), credential.content_id());
        std::fs::File::open("/dev/urandom")
            .unwrap()
            .read_exact(&mut nonce)
            .unwrap();
        let binding = hellas_auth::Digest::from_bytes(nonce);
        let hellas_rpc::RootProof::AppleAppAttest(assertion) =
            reloaded.prove_open_binding(binding).await.unwrap()
        else {
            panic!("reloaded hardware root returned a software proof");
        };
        let second =
            verify_apple_assertion(&assertion, binding.as_bytes(), &registered, &policy).unwrap();
        assert!(second.counter > first.counter);
        nonce[0] ^= 1;
        assert!(verify_apple_assertion(&assertion, &nonce, &registered, &policy).is_err());
        println!(
            "Apple root chain OK; App ID={app_id}; CDHash={}; live challenge verified; counter {} -> {}",
            hex::encode(cd_hash),
            first.counter,
            second.counter
        );
    }
}
