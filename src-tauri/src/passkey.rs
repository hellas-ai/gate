//! System passkeys retain only a credential identifier and public key in Gate.
use anyhow::{Context, ensure};
use base64::{
    Engine,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use hellas_auth::{
    Digest,
    authority::{AuthorityError, PasskeyAssertion},
};
use hellas_authority::authenticator::{Authenticator, PublicKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

const RP: &str = "hellas.ai";
const ORIGIN: &str = "https://hellas.ai";

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativePasskey {
    credential_id: Vec<u8>,
    public_key: Vec<u8>,
}
#[derive(Deserialize)]
struct Reply {
    credential_id: String,
    client_data_json: String,
    #[serde(default)]
    attestation_object: String,
    #[serde(default)]
    authenticator_data: String,
    #[serde(default)]
    signature: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClientData {
    #[serde(rename = "type")]
    kind: String,
    challenge: String,
    origin: String,
    #[serde(default)]
    cross_origin: bool,
}
fn decode(value: &str, limit: usize) -> anyhow::Result<Vec<u8>> {
    ensure!(
        value.len() <= limit.div_ceil(3) * 4,
        "passkey response exceeds limit"
    );
    let bytes = STANDARD.decode(value)?;
    ensure!(bytes.len() <= limit, "passkey response exceeds limit");
    Ok(bytes)
}
fn client_data(value: &str, challenge: Digest, kind: &str) -> anyhow::Result<Vec<u8>> {
    let bytes = decode(value, hellas_webauthn::MAX_CLIENT_DATA_JSON_LEN)?;
    let data: ClientData = serde_json::from_slice(&bytes)?;
    ensure!(
        data.kind == kind && data.challenge == URL_SAFE_NO_PAD.encode(challenge.as_bytes()),
        "passkey challenge mismatch"
    );
    ensure!(
        data.origin == ORIGIN && !data.cross_origin,
        "passkey origin mismatch"
    );
    Ok(bytes)
}
fn authenticator_data(data: &[u8]) -> anyhow::Result<()> {
    ensure!(data.len() >= 37, "truncated authenticator data");
    ensure!(
        data[..32] == Sha256::digest(RP.as_bytes())[..],
        "passkey relying party mismatch"
    );
    ensure!(
        data[32] & 0x05 == 0x05,
        "passkey requires user presence and verification"
    );
    ensure!(data[32] & 0x80 == 0, "passkey extensions are unsupported");
    Ok(())
}
fn cbor(bytes: &[u8]) -> anyhow::Result<ciborium::Value> {
    let mut input = bytes;
    let value = ciborium::from_reader(&mut input)?;
    ensure!(input.is_empty(), "trailing CBOR data");
    Ok(value)
}
fn field<'a>(
    value: &'a ciborium::Value,
    key: &ciborium::Value,
) -> anyhow::Result<&'a ciborium::Value> {
    let entries = value.as_map().context("expected CBOR map")?;
    let mut matches = entries.iter().filter(|(k, _)| k == key);
    let (_, result) = matches.next().context("missing CBOR field")?;
    ensure!(matches.next().is_none(), "duplicate CBOR field");
    Ok(result)
}
impl NativePasskey {
    pub async fn register(name: &str) -> anyhow::Result<Self> {
        tracing::debug!("requesting native passkey registration");
        let challenge = Digest::from_bytes(crate::identity::random()?);
        let reply = request(serde_json::json!({
            "kind": "register", "name": name,
            "challenge": STANDARD.encode(challenge.as_bytes()),
            "user_id": STANDARD.encode(crate::identity::random::<32>()?),
        }))
        .await?;
        Self::registration(reply, challenge)
    }
    fn registration(reply: Reply, challenge: Digest) -> anyhow::Result<Self> {
        client_data(&reply.client_data_json, challenge, "webauthn.create")?;
        let credential_id = decode(&reply.credential_id, 1024)?;
        ensure!(!credential_id.is_empty(), "empty credential identifier");
        let object = cbor(&decode(&reply.attestation_object, 16384)?)?;
        let data = field(&object, &ciborium::Value::Text("authData".into()))?
            .as_bytes()
            .context("missing authenticator data")?;
        authenticator_data(data)?;
        ensure!(
            data[32] & 0x40 != 0 && data.len() >= 55,
            "missing attested credential data"
        );
        let length = u16::from_be_bytes([data[53], data[54]]) as usize;
        ensure!(
            data.get(55..55 + length) == Some(credential_id.as_slice()),
            "registration credential mismatch"
        );
        let key = cbor(
            data.get(55 + length..)
                .context("truncated credential key")?,
        )?;
        for (label, expected) in [(1, 2), (3, -7), (-1, 1)] {
            ensure!(
                field(&key, &ciborium::Value::Integer(label.into()))?
                    == &ciborium::Value::Integer(expected.into()),
                "passkey must use ES256/P-256"
            );
        }
        let coordinate = |label: i32| -> anyhow::Result<[u8; 32]> {
            Ok(field(&key, &ciborium::Value::Integer(label.into()))?
                .as_bytes()
                .context("missing key coordinate")?
                .as_slice()
                .try_into()?)
        };
        let public = PublicKey::from_coordinates(&coordinate(-2)?, &coordinate(-3)?)
            .map_err(|_| anyhow::anyhow!("invalid passkey public key"))?;
        // No attestation trust is claimed. Subsequent signed bootstrap/card assertions
        // establish possession of this key before an installation is committed.
        Ok(Self {
            credential_id,
            public_key: public.to_compressed().to_vec(),
        })
    }
    pub fn validate(&self) -> anyhow::Result<()> {
        ensure!(
            !self.credential_id.is_empty() && self.credential_id.len() <= 1024,
            "invalid credential identifier"
        );
        self.public_key()?;
        Ok(())
    }
    fn assertion(&self, reply: Reply, challenge: Digest) -> anyhow::Result<PasskeyAssertion> {
        ensure!(
            decode(&reply.credential_id, 1024)? == self.credential_id,
            "selected passkey differs from owner"
        );
        let client = client_data(&reply.client_data_json, challenge, "webauthn.get")?;
        let auth = decode(
            &reply.authenticator_data,
            hellas_webauthn::AUTHENTICATOR_DATA_LEN,
        )?;
        authenticator_data(&auth)?;
        let signature = hellas_webauthn::signature_from_der(&decode(&reply.signature, 80)?)
            .map_err(|_| anyhow::anyhow!("invalid passkey signature"))?;
        hellas_webauthn::verify(
            &hellas_webauthn::Assertion {
                signature: &signature,
                authenticator_data: &auth,
                client_data_json: &client,
            },
            &self.public_key()?,
            challenge.as_bytes(),
        )
        .map_err(|_| anyhow::anyhow!("passkey assertion verification failed"))?;
        Ok(PasskeyAssertion::new(signature, auth, client)?)
    }
}
#[async_trait::async_trait]
impl Authenticator for NativePasskey {
    fn public_key(&self) -> Result<PublicKey, AuthorityError> {
        PublicKey::from_sec1(&self.public_key).map_err(|_| AuthorityError::Credential)
    }
    async fn assert(&self, challenge: Digest) -> Result<PasskeyAssertion, AuthorityError> {
        tracing::debug!("requesting native passkey assertion");
        let reply = request(serde_json::json!({
            "kind": "assert", "challenge": STANDARD.encode(challenge.as_bytes()),
            "credential_id": STANDARD.encode(&self.credential_id),
        }))
        .await
        .map_err(|error| {
            tracing::warn!(%error, "native passkey approval failed");
            AuthorityError::Credential
        })?;
        self.assertion(reply, challenge).map_err(|error| {
            tracing::warn!(%error, "native passkey assertion rejected");
            AuthorityError::Credential
        })
    }
}
/// Stop pending prompts before shutdown waits for an operation holding a lock.
/// This is process-wide: Gate restarts by launching a new application process.
pub fn shutdown() {
    #[cfg(target_os = "macos")]
    unsafe {
        unsafe extern "C" {
            fn hellas_native_passkey_shutdown();
        }
        hellas_native_passkey_shutdown();
    }
}
#[cfg(not(target_os = "macos"))]
async fn request(_: serde_json::Value) -> anyhow::Result<Reply> {
    anyhow::bail!("Native passkeys currently require the signed macOS Gate app")
}
#[cfg(target_os = "macos")]
async fn request(value: serde_json::Value) -> anyhow::Result<Reply> {
    use std::ffi::{CStr, CString, c_char, c_void};
    type Sender = tokio::sync::oneshot::Sender<anyhow::Result<Reply>>;
    unsafe extern "C" {
        fn hellas_native_passkey_available() -> i32;
        fn hellas_native_passkey_start(
            request: *const c_char,
            callback: extern "C" fn(*mut c_void, *const c_char),
            context: *mut c_void,
        ) -> u64;
        fn hellas_native_passkey_cancel(token: u64);
    }
    extern "C" fn complete(context: *mut c_void, json: *const c_char) {
        // The bridge owns this sender after start and calls back exactly once,
        // including cancellation. The JSON allocation lives through this call.
        let sender = unsafe { Box::from_raw(context.cast::<Sender>()) };
        let result = (|| {
            ensure!(!json.is_null(), "empty native passkey reply");
            let bytes = unsafe { CStr::from_ptr(json) }.to_bytes();
            ensure!(bytes.len() <= 65536, "native passkey reply exceeds limit");
            let value: serde_json::Value = serde_json::from_slice(bytes)?;
            if let Some(error) = value.get("error").and_then(|v| v.as_str()) {
                anyhow::bail!("{error}");
            }
            Ok(serde_json::from_value(value)?)
        })();
        let _ = sender.send(result);
    }
    struct Pending(u64);
    impl Drop for Pending {
        fn drop(&mut self) {
            unsafe { hellas_native_passkey_cancel(self.0) };
        }
    }
    ensure!(
        unsafe { hellas_native_passkey_available() } != 0,
        "Native passkeys require a signed Gate app with the webcredentials:hellas.ai entitlement"
    );
    let request = CString::new(serde_json::to_vec(&value)?)?;
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let _pending = Pending(unsafe {
        hellas_native_passkey_start(
            request.as_ptr(),
            complete,
            Box::into_raw(Box::new(sender)).cast(),
        )
    });
    tokio::time::timeout(std::time::Duration::from_secs(120), receiver)
        .await
        .context("passkey approval timed out")?
        .context("passkey prompt closed")?
}

#[cfg(test)]
mod tests {
    use super::*;
    use p256::ecdsa::{Signature, SigningKey, signature::Signer};
    fn fixture() -> (SigningKey, NativePasskey, Digest) {
        let signer = SigningKey::from_bytes((&[7u8; 32]).into()).unwrap();
        let native = NativePasskey {
            credential_id: vec![1, 2, 3],
            public_key: signer
                .verifying_key()
                .to_sec1_point(true)
                .as_bytes()
                .to_vec(),
        };
        (signer, native, Digest::hash(b"one native owner approval"))
    }
    fn assertion(
        signer: &SigningKey,
        native: &NativePasskey,
        challenge: Digest,
        flags: u8,
    ) -> Reply {
        let client = serde_json::to_vec(&serde_json::json!({
            "type":"webauthn.get", "origin":ORIGIN,
            "challenge":URL_SAFE_NO_PAD.encode(challenge.as_bytes()), "crossOrigin":false,
        }))
        .unwrap();
        let mut auth = Sha256::digest(RP.as_bytes()).to_vec();
        auth.push(flags);
        auth.extend_from_slice(&0u32.to_be_bytes());
        let mut signed = auth.clone();
        signed.extend_from_slice(&Sha256::digest(&client));
        let signature: Signature = signer.sign(&signed);
        Reply {
            credential_id: STANDARD.encode(&native.credential_id),
            client_data_json: STANDARD.encode(client),
            attestation_object: String::new(),
            authenticator_data: STANDARD.encode(auth),
            signature: STANDARD.encode(signature.to_der().as_bytes()),
        }
    }
    #[test]
    fn native_assertion_verifies_and_rejects_wrong_challenge_key_credential_and_missing_uv() {
        let (signer, native, challenge) = fixture();
        let proof = native
            .assertion(assertion(&signer, &native, challenge, 5), challenge)
            .unwrap();
        assert_eq!(proof.authenticator_data()[32], 5);
        assert!(
            native
                .assertion(assertion(&signer, &native, challenge, 1), challenge)
                .is_err()
        );
        assert!(
            native
                .assertion(
                    assertion(&signer, &native, challenge, 5),
                    Digest::hash(b"different approval")
                )
                .is_err()
        );
        let foreign = SigningKey::from_bytes((&[8u8; 32]).into()).unwrap();
        assert!(
            native
                .assertion(assertion(&foreign, &native, challenge, 5), challenge)
                .is_err()
        );
        let mut reply = assertion(&signer, &native, challenge, 5);
        reply.credential_id = STANDARD.encode([9]);
        assert!(native.assertion(reply, challenge).is_err());
        let mut reply = assertion(&signer, &native, challenge, 5);
        reply.authenticator_data = STANDARD.encode([0u8; 37]);
        assert!(native.assertion(reply, challenge).is_err());
        let client = serde_json::json!({"type":"webauthn.get", "origin":"https://other.example", "challenge":URL_SAFE_NO_PAD.encode(challenge.as_bytes())});
        assert!(
            client_data(
                &STANDARD.encode(serde_json::to_vec(&client).unwrap()),
                challenge,
                "webauthn.get"
            )
            .is_err()
        );
    }
    #[test]
    fn registration_extracts_p256_key_and_rejects_mismatched_credential_or_algorithm() {
        use ciborium::Value as V;
        let (_, native, challenge) = fixture();
        let (x, y) = native.public_key().unwrap().to_coordinates();
        let key = V::Map(vec![
            (V::Integer(1.into()), V::Integer(2.into())),
            (V::Integer(3.into()), V::Integer((-7).into())),
            (V::Integer((-1).into()), V::Integer(1.into())),
            (V::Integer((-2).into()), V::Bytes(x.to_vec())),
            (V::Integer((-3).into()), V::Bytes(y.to_vec())),
        ]);
        let reply = |key: &V, credential: &[u8]| {
            let mut auth = Sha256::digest(RP.as_bytes()).to_vec();
            auth.push(0x45);
            auth.extend_from_slice(&[0; 20]); // counter + AAGUID
            auth.extend_from_slice(&(native.credential_id.len() as u16).to_be_bytes());
            auth.extend_from_slice(&native.credential_id);
            ciborium::into_writer(key, &mut auth).unwrap();
            let mut object = Vec::new();
            ciborium::into_writer(
                &V::Map(vec![(V::Text("authData".into()), V::Bytes(auth))]),
                &mut object,
            )
            .unwrap();
            Reply {
                credential_id: STANDARD.encode(credential),
                client_data_json: STANDARD.encode(serde_json::to_vec(&serde_json::json!({"type":"webauthn.create", "origin":ORIGIN, "challenge":URL_SAFE_NO_PAD.encode(challenge.as_bytes())})).unwrap()),
                attestation_object: STANDARD.encode(object),
                authenticator_data: String::new(), signature: String::new(),
            }
        };
        let parsed =
            NativePasskey::registration(reply(&key, &native.credential_id), challenge).unwrap();
        assert_eq!(parsed.public_key, native.public_key);
        assert_eq!(parsed.credential_id, native.credential_id);
        assert!(NativePasskey::registration(reply(&key, &[9]), challenge).is_err());
        let mut wrong = key;
        wrong.as_map_mut().unwrap()[1].1 = V::Integer((-8).into());
        assert!(
            NativePasskey::registration(reply(&wrong, &native.credential_id), challenge).is_err()
        );
    }
    #[cfg(not(target_os = "macos"))]
    #[tokio::test]
    async fn unsupported_platform_does_not_create_software_identity() {
        let directory = tempfile::tempdir().unwrap();
        let secrets = tempfile::tempdir().unwrap();
        let vault = crate::vault::Vault::fixture(secrets.path());
        assert!(
            crate::identity::Identity::create_native(directory.path(), &vault, "Alice".into())
                .await
                .is_err()
        );
        assert!(!directory.path().join("installation.json").exists());
        assert!(vault.required("owner-software-passkey").await.is_err());
    }
}
