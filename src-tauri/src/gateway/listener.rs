use crate::{control::LocalNode, dto::FetchPresentation};
use anyhow::Context;
use hellas_sdk::node_control::host::GatewayFactory;
use std::sync::Arc;

pub fn factory(local: Arc<LocalNode>) -> GatewayFactory {
    Arc::new(move |access| {
        let local = local.clone();
        Box::pin(async move {
            let result: anyhow::Result<hellas_gateway::GatewayHandle> = async {
                let config = local
                    .gateway
                    .get()
                    .await
                    .context("configure a gateway target first")?;
                let http = super::validate(&local, &config)?;
                let required = if http.is_some() {
                    FetchPresentation::Http
                } else {
                    FetchPresentation::Responses
                };
                let target = local
                    .clients
                    .target_for(&local, &config.client, Some(required))
                    .await?;
                let archive = hellas_gateway::ArchiveOptions {
                    directory: local.directory.join("gateway-archive"),
                    zdr: config.zdr,
                };
                let responses = http.is_none().then(|| hellas_gateway::ResponsesFetch {
                    provider: target.provider,
                    service: target.service.clone(),
                    method: target.method.clone(),
                    overrides: Default::default(),
                    models: vec![],
                });
                let handle = hellas_gateway::start(hellas_gateway::GatewayOptions {
                    server: hellas_gateway::ServerOptions {
                        access,
                        archive,
                        allow_remote: false,
                        host: "127.0.0.1".into(),
                        port: config.port,
                        wrap: None,
                        wrap_args: vec![],
                    },
                    work: target.backend.clone(),
                    presentations: hellas_gateway::Presentations {
                        models: vec![],
                        responses,
                        http,
                    },
                })
                .await?;
                Ok(handle)
            }
            .await;
            result.map_err(anyhow::Error::into_boxed_dyn_error)
        })
    })
}
