//! TLS transport setup with an optional CA certificate and client certificate.

use std::io::BufReader;
use std::sync::Arc;

use rumqttc::{TlsConfiguration, Transport};
use rustls::RootCertStore;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};

use crate::config::ConnectionProfile;

/// Builds the TLS transport for a profile.
pub fn transport_for(profile: &ConnectionProfile) -> Result<Transport, String> {
    Ok(Transport::tls_with_config(tls_config_for(profile)?))
}

/// Builds the TLS configuration shared by the TLS and secure WebSocket
/// transports.
///
/// Without a configured CA certificate this trusts the platform roots. With a
/// certificate file, the platform roots are extended by the configured
/// certificate, which may contain one or more PEM certificates or a single DER
/// certificate. A configured client certificate and key enable mutual TLS.
pub fn tls_config_for(profile: &ConnectionProfile) -> Result<TlsConfiguration, String> {
    let ca_path = profile.ca_cert_path.trim();
    let cert_path = profile.client_cert_path.trim();
    let key_path = profile.client_key_path.trim();

    match (cert_path.is_empty(), key_path.is_empty()) {
        (true, true) => {}
        (false, false) => {}
        _ => return Err("both a client certificate and a client key are required".to_owned()),
    }
    let uses_client_auth = !cert_path.is_empty();

    if ca_path.is_empty() && !uses_client_auth {
        return Ok(TlsConfiguration::default());
    }

    let mut roots = RootCertStore::empty();
    let native = rustls_native_certs::load_native_certs();
    for certificate in native.certs {
        // Some platform stores contain certificates that rustls cannot use as
        // trust anchors; keep the remaining ones.
        let _ = roots.add(certificate);
    }
    if !ca_path.is_empty() {
        for certificate in load_certificate_chain(ca_path)? {
            roots
                .add(certificate)
                .map_err(|err| format!("invalid CA certificate: {err}"))?;
        }
    }
    if roots.is_empty() {
        return Err("no trusted root certificates available".to_owned());
    }

    let builder = rustls::ClientConfig::builder().with_root_certificates(roots);
    let config = if uses_client_auth {
        let certificates = load_certificate_chain(cert_path)?;
        let key = load_private_key(key_path)?;
        builder
            .with_client_auth_cert(certificates, key)
            .map_err(|err| format!("invalid client certificate or key: {err}"))?
    } else {
        builder.with_no_client_auth()
    };
    Ok(TlsConfiguration::Rustls(Arc::new(config)))
}

/// Checks that a CA certificate file exists and contains usable certificates.
/// Used by the profile editor to report mistakes before connecting.
pub fn validate_ca_certificate(path: &str) -> Result<(), String> {
    let certificates = load_certificate_chain(path)?;
    let mut roots = RootCertStore::empty();
    for certificate in certificates {
        roots
            .add(certificate)
            .map_err(|err| format!("invalid CA certificate: {err}"))?;
    }
    if roots.is_empty() {
        return Err("no certificates found".to_owned());
    }
    Ok(())
}

/// Checks a client certificate/key pair from the profile editor. Both fields
/// must either be empty or point to parseable files.
pub fn validate_client_credentials(cert_path: &str, key_path: &str) -> Result<(), String> {
    let cert_path = cert_path.trim();
    let key_path = key_path.trim();
    match (cert_path.is_empty(), key_path.is_empty()) {
        (true, true) => Ok(()),
        (false, false) => {
            load_certificate_chain(cert_path)?;
            load_private_key(key_path)?;
            Ok(())
        }
        _ => Err("both a client certificate and a client key are required".to_owned()),
    }
}

fn load_certificate_chain(path: &str) -> Result<Vec<CertificateDer<'static>>, String> {
    let bytes = std::fs::read(path).map_err(|err| err.to_string())?;
    if bytes.is_empty() {
        return Err("file is empty".to_owned());
    }

    let mut reader = BufReader::new(std::io::Cursor::new(&bytes));
    let mut certificates = Vec::new();
    for certificate in rustls_pemfile::certs(&mut reader) {
        certificates.push(certificate.map_err(|err| err.to_string())?);
    }

    // Also accept a single DER-encoded certificate.
    if certificates.is_empty() {
        certificates.push(CertificateDer::from(bytes));
    }

    if certificates.is_empty() {
        return Err("no certificates found".to_owned());
    }
    Ok(certificates)
}

