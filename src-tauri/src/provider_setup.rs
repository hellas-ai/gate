//! Provider assembly is typechecked on every host platform.
use crate::{dto::ProviderConfig, state::decode_record};
use anyhow::Context as _;
use hellas_rpc::protocol::work_grant::{
    budget::{Limit, Meter, Window},
    records::Principal,
    resource::HttpsResource,
};
use std::{
    num::NonZeroU64,
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct HttpsRoute {
    service: String,
    method: String,
    account: hellas_sdk::HttpProviderConfig,
    resource: Option<Resource>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Resource {
    name: String,
    https: HttpsResource,
}

pub async fn start<R: hellas_sdk::RootProver + Send + Sync + 'static>(
    config: ProviderConfig,
    identity: hellas_sdk::ClientIdentity,
    enrollment: hellas_rpc::ProviderEnrollmentBundle,
    root: Arc<R>,
    state_directory: PathBuf,
    chain: &crate::chain::ChainNode,
) -> anyhow::Result<hellas_sdk::ProviderHandle> {
    anyhow::ensure!(
        config.contacts.len() <= 256,
        "at most 256 contacts are allowed"
    );
    let grantees = config
        .contacts
        .iter()
        .map(|record| Ok(Principal::decode(&decode_record(record, 32 * 1024)?)?))
        .collect::<anyhow::Result<Vec<_>>>()?;
    let paid_work = config
        .work_config_path
        .as_deref()
        .map(|path| hellas_sdk::work_config::load_work_config(Path::new(path)))
        .transpose()?;
    let paid_work = match paid_work {
        Some(config) => {
            let node = chain.get(&config).await?;
            Some(hellas_sdk::PaidProviderOptions::new(config, node)?)
        }
        None => None,
    };
    let mut routes = hellas_sdk::FetchRouteRegistry::new();
    let mut policies = vec![];
    if !config.openai_api_key.trim().is_empty() {
        policies.push(hellas_sdk::responses_policy(
            &config.service,
            &config.method,
        )?);
        routes.register(
            hellas_sdk::FetchRoute::new(&config.service, &config.method),
            hellas_sdk::FetchRouteEntry::new(
                Arc::new(hellas_sdk::OpenAiResponsesFetchProvider::with_bearer(
                    config.openai_api_key,
                )?),
                Arc::new(hellas_sdk::ResponsesFetchAdaptorFactory::new(
                    hellas_rpc::FetchEnvironment::OpenAiResponses,
                )),
                hellas_sdk::FetchRoutePolicy::default(),
            )?,
        )?;
    }
    if let Some(json) = config.https_config {
        anyhow::ensure!(
            json.len() <= 1 << 20,
            "HTTPS configuration exceeds its byte limit"
        );
        let entries: Vec<HttpsRoute> =
            serde_json::from_str(&json).context("invalid HTTPS routes configuration")?;
        for entry in entries {
            if let Some(resource) = entry.resource {
                let mut policy = hellas_sdk::responses_policy(&entry.service, &entry.method)?;
                policy.name = resource.name;
                policy.https = Some(resource.https);
                if let hellas_rpc::protocol::work_profile::WorkPolicy::Fetch { policy, .. } =
                    &mut policy.work
                {
                    policy.allowed_environment = hellas_rpc::FetchEnvironment::Http.manifest_id();
                }
                policies.push(policy);
            }
            routes.register(
                hellas_sdk::FetchRoute::new(entry.service, entry.method),
                entry
                    .account
                    .into_entry(hellas_sdk::FetchRoutePolicy::default())?,
            )?;
        }
    }
    // Keep the grant store mounted even with an empty contact list so removals
    // revoke existing grants before the paid listener is opened.
    hellas_sdk::start_fetch_provider(hellas_sdk::FetchProviderOptions {
        port: config.port,
        identity,
        enrollment,
        root,
        state_directory,
        routes,
        grants: Some(hellas_sdk::GrantProviderOptions {
            grantees,
            policies,
            limits: vec![Limit {
                meter: Meter::Requests,
                window: Window::Day,
                amount: config.requests_per_day.into(),
            }],
            max_job_millis: NonZeroU64::new(300_000).unwrap(),
        }),
        paid_work,
        fetch_max_in_flight: hellas_rpc::DEFAULT_FETCH_MAX_IN_FLIGHT,
        fetch_queue_capacity: hellas_rpc::DEFAULT_FETCH_QUEUE_CAPACITY,
    })
    .await
    .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    struct SoftwareRoot;
    impl hellas_sdk::RootProver for SoftwareRoot {
        async fn prove_statement(
            &self,
            _: &[u8],
        ) -> Result<hellas_rpc::RootProof, hellas_attestation::AttestationError> {
            panic!("enrollment already signed")
        }
        async fn prove_open_binding(
            &self,
            _: hellas_rpc::Digest,
        ) -> Result<hellas_rpc::RootProof, hellas_attestation::AttestationError> {
            panic!("software Open uses producer")
        }
    }

    #[tokio::test]
    async fn provider_exports_responses_and_https_resources_from_one_runtime() {
        let directory = tempfile::tempdir().unwrap();
        let identity = hellas_sdk::ClientIdentity::generate();
        let enrollment = identity
            .contact_enrollment(&hellas_rpc::ProducerSigningKey::generate())
            .unwrap();
        let template = HttpsResource {
            origin: "https://example.com".into(),
            paths: vec!["/v1/chat/completions".into()],
            methods: vec!["POST".into()],
            credential: None,
            tls: hellas_rpc::http_fetch::HttpTls {
                roots: hellas_rpc::http_fetch::HttpTrustRoots::WebPki,
                spki_sha256: vec![],
            },
            accounting: hellas_rpc::http_usage::AccountingProfile::OpenaiChat,
            max_output_tokens: 100,
            max_response_bytes: 4096,
        };
        let chain = crate::chain::ChainNode::new(directory.path().join("chain"));
        let provider = start(ProviderConfig {
            service: "openai".into(), method: "responses".into(), openai_api_key: "fixture".into(),
            work_config_path: None,
            https_config: Some(serde_json::json!([{
                "service": "lan", "method": "chat", "account": {"allowed_hosts":["example.com"]},
                "resource": {"name":"chat", "https":template}
            }]).to_string()),
            contacts: vec![STANDARD.encode(enrollment.canonical_bytes())], requests_per_day: 100, port: Some(0),
        }, identity, enrollment, Arc::new(SoftwareRoot), directory.path().join("provider"), &chain).await.unwrap();
        let offers = provider.offers().unwrap();
        assert_eq!(offers.len(), 1);
        let policies = &offers[0].offer().grant.policies;
        assert_eq!(policies.len(), 2);
        assert_eq!(policies[0].name, "responses");
        assert_eq!(policies[1].https.as_ref(), Some(&template));
        provider.shutdown().await;
        assert!(
            !directory.path().join("chain").exists(),
            "authorized-only provider starts no chain node"
        );
    }
}
