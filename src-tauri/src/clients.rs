//! Run and HTTP targets are declared through the daemon's Configure/Watch surface.
use crate::{contacts, control::LocalNode, dto::*, identity, paid, trust, vault};
use anyhow::{Context, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use hellas_auth::authority::*;
use hellas_gateway::{WorkExecutionBackend, WorkFetchRequest};
use hellas_sdk::{ExecutorGateway, grant_client::UnpinnedOffer};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

pub struct Target {
    pub id: String,
    pub backend: Arc<ExecutorGateway>,
    pub service: String,
    pub method: String,
    pub presentation: FetchPresentation,
}
pub(crate) struct ConfiguredTarget {
    pub id: String,
    pub backend: hellas_sdk::daemon_config::Backend,
    pub service: String,
    pub method: String,
    pub presentation: FetchPresentation,
}
enum Selection {
    Authorized {
        access: contacts::Imported,
        offer: Box<hellas_authority::model::grant::records::SignedOffer>,
        policy: Box<hellas_authority::model::grant::records::GrantPolicy>,
        trust: Box<hellas_sdk::client::ProviderTrustAnchor>,
    },
    Paid {
        selected: Box<PaidProviderInput>,
        providers: Vec<paid::ProviderSnapshot>,
    },
}
fn backend_id(channel: &str) -> String {
    format!(
        "client-{}",
        hex::encode(&hellas_auth::Digest::hash(channel.as_bytes()).as_bytes()[..24])
    )
}
fn route(
    policy: &hellas_rpc::protocol::invocation::InvocationPolicy,
) -> anyhow::Result<(String, String, FetchPresentation, hellas_rpc::ContentId)> {
    use hellas_rpc::protocol::invocation::{Admission, fetch::FetchRoutePolicy};
    let [Admission::Fetch { admission }] = policy.admissions.as_slice() else {
        anyhow::bail!("select a resource with one named Fetch route");
    };
    let FetchRoutePolicy::SealedRoute { service, method } = &admission.route else {
        anyhow::bail!("select a named Fetch resource");
    };
    Ok((
        service.clone(),
        method.clone(),
        match admission.contract.schema() {
            hellas_rpc::FetchSchema::HttpV1 => FetchPresentation::Http,
            hellas_rpc::FetchSchema::StatelessResponsesV1 => FetchPresentation::Responses,
            hellas_rpc::FetchSchema::ResponsesItemsV1 => {
                anyhow::bail!("this Fetch schema has no supported Gate presentation")
            }
        },
        admission.contract.manifest_id(),
    ))
}

pub struct Clients {
    directory: PathBuf,
}
impl Clients {
    pub fn new(directory: &Path) -> Self {
        Self {
            directory: directory.into(),
        }
    }
    fn selection(&self, local: &LocalNode, input: &WorkClientConfig) -> anyhow::Result<Selection> {
        match &input.target {
            WorkTarget::Authorized {
                access_id,
                allowance,
                resource,
            } => {
                let access = contacts::imported(&self.directory, access_id)?;
                ensure!(
                    access.subject == local.identity.public.owner
                        && access.holder
                            == NodeKey::from_public_key(
                                local.identity.node.node_key().public_key()
                            )?,
                    "imported access belongs to another person or device"
                );
                ensure!(
                    access.expires > UnixMillis::now()?,
                    "imported access expired"
                );
                let offer = access
                    .offers
                    .iter()
                    .map(|s| {
                        Ok(
                            hellas_authority::model::grant::records::SignedOffer::decode(
                                &STANDARD.decode(s)?,
                                UnixMillis::now()?,
                            )?,
                        )
                    })
                    .collect::<anyhow::Result<Vec<_>>>()?
                    .into_iter()
                    .find(|o| o.offer().grant.id.to_string() == *allowance)
                    .context("allowance is absent from this access bundle")?;
                let selected = offer
                    .offer()
                    .grant
                    .policies
                    .iter()
                    .find(|p| p.name == *resource)
                    .context("selected resource is absent from this allowance")?
                    .clone();
                route(&selected.invocation_policy)?;
                let trust = trust::resolve(&self.directory, &input.trust_policy)?;
                Ok(Selection::Authorized {
                    access,
                    offer: Box::new(offer),
                    policy: Box::new(selected),
                    trust: Box::new(trust),
                })
            }
            WorkTarget::Paid {
                pool_id,
                provider,
                service,
                method,
            } => {
                let pool: PaidPoolInput = vault::read(&paid::pool_path(&self.directory, pool_id)?)?
                    .context("configure a typed paid pool first")?;
                let providers = paid::pool_snapshot(&self.directory, &pool)?;
                let selected = pool
                    .providers
                    .iter()
                    .find(|p| p.provider == *provider)
                    .context("provider is absent from paid pool")?
                    .clone();
                ensure!(
                    selected.trust_policy == input.trust_policy,
                    "Run trust policy must match selected pool provider"
                );
                ensure!(
                    *service == selected.policy.service && *method == selected.policy.method,
                    "paid route differs from selected policy"
                );
                Ok(Selection::Paid {
                    selected: Box::new(selected),
                    providers,
                })
            }
        }
    }
    pub(crate) async fn configuration(
        &self,
        local: &LocalNode,
        input: &WorkClientConfig,
    ) -> anyhow::Result<ConfiguredTarget> {
        use hellas_sdk::daemon_config::{Backend, BackendKind, Trust};
        let trust = Some(Trust {
            store: self.directory.join("trust"),
            policy: input.trust_policy.clone(),
        });
        let (id, kind, service, method, presentation) = match self.selection(local, input)? {
            Selection::Authorized {
                access,
                offer,
                policy,
                trust: approved,
            } => {
                let (service, method, presentation, _) = route(&policy.invocation_policy)?;
                UnpinnedOffer::decode(&offer.encode()?, UnixMillis::now()?)?.pin(&approved)?;
                let bundle = hellas_sdk::access_bundle::AccessBundle {
                    offers: vec![*offer],
                    authority: access.authority,
                    authorization: access.authorization,
                    subject: access.subject,
                    holder: access.holder,
                    credential: identity::decode_credential(
                        &local.vault.required(&access.vault_key()).await?,
                    )?,
                    expires: access.expires,
                    label: None,
                };
                (
                    backend_id(&format!(
                        "grant:{}:{}",
                        access.authority,
                        bundle.offers[0].offer().grant.id
                    )),
                    BackendKind::Grant {
                        access: Some(STANDARD.encode(bundle.encode()?)),
                        resource: Some(policy.resource),
                        evaluator: None,
                        trust,
                    },
                    service,
                    method,
                    presentation,
                )
            }
            Selection::Paid {
                selected,
                providers,
                ..
            } => {
                let snapshot = providers
                    .into_iter()
                    .find(|p| p.input.provider == selected.provider)
                    .context("paid provider is missing")?;
                (
                    backend_id(&format!(
                        "paid:{}:{}",
                        snapshot.profile.state_key(),
                        selected.provider
                    )),
                    BackendKind::Paid {
                        addresses: selected
                            .addresses
                            .iter()
                            .map(|address| address.parse())
                            .collect::<Result<_, _>>()?,
                        acceptance_blocks: selected.acceptance_blocks,
                        terminal_blocks: selected.terminal_blocks,
                        payment_blocks: selected.payment_blocks,
                        timeout_seconds: Some(selected.timeout_seconds),
                        provider: selected.provider,
                        chain: Some(snapshot.profile.to_file()),
                        max_price: selected.policy.fixed_price,
                        evaluator: None,
                        trust,
                    },
                    selected.policy.service,
                    selected.policy.method,
                    selected.policy.presentation,
                )
            }
        };
        Ok(ConfiguredTarget {
            id,
            backend: Backend {
                max_in_flight: 4.try_into()?,
                kind,
            },
            service,
            method,
            presentation,
        })
    }
    pub async fn target(
        &self,
        local: &LocalNode,
        input: &WorkClientConfig,
    ) -> anyhow::Result<Arc<Target>> {
        let selected = self.configuration(local, input).await?;
        let session = local.session(true).await?;
        let view = session
            .edit(None, |document| {
                document
                    .backends
                    .insert(selected.id.clone(), selected.backend);
                Ok(())
            })
            .await?;
        ensure!(
            view.document.backends.contains_key(&selected.id),
            "configured backend is absent from Watch"
        );
        let backend = local.core().execution();
        let active = backend
            .backend(&selected.id)
            .context("configured backend is unavailable")?;
        ensure!(active.fetch_routes().iter().any(|(schema, route)| {
            matches!(route, hellas_rpc::protocol::invocation::fetch::FetchRoutePolicy::SealedRoute { service, method }
                if *service == selected.service && *method == selected.method)
                && matches!((schema, selected.presentation),
                    (hellas_rpc::FetchSchema::HttpV1, FetchPresentation::Http)
                    | (hellas_rpc::FetchSchema::StatelessResponsesV1, FetchPresentation::Responses))
        }), "quoted route differs from selected policy");
        Ok(Arc::new(Target {
            id: selected.id,
            backend,
            service: selected.service,
            method: selected.method,
            presentation: selected.presentation,
        }))
    }
    pub async fn save_pool(&self, input: PaidPoolInput) -> anyhow::Result<()> {
        paid::save_pool(&self.directory, input)
    }
    pub async fn fetch(
        &self,
        local: &LocalNode,
        input: &RunRequest,
    ) -> anyhow::Result<hellas_gateway::WorkOutputStream<hellas_rpc::output::OutputEvent>> {
        let target = self.target(local, &input.client).await?;
        Ok(target.backend.fetch(
            &target.id,
            target.backend.jobs.admit(&hellas_gateway::jobs::Request {
                origin: hellas_gateway::jobs::Origin::Local,
                method: "POST".into(),
                path: "/run".into(),
            })?,
            WorkFetchRequest {
                service: target.service.clone(),
                method: target.method.clone(),
                body: input.input.as_bytes().to_vec(),
            },
        )?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hellas_rpc::protocol::invocation::{
        Admission, InvocationPolicy, Limits,
        fetch::{FetchAdmission, FetchLimits, FetchRoutePolicy},
    };

    fn policy(contract: hellas_rpc::FetchContract) -> InvocationPolicy {
        InvocationPolicy {
            admissions: vec![Admission::Fetch {
                admission: FetchAdmission {
                    contract,
                    route: FetchRoutePolicy::sealed_route("service", "method").unwrap(),
                    limits: FetchLimits {
                        max_request_body_bytes: 64 << 10,
                        max_output_events: 4096,
                        max_spool_bytes: 8 << 20,
                        max_encoded_result_frame: 4 << 20,
                        max_encoded_prepared_input: 128 << 10,
                    },
                },
            }],
            limits: Limits {
                max_upload_bytes: 128 << 10,
                max_output_bytes: 4 << 20,
                max_steps: 1,
                execution_timeout_ms: 300_000,
            },
        }
    }

    #[test]
    fn presentation_refuses_unsupported_or_ambiguous_fetch_admissions() {
        let http = hellas_rpc::FetchContract::http().unwrap();
        let responses = hellas_adaptors::openai::responses::fetch::responses_contract().unwrap();
        for (contract, presentation) in [
            (http.clone(), FetchPresentation::Http),
            (responses, FetchPresentation::Responses),
        ] {
            let selected = policy(contract.clone());
            selected.validate().unwrap();
            let (_, _, actual, environment) = route(&selected).unwrap();
            assert_eq!(actual, presentation);
            assert_eq!(environment, contract.manifest_id());
        }
        let unsupported = policy(
            hellas_rpc::FetchContract::new(
                "responses-items-0.0.1",
                hellas_rpc::FetchSchema::ResponsesItemsV1,
                "responses-driver-0.0.1",
                hellas_rpc::FetchEgress::FixedHttps("https://api.example.com/responses".into()),
            )
            .unwrap(),
        );
        unsupported.validate().unwrap();
        assert!(route(&unsupported).is_err());

        let mut ambiguous = policy(http);
        ambiguous.admissions.push(Admission::Compute {
            evaluator: hellas_rpc::protocol::invocation::compute::EVALUATOR.into(),
            programs: hellas_rpc::protocol::invocation::ComputePrograms::Any {},
        });
        ambiguous.validate().unwrap();
        assert!(route(&ambiguous).is_err());
    }
}
