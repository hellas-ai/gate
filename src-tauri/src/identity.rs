use std::fs;
use std::io::{ErrorKind, Write};
use std::path::Path;

use anyhow::Context;
use hellas_sdk::ClientIdentity;

const TAG: &[u8] = b"hellas.gate.client.v1\0";
const SECRET_BYTES: usize = 64;

/// Load the stable transport/caller identity owned by this installation, or
/// create it exactly once. The format is intentionally tiny and private to
/// Gate; provider enrollment is a separate, attested record.
pub fn load_or_create(path: &Path) -> anyhow::Result<ClientIdentity> {
    match fs::read(path) {
        Ok(bytes) => decode(&bytes),
        Err(error) if error.kind() == ErrorKind::NotFound => create(path),
        Err(error) => Err(error).with_context(|| format!("reading {} failed", path.display())),
    }
}

fn create(path: &Path) -> anyhow::Result<ClientIdentity> {
    let identity = ClientIdentity::generate();
    let mut bytes = Vec::with_capacity(TAG.len() + SECRET_BYTES);
    bytes.extend_from_slice(TAG);
    bytes.extend_from_slice(&identity.transport_secret_bytes());
    bytes.extend_from_slice(&identity.caller_secret_bytes());

    decode(&persist(path, &bytes)?)
}

pub fn contact(
    path: &Path,
    identity: &ClientIdentity,
) -> anyhow::Result<hellas_rpc::protocol::work_grant::records::Principal> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == ErrorKind::NotFound => persist(
            path,
            &hellas_rpc::ProducerSigningKey::generate().to_secret_bytes(),
        )?,
        Err(error) => return Err(error.into()),
    };
    let root = hellas_rpc::ProducerSigningKey::from_secret_bytes(
        bytes
            .try_into()
            .map_err(|_| anyhow::anyhow!("invalid contact root"))?,
    )?;
    Ok(
        hellas_rpc::protocol::work_grant::records::Principal::verify(
            identity.contact_enrollment(&root)?,
        )?,
    )
}

fn persist(path: &Path, bytes: &[u8]) -> anyhow::Result<Vec<u8>> {
    let directory = path.parent().context("identity path has no parent")?;
    let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
    #[cfg(unix)]
    temporary.as_file().set_permissions({
        use std::os::unix::fs::PermissionsExt;
        fs::Permissions::from_mode(0o600)
    })?;
    temporary.write_all(bytes)?;
    temporary.flush()?;
    temporary.as_file().sync_all()?;
    match temporary.persist_noclobber(path) {
        Ok(_) => {
            #[cfg(unix)]
            fs::File::open(directory)?.sync_all()?;
            Ok(bytes.to_vec())
        }
        Err(_) if path.exists() => Ok(fs::read(path)?),
        Err(error) => Err(error.error).context("persisting Gate identity failed"),
    }
}

fn decode(bytes: &[u8]) -> anyhow::Result<ClientIdentity> {
    anyhow::ensure!(
        bytes.len() == TAG.len() + SECRET_BYTES && bytes.starts_with(TAG),
        "unsupported Gate identity format"
    );
    let transport = bytes[TAG.len()..TAG.len() + 32].try_into().unwrap();
    let caller = bytes[TAG.len() + 32..].try_into().unwrap();
    ClientIdentity::from_secret_bytes(transport, caller).map_err(Into::into)
}

pub fn producer_id(identity: &ClientIdentity) -> String {
    encode_hex(identity.caller_key().producer_id().as_bytes())
}

pub fn public_key(identity: &ClientIdentity) -> String {
    encode_hex(identity.caller_key().public_key().bytes())
}

fn encode_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(DIGITS[(byte >> 4) as usize] as char);
        output.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_is_stable_and_private() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("identity");
        let first = load_or_create(&path).unwrap();
        let second = load_or_create(&path).unwrap();
        assert_eq!(first.node_id(), second.node_id());
        assert_eq!(producer_id(&first), producer_id(&second));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn contact_root_survives_restart_and_is_distinct_from_the_producer() {
        let directory = tempfile::tempdir().unwrap();
        let identity = load_or_create(&directory.path().join("identity")).unwrap();
        let path = directory.path().join("contact-root");
        let first = contact(&path, &identity).unwrap();
        let second = contact(&path, &identity).unwrap();
        assert_eq!(first, second);
        assert_ne!(
            first.bundle().genesis.statement.root_public_key,
            identity.caller_key().public_key()
        );
        assert_eq!(first.transport(), *identity.node_id().as_bytes());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn rejects_unknown_format() {
        assert!(decode(b"not an identity").is_err());
    }
}
