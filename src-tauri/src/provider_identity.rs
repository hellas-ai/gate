mod platform {
    use std::fs;
    use std::io::{ErrorKind, Write};
    use std::path::Path;
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};

    use anyhow::{Context, bail};
    use hellas_attestation::{
        AnchorTime, AppleCredential, ApplePolicy, RegisteredAppleCredential, RootProver,
        apple_app_attest_root_ca, apple_client_data_hash, apple_credential_identity,
        register_apple, verify_apple_assertion,
    };
    use hellas_rpc::{
        AppleAppAttestEnrollment, DagCborDecoder, DagCborEncoder, Digest, PlatformCredential,
        PlatformEnrollment, ProviderEnrollmentBundle, ProviderGenesisStatement, PublicKey,
        RootKind, RootProof, SignedProviderGenesis,
    };
    use hellas_sdk::ClientIdentity;

    use crate::apple::AppleAppAttestRoot;

    const TAG: &str = "hellas.gate.provider-identity.v1";
    const VERSION: u64 = 1;

    pub struct ProviderIdentity {
        root: Arc<AppleAppAttestRoot>,
        enrollment: ProviderEnrollmentBundle,
    }

    impl ProviderIdentity {
        pub fn enrollment(&self) -> &ProviderEnrollmentBundle {
            &self.enrollment
        }

        pub fn root(&self) -> Arc<AppleAppAttestRoot> {
            self.root.clone()
        }
    }

    pub async fn load_or_create(
        path: &Path,
        client: &ClientIdentity,
    ) -> anyhow::Result<ProviderIdentity> {
        match fs::read(path) {
            Ok(bytes) => materialize(&bytes, client),
            Err(error) if error.kind() == ErrorKind::NotFound => create(path, client).await,
            Err(error) => Err(error).with_context(|| format!("reading {} failed", path.display())),
        }
    }

    async fn create(path: &Path, client: &ClientIdentity) -> anyhow::Result<ProviderIdentity> {
        let installation_nonce = installation_nonce(client);
        let enrollment_statement = enrollment_statement(installation_nonce);
        let (root, credential) =
            tokio::task::spawn_blocking(move || AppleAppAttestRoot::create(&enrollment_statement))
                .await
                .context("Apple App Attest enrollment task failed")??;
        let validation_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("system clock is before the Unix epoch")?
            .as_secs();
        let credential_identity = apple_credential_identity(&credential.attestation)?;
        let registered = register_apple(
            &credential,
            credential_identity.rp_id_hash,
            apple_app_attest_root_ca(),
            AnchorTime(validation_time),
        )?;
        let statement = provider_statement(client, installation_nonce, &credential, &registered);
        let genesis = SignedProviderGenesis {
            root_proof: root.prove_statement(&statement.canonical_bytes()).await?,
            statement,
        };
        let enrollment = ProviderEnrollmentBundle {
            genesis,
            platform: PlatformEnrollment::AppleAppAttest(AppleAppAttestEnrollment {
                attestation_object: credential.attestation,
                client_data_hash: credential.client_data_hash,
                validation_time,
            }),
        };
        validate(&enrollment, client)?;
        let stored = encode(root.key(), &enrollment);
        persist(path, &stored)?;
        Ok(ProviderIdentity {
            root: Arc::new(root),
            enrollment,
        })
    }

    fn materialize(bytes: &[u8], client: &ClientIdentity) -> anyhow::Result<ProviderIdentity> {
        let (key, enrollment) = decode(bytes)?;
        validate(&enrollment, client)?;
        let PlatformEnrollment::AppleAppAttest(platform) = &enrollment.platform else {
            bail!("Gate provider identity does not contain Apple enrollment");
        };
        let credential = AppleCredential {
            attestation: platform.attestation_object.clone(),
            client_data_hash: platform.client_data_hash,
        };
        Ok(ProviderIdentity {
            root: Arc::new(AppleAppAttestRoot::load(key, credential.content_id())),
            enrollment,
        })
    }

    fn validate(
        enrollment: &ProviderEnrollmentBundle,
        client: &ClientIdentity,
    ) -> anyhow::Result<()> {
        let PlatformEnrollment::AppleAppAttest(platform) = &enrollment.platform else {
            bail!("Gate provider identity requires Apple App Attest enrollment");
        };
        let credential = AppleCredential {
            attestation: platform.attestation_object.clone(),
            client_data_hash: platform.client_data_hash,
        };
        let credential_identity = apple_credential_identity(&credential.attestation)?;
        let registered = register_apple(
            &credential,
            credential_identity.rp_id_hash,
            apple_app_attest_root_ca(),
            AnchorTime(platform.validation_time),
        )?;
        let statement = &enrollment.genesis.statement;
        anyhow::ensure!(
            statement.root_kind == RootKind::SecureEnclave
                && statement.root_public_key == PublicKey::P256(registered.public_key)
                && statement.producer_public_key == client.caller_key().public_key()
                && statement.transport_public_key
                    == PublicKey::Ed25519(*client.node_id().as_bytes())
                && statement.platform_credential
                    == PlatformCredential::Registered(credential.content_id()),
            "persisted Apple provider identity does not match this Gate installation"
        );
        let RootProof::AppleAppAttest(assertion) = &enrollment.genesis.root_proof else {
            bail!("Apple provider identity requires an App Attest root proof");
        };
        verify_apple_assertion(
            assertion,
            &apple_client_data_hash(&statement.canonical_bytes()),
            &RegisteredAppleCredential {
                id: credential.content_id(),
                public_key: registered.public_key,
            },
            &ApplePolicy {
                expected_rp_id_hash: credential_identity.rp_id_hash,
                allowed_cd_hashes: vec![credential_identity.cd_hash],
            },
        )?;
        Ok(())
    }

    fn provider_statement(
        client: &ClientIdentity,
        installation_nonce: [u8; 32],
        credential: &AppleCredential,
        registered: &RegisteredAppleCredential,
    ) -> ProviderGenesisStatement {
        ProviderGenesisStatement {
            root_kind: RootKind::SecureEnclave,
            root_public_key: PublicKey::P256(registered.public_key),
            producer_public_key: client.caller_key().public_key(),
            transport_public_key: PublicKey::Ed25519(*client.node_id().as_bytes()),
            platform_credential: PlatformCredential::Registered(credential.content_id()),
            installation_nonce,
        }
    }

    fn installation_nonce(client: &ClientIdentity) -> [u8; 32] {
        let mut encoder = DagCborEncoder::new();
        encoder.array(3);
        encoder.str("hellas.gate.installation-nonce.v1");
        encoder.bytes(&client.transport_secret_bytes());
        encoder.bytes(&client.caller_secret_bytes());
        *Digest::hash(&encoder.into_bytes()).as_bytes()
    }

    fn enrollment_statement(installation_nonce: [u8; 32]) -> Vec<u8> {
        let mut encoder = DagCborEncoder::new();
        encoder.array(2);
        encoder.str("hellas.apple.app-attest.enrollment.v1");
        encoder.bytes(&installation_nonce);
        encoder.into_bytes()
    }

    fn encode(key: &str, enrollment: &ProviderEnrollmentBundle) -> Vec<u8> {
        let mut encoder = DagCborEncoder::new();
        encoder.array(4);
        encoder.str(TAG);
        encoder.u64(VERSION);
        encoder.str(key);
        encoder.bytes(&enrollment.canonical_bytes());
        encoder.into_bytes()
    }

    fn decode(bytes: &[u8]) -> anyhow::Result<(String, ProviderEnrollmentBundle)> {
        let mut decoder = DagCborDecoder::new(bytes);
        decoder.array(4, "Gate provider identity")?;
        decoder.tag(TAG)?;
        anyhow::ensure!(
            decoder.u64("Gate provider identity version")? == VERSION,
            "unsupported Gate provider identity version"
        );
        let key = decoder.text("Apple App Attest key")?.to_owned();
        let enrollment =
            ProviderEnrollmentBundle::from_canonical_bytes(decoder.bytes("provider enrollment")?)?;
        decoder.finish()?;
        anyhow::ensure!(
            encode(&key, &enrollment) == bytes,
            "Gate provider identity is not canonical"
        );
        Ok((key, enrollment))
    }

    fn persist(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
        let directory = path
            .parent()
            .context("provider identity path has no parent")?;
        let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
        // On Windows the file inherits the data directory's owner-only DACL
        // (create_private_directory); there is no mode to set.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            temporary
                .as_file()
                .set_permissions(fs::Permissions::from_mode(0o600))?;
        }
        temporary.write_all(bytes)?;
        temporary.flush()?;
        temporary.as_file().sync_all()?;
        match temporary.persist_noclobber(path) {
            Ok(_) => {
                // Directory fsync has no Windows counterpart.
                #[cfg(unix)]
                fs::File::open(directory)?.sync_all()?;
                Ok(())
            }
            Err(_) if path.exists() => Ok(()),
            Err(error) => Err(error.error).context("persisting provider identity failed"),
        }
    }
}

pub use platform::{ProviderIdentity, load_or_create};
