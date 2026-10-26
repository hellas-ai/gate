//! Startup transport preferences are public local metadata; HostHandle owns the socket.
use serde::{Deserialize, Serialize};
use std::path::Path;
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub listen_port: Option<u16>,
}
pub fn load(directory: &Path) -> anyhow::Result<Settings> {
    Ok(crate::vault::read(&directory.join("settings.json"))?.unwrap_or_default())
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
    crate::vault::write(
        &directory.join("settings.json"),
        &Settings { listen_port: port },
    )
}
