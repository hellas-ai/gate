//! Startup transport preferences are public local metadata; HostHandle owns the socket.
use serde::{Deserialize, Serialize};
use std::path::Path;
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub listen_port: Option<u16>,
}
pub fn load(directory: &Path) -> anyhow::Result<Settings> {
    let path = crate::provider_setup::configuration_path(directory)?;
    if !path.try_exists()? && std::env::var_os("HELLAS_GATE_CONFIG").is_none() {
        return Ok(Settings::default());
    }
    Ok(Settings {
        listen_port: hellas_sdk::serve_config::ServeConfig::load(&path)?.port,
    })
}
pub fn save(directory: &Path, port: Option<u16>, current: &[String]) -> anyhow::Result<()> {
    let port = port.filter(|p| *p != 0);
    if let Some(port) = port {
        let already_bound = current.iter().any(|a| {
            a.parse::<std::net::SocketAddr>()
                .is_ok_and(|a| a.is_ipv4() && a.port() == port)
        });
        if !already_bound {
            let _probe = std::net::UdpSocket::bind((std::net::Ipv4Addr::UNSPECIFIED, port))?;
        }
    }
    let path = crate::provider_setup::configuration_path(directory)?;
    let mut document = if path.try_exists()? {
        hellas_sdk::serve_config::ServeConfig::load(&path)?
    } else {
        hellas_sdk::serve_config::ServeConfig::empty()
    };
    document.port = port;
    document.revision = document
        .revision
        .checked_add(1)
        .ok_or_else(|| anyhow::anyhow!("configuration revision overflow"))?;
    crate::vault::write(&path, &document)
}