fn load_private_key(path: &str) -> Result<PrivateKeyDer<'static>, String> {
    let bytes = std::fs::read(path).map_err(|err| err.to_string())?;
    if bytes.is_empty() {
        return Err("file is empty".to_owned());
    }

    let mut reader = BufReader::new(std::io::Cursor::new(&bytes));
    if let Some(key) = rustls_pemfile::private_key(&mut reader).map_err(|err| err.to_string())? {
        return Ok(key);
    }

    // Also accept a DER-encoded PKCS#1, PKCS#8, or SEC1 private key.
    PrivateKeyDer::try_from(bytes).map_err(|err| format!("invalid private key: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Self-signed fixture (CN "mqtt-probe-test") for the TLS tests below.
    // It is not a credential for any real service.
    const TEST_CLIENT_CERT: &str = "-----BEGIN CERTIFICATE-----
MIIBiTCCAS+gAwIBAgIUBPhbktqpCD6xrvr+jHzwGsMBx6IwCgYIKoZIzj0EAwIw
GjEYMBYGA1UEAwwPbXF0dC1wcm9iZS10ZXN0MB4XDTI2MTAyMDE1NTEzN1oXDTM2
MTAxNzE1NTEzN1owGjEYMBYGA1UEAwwPbXF0dC1wcm9iZS10ZXN0MFkwEwYHKoZI
zj0CAQYIKoZIzj0DAQcDQgAEMPYbVbTVTXDhGSrwjnVXMt0rpNUbk9cG5bDDqVl6
68eusoRV6f6HwhW17dZSpnw3UYx+P1KAWwmiPtU8mInNgKNTMFEwHQYDVR0OBBYE
FJSve2FsiR2YLPabJYHperXmUU9XMB8GA1UdIwQYMBaAFJSve2FsiR2YLPabJYHp
erXmUU9XMA8GA1UdEwEB/wQFMAMBAf8wCgYIKoZIzj0EAwIDSAAwRQIgZFfA2b5k
QMTRFdjK2K8Xk2rEXXxl48jXg9Y1aCaKVawCIQDgUEHAE40BPRyszVZpFv7EH3/0
sKB7r/gqy8balgaH4w==
-----END CERTIFICATE-----
";

    const TEST_CLIENT_KEY: &str = "-----BEGIN EC PRIVATE KEY-----
MHcCAQEEIEJ+ax/jVr7udbUyWYi94zjI7sOG0yM1wuF+DUFOb0k4oAoGCCqGSM49
AwEHoUQDQgAEMPYbVbTVTXDhGSrwjnVXMt0rpNUbk9cG5bDDqVl668eusoRV6f6H
whW17dZSpnw3UYx+P1KAWwmiPtU8mInNgA==
-----END EC PRIVATE KEY-----
";

    fn temp_file(name: &str, contents: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("mqtt-probe-tls-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn missing_ca_certificate_is_reported() {
        let path = "/definitely/not/a/certificate.pem";
        assert!(load_certificate_chain(path).is_err());
    }

    #[test]
    fn empty_ca_path_keeps_default_transport() {
        let profile = ConnectionProfile::default();
        assert!(transport_for(&profile).is_ok());
    }

    #[test]
    fn client_certificate_and_key_must_both_be_set() {
        let mut profile = ConnectionProfile {
            client_cert_path: "/tmp/client.pem".to_owned(),
            ..ConnectionProfile::default()
        };
        assert!(tls_config_for(&profile).is_err());

        profile.client_cert_path.clear();
        profile.client_key_path = "/tmp/client-key.pem".to_owned();
        assert!(tls_config_for(&profile).is_err());
    }

    #[test]
    fn valid_client_credentials_are_accepted() {
        let cert = temp_file("client.pem", TEST_CLIENT_CERT);
        let key = temp_file("client-key.pem", TEST_CLIENT_KEY);

        assert!(validate_client_credentials(cert.to_str().unwrap(), key.to_str().unwrap()).is_ok());

        let _ = std::fs::remove_dir_all(cert.parent().unwrap());
    }

    #[test]
    fn missing_client_key_is_reported() {
        let cert = temp_file("client.pem", TEST_CLIENT_CERT);
        let missing = cert.with_file_name("missing-key.pem");

        assert!(
            validate_client_credentials(cert.to_str().unwrap(), missing.to_str().unwrap()).is_err()
        );

        let _ = std::fs::remove_dir_all(cert.parent().unwrap());
    }

    #[test]
    fn client_credentials_with_crt_and_key_suffixes_build_a_tls_configuration() {
        // The file suffix does not matter: PEM/DER content is detected when
        // the file is read, so `.crt` certificates and `.key` keys work.
        let cert = temp_file("client.crt", TEST_CLIENT_CERT);
        let key = temp_file("client.key", TEST_CLIENT_KEY);

        let profile = ConnectionProfile {
            tls: true,
            // The fixture is self-signed, so it also serves as its own root.
            ca_cert_path: cert.to_str().unwrap().to_owned(),
            client_cert_path: cert.to_str().unwrap().to_owned(),
            client_key_path: key.to_str().unwrap().to_owned(),
            ..ConnectionProfile::default()
        };
        assert!(matches!(
            tls_config_for(&profile),
            Ok(TlsConfiguration::Rustls(_))
        ));

        let _ = std::fs::remove_dir_all(cert.parent().unwrap());
    }
}
