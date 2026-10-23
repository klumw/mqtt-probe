//! MQTT over WebSocket transport and URL helpers.
//!
//! rumqttc expects the complete WebSocket URL as the broker address; the
//! separate port argument is ignored for this transport.

use rumqttc::Transport;

use crate::config::{ConnectionProfile, DEFAULT_WEBSOCKET_PATH};

use super::tls;

/// Builds the URL expected by rumqttc, e.g. `ws://host:8000/mqtt`.
pub fn url_for(profile: &ConnectionProfile) -> String {
    let scheme = if profile.tls { "wss" } else { "ws" };
    let host = bracket_ipv6(profile.host.trim());
    format!(
        "{scheme}://{host}:{}{}",
        profile.port,
        normalize_path(&profile.websocket_path)
    )
}

/// Normalizes the WebSocket path: an empty value becomes `/mqtt` and a missing
/// leading slash is added.
pub fn normalize_path(path: &str) -> String {
    let path = path.trim();
    if path.is_empty() {
        return DEFAULT_WEBSOCKET_PATH.to_owned();
    }
    if path.starts_with('/') {
        path.to_owned()
    } else {
        format!("/{path}")
    }
}

fn bracket_ipv6(host: &str) -> String {
    if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_owned()
    }
}

/// Builds the WebSocket transport, reusing the TCP TLS roots for `wss`.
pub fn transport_for(profile: &ConnectionProfile) -> Result<Transport, String> {
    if profile.tls {
        Ok(Transport::wss_with_config(tls::tls_config_for(profile)?))
    } else {
        Ok(Transport::ws())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn websocket_urls_use_the_selected_scheme_and_path() {
        let mut profile = ConnectionProfile {
            host: "broker.example".to_owned(),
            port: 8000,
            websocket: true,
            websocket_path: "mqtt".to_owned(),
            ..ConnectionProfile::default()
        };
        assert_eq!(url_for(&profile), "ws://broker.example:8000/mqtt");

        profile.tls = true;
        assert_eq!(url_for(&profile), "wss://broker.example:8000/mqtt");
    }

    #[test]
    fn empty_or_relative_paths_are_normalized() {
        assert_eq!(normalize_path(""), "/mqtt");
        assert_eq!(normalize_path("   "), "/mqtt");
        assert_eq!(normalize_path("custom"), "/custom");
        assert_eq!(normalize_path("/custom"), "/custom");
    }

    #[test]
    fn ipv6_hosts_are_bracketed() {
        let profile = ConnectionProfile {
            host: "::1".to_owned(),
            port: 8000,
            websocket: true,
            websocket_path: "/mqtt".to_owned(),
            ..ConnectionProfile::default()
        };
        assert_eq!(url_for(&profile), "ws://[::1]:8000/mqtt");
    }
}
