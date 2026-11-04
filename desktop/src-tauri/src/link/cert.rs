//! **Yap Link's certificate**: the phone's page needs HTTPS for its
//! microphone (a page on `http://192.168.x.x` never gets one), so Yap serves
//! it on a self-signed certificate made here once (rcgen, ECDSA P-256) and
//! kept in the data folder's `link/` (`cert.der`, `key.der`, `cert.json`:
//! the names it's for and when it ends). Its names are this PC's addresses
//! on its networks and `<name>.local`; made again when an address it doesn't
//! name becomes the QR's, or a month before it ends (two years). The phone
//! shows Chrome's "Your connection is not private" once (Advanced →
//! Proceed): no certificate authority vouches for a PC at home.

use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

/// The folder in the data folder.
pub const DIR: &str = "link";

/// What `cert.json` says about the certificate.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct About {
    /// Every name and address it's for.
    pub names: Vec<String>,
    /// Its last day (unix seconds).
    pub until: u64,
}

fn dir() -> PathBuf {
    crate::config::data_dir().join(DIR)
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// How long one lasts, and how long before its end it's made again.
const LASTS_DAYS: i64 = 730;
const RENEW_SECS: u64 = 30 * 86_400;

/// The names a certificate for this PC is for: its reachable addresses,
/// `<name>.local`, and the loopback ones (the page opened on the PC). Pure.
pub fn names_for(addresses: &[IpAddr], pc: &str) -> Vec<String> {
    let mut out: Vec<String> = addresses.iter().map(IpAddr::to_string).collect();
    let host: String = pc.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect::<String>().to_ascii_lowercase();
    if !host.is_empty() {
        out.push(format!("{host}.local"));
    }
    out.extend(["localhost".to_string(), "127.0.0.1".to_string(), "::1".to_string()]);
    let mut seen = std::collections::HashSet::new();
    out.retain(|n| seen.insert(n.clone()));
    out
}

/// Does the kept certificate still do: it names `wanted` (the QR's
/// address) and isn't near its end? Pure.
pub fn still_good(about: &About, wanted: &str, now: u64) -> bool {
    about.names.iter().any(|n| n == wanted) && about.until > now + RENEW_SECS
}

/// A fresh certificate and its key (DER), for `names`.
pub fn make(names: &[String]) -> Result<(Vec<u8>, Vec<u8>, About), String> {
    use chrono::Datelike;
    let err = |e: rcgen::Error| format!("Couldn't make Yap Link's certificate: {e}");
    let mut params = rcgen::CertificateParams::new(names.to_vec()).map_err(err)?;
    let mut dn = rcgen::DistinguishedName::new();
    dn.push(rcgen::DnType::CommonName, "Yap on this PC");
    dn.push(rcgen::DnType::OrganizationName, "Yap Link");
    params.distinguished_name = dn;
    let from = chrono::Utc::now().date_naive() - chrono::Duration::days(1);
    let to = from + chrono::Duration::days(LASTS_DAYS);
    params.not_before = rcgen::date_time_ymd(from.year(), from.month() as u8, from.day() as u8);
    params.not_after = rcgen::date_time_ymd(to.year(), to.month() as u8, to.day() as u8);
    let key = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).map_err(err)?;
    let cert = params.self_signed(&key).map_err(err)?;
    let until = to.and_hms_opt(0, 0, 0).map(|t| t.and_utc().timestamp().max(0) as u64).unwrap_or(0);
    Ok((cert.der().to_vec(), key.serialize_der(), About { names: names.to_vec(), until }))
}

/// The certificate the server uses: the kept one while it names `wanted`,
/// else a new one (kept for next time). `wanted`: the QR's address.
pub fn load_or_make(wanted: IpAddr, addresses: &[IpAddr], pc: &str) -> Result<(Vec<u8>, Vec<u8>), String> {
    let d = dir();
    let read = || -> Option<(Vec<u8>, Vec<u8>, About)> {
        let about: About = serde_json::from_slice(&std::fs::read(d.join("cert.json")).ok()?).ok()?;
        Some((std::fs::read(d.join("cert.der")).ok()?, std::fs::read(d.join("key.der")).ok()?, about))
    };
    if let Some((cert, key, about)) = read() {
        if still_good(&about, &wanted.to_string(), now_secs()) && !cert.is_empty() && !key.is_empty() {
            return Ok((cert, key));
        }
        tracing::info!("Yap Link: a new certificate (this PC's address changed, or the old one ends soon)");
    }
    let mut all: Vec<IpAddr> = vec![wanted];
    all.extend(addresses.iter().copied());
    let (cert, key, about) = make(&names_for(&all, pc))?;
    let write = || -> std::io::Result<()> {
        std::fs::create_dir_all(&d)?;
        crate::config::atomic_write_bytes(&d.join("key.der"), &key)?;
        crate::config::atomic_write_bytes(&d.join("cert.der"), &cert)?;
        crate::config::atomic_write(&d.join("cert.json"), &serde_json::to_string_pretty(&about).unwrap_or_default())
    };
    if let Err(e) = write() {
        // It still serves; the next start makes another (one more warning on the phone).
        tracing::warn!("Yap Link: couldn't keep its certificate ({e})");
    }
    Ok((cert, key))
}

/// The TLS setup for `cert` and `key` (DER): rustls on ring, HTTP/1.1.
pub fn server_config(cert: Vec<u8>, key: Vec<u8>) -> Result<Arc<rustls::ServerConfig>, String> {
    use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut config = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| format!("Yap Link's TLS: {e}"))?
        .with_no_client_auth()
        .with_single_cert(vec![CertificateDer::from(cert)], PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key)))
        .map_err(|e| format!("Yap Link's certificate didn't load: {e}"))?;
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(Arc::new(config))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_certificate_names_the_pcs_addresses_and_its_local_name() {
        let ips: Vec<IpAddr> = vec!["192.168.1.20".parse().unwrap(), "fd00::20".parse().unwrap(), "192.168.1.20".parse().unwrap()];
        let names = names_for(&ips, "DESKTOP-Ab 12");
        assert_eq!(names, ["192.168.1.20", "fd00::20", "desktop-ab12.local", "localhost", "127.0.0.1", "::1"]);
        let about = About { names: names.clone(), until: 10_000_000 };
        assert!(still_good(&about, "192.168.1.20", 1_000));
        assert!(!still_good(&about, "192.168.1.21", 1_000), "a new address: a new certificate");
        assert!(!still_good(&about, "192.168.1.20", 10_000_000 - 86_400), "near its end");
    }

    #[test]
    fn a_made_certificate_loads_into_tls() {
        let (cert, key, about) = make(&names_for(&["192.168.1.20".parse().unwrap()], "desk")).unwrap();
        assert!(!cert.is_empty() && !key.is_empty());
        assert!(about.until > now_secs() + 700 * 86_400);
        assert!(server_config(cert, key).is_ok());
    }
}
