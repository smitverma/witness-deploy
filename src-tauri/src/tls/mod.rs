use std::{
    collections::HashMap,
    fmt, fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
};

use rcgen::{
    BasicConstraints, Certificate, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa,
    KeyPair, KeyUsagePurpose,
};
use rustls::{
    crypto::aws_lc_rs::sign::any_supported_type,
    pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer},
    server::{ClientHello, ResolvesServerCert},
    sign::CertifiedKey,
    ClientConfig, RootCertStore, ServerConfig,
};
use serde::{Deserialize, Serialize};
use x509_parser::prelude::{FromDer, X509Certificate};

use crate::error::{Result, WitnessError};

const CA_CERT_NAME: &str = "witness-ca.pem";
const CA_KEY_NAME: &str = "witness-ca-key.pem";

/// Cached upstream TLS client config. Building a `RootCertStore` from
/// `webpki_roots` on every repeater send is wasteful; share one instance.
static CLIENT_CONFIG: OnceLock<Arc<ClientConfig>> = OnceLock::new();

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum CertificateStatus {
    Present,
    Missing,
    Expired,
}

pub struct CertificateAuthority {
    directory: PathBuf,
    certificate: Certificate,
    certificate_der: CertificateDer<'static>,
    key_pair: KeyPair,
    cache: Mutex<HashMap<String, Arc<CertifiedKey>>>,
}

impl fmt::Debug for CertificateAuthority {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CertificateAuthority")
            .field("directory", &self.directory)
            .finish_non_exhaustive()
    }
}

impl CertificateAuthority {
    pub fn load_or_create_default() -> Result<Arc<Self>> {
        Self::load_or_create(Self::default_directory()?)
    }

    pub fn default_directory() -> Result<PathBuf> {
        Ok(dirs::home_dir()
            .ok_or_else(|| WitnessError::Tls("cannot locate the home directory".into()))?
            .join(".witness")
            .join("certs"))
    }

    pub fn load_or_create(directory: impl Into<PathBuf>) -> Result<Arc<Self>> {
        let directory = directory.into();
        fs::create_dir_all(directory.join("hosts"))?;
        let cert_path = directory.join(CA_CERT_NAME);
        let key_path = directory.join(CA_KEY_NAME);

        let (certificate, certificate_der, key_pair) = if cert_path.exists() && key_path.exists() {
            let cert_pem = fs::read_to_string(&cert_path)?;
            let key_pem = fs::read_to_string(&key_path)?;
            let certificate_der = rustls_pemfile::certs(&mut cert_pem.as_bytes())
                .next()
                .transpose()?
                .ok_or_else(|| WitnessError::Tls("CA certificate file is empty".into()))?;
            let params = CertificateParams::from_ca_cert_pem(&cert_pem)
                .map_err(|error| WitnessError::Tls(error.to_string()))?;
            let key_pair = KeyPair::from_pem(&key_pem)
                .map_err(|error| WitnessError::Tls(error.to_string()))?;
            let certificate = params
                .self_signed(&key_pair)
                .map_err(|error| WitnessError::Tls(error.to_string()))?;
            (certificate, certificate_der, key_pair)
        } else {
            let pair = Self::generate_ca()?;
            fs::write(&cert_path, pair.0.pem())?;
            fs::write(&key_path, pair.1.serialize_pem())?;
            let certificate_der = pair.0.der().clone();
            (pair.0, certificate_der, pair.1)
        };

        Ok(Arc::new(Self {
            directory,
            certificate,
            certificate_der,
            key_pair,
            cache: Mutex::new(HashMap::new()),
        }))
    }

    fn generate_ca() -> Result<(Certificate, KeyPair)> {
        let mut params = CertificateParams::new(Vec::<String>::new())
            .map_err(|error| WitnessError::Tls(error.to_string()))?;
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        params
            .distinguished_name
            .push(DnType::CommonName, "Witness Local CA");
        params
            .distinguished_name
            .push(DnType::OrganizationName, "Witness Security");
        params.key_usages = vec![
            KeyUsagePurpose::DigitalSignature,
            KeyUsagePurpose::KeyCertSign,
            KeyUsagePurpose::CrlSign,
        ];
        let key_pair = KeyPair::generate().map_err(|error| WitnessError::Tls(error.to_string()))?;
        let certificate = params
            .self_signed(&key_pair)
            .map_err(|error| WitnessError::Tls(error.to_string()))?;
        Ok((certificate, key_pair))
    }

    pub fn status(directory: &Path) -> CertificateStatus {
        let cert_path = directory.join(CA_CERT_NAME);
        if !cert_path.exists() || !directory.join(CA_KEY_NAME).exists() {
            return CertificateStatus::Missing;
        }
        let valid = fs::read_to_string(cert_path)
            .ok()
            .and_then(|pem| rustls_pemfile::certs(&mut pem.as_bytes()).next()?.ok())
            .is_some_and(|der| {
                X509Certificate::from_der(der.as_ref())
                    .is_ok_and(|(_, certificate)| certificate.validity().is_valid())
            });
        if valid {
            CertificateStatus::Present
        } else {
            CertificateStatus::Expired
        }
    }

    pub fn ca_certificate_path(&self) -> PathBuf {
        self.directory.join(CA_CERT_NAME)
    }

    pub fn export_certificate(&self, destination: &Path) -> Result<()> {
        fs::copy(self.ca_certificate_path(), destination)?;
        Ok(())
    }

    pub fn certified_key(&self, host: &str) -> Result<Arc<CertifiedKey>> {
        let normalized = host.trim_matches(['[', ']']).to_ascii_lowercase();
        // Poison-tolerant: recover instead of panicking.
        if let Some(cached) = self
            .cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&normalized)
        {
            return Ok(cached.clone());
        }

        let mut params = CertificateParams::new(vec![normalized.clone()])
            .map_err(|error| WitnessError::Tls(error.to_string()))?;
        params
            .distinguished_name
            .push(DnType::CommonName, normalized.clone());
        params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        params.use_authority_key_identifier_extension = true;
        let leaf_key = KeyPair::generate().map_err(|error| WitnessError::Tls(error.to_string()))?;
        let leaf = params
            .signed_by(&leaf_key, &self.certificate, &self.key_pair)
            .map_err(|error| WitnessError::Tls(error.to_string()))?;

        let private_key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(leaf_key.serialize_der()));
        let signing_key = any_supported_type(&private_key)
            .map_err(|error| WitnessError::Tls(error.to_string()))?;
        let certified = Arc::new(CertifiedKey::new(
            vec![leaf.der().clone(), self.certificate_der.clone()],
            signing_key,
        ));

        // Filename uses a stable hash of the normalized host to avoid
        // collisions (e.g. `a/b` vs `a_b` previously mapped to the same
        // sanitized name). The in-memory cache key remains the normalized
        // host; only the on-disk name is hashed (with a readable prefix).
        let stem = host_file_stem(&normalized);
        let host_dir = self.directory.join("hosts");
        fs::write(host_dir.join(format!("{stem}.pem")), leaf.pem())?;
        fs::write(
            host_dir.join(format!("{stem}-key.pem")),
            leaf_key.serialize_pem(),
        )?;

        self.cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(normalized, certified.clone());
        Ok(certified)
    }

    pub fn server_config(self: &Arc<Self>) -> Arc<ServerConfig> {
        let resolver = Arc::new(DynamicCertResolver {
            authority: self.clone(),
        });
        Self::server_config_with_resolver(resolver)
    }

    /// Build a TLS server configuration for the authority requested by an
    /// HTTP CONNECT tunnel. CONNECT is authoritative even if the client
    /// omits SNI or encrypts it with ECH, so this keeps certificate selection
    /// correct for normal browser traffic.
    pub fn server_config_for_host(self: &Arc<Self>, host: &str) -> Result<Arc<ServerConfig>> {
        let resolver = Arc::new(FixedCertResolver {
            certified_key: self.certified_key(host)?,
        });
        Ok(Self::server_config_with_resolver(resolver))
    }

    fn server_config_with_resolver(resolver: Arc<dyn ResolvesServerCert>) -> Arc<ServerConfig> {
        let mut config = ServerConfig::builder()
            .with_no_client_auth()
            .with_cert_resolver(resolver);
        config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        Arc::new(config)
    }

    pub fn client_config() -> Arc<ClientConfig> {
        CLIENT_CONFIG
            .get_or_init(|| {
                let roots =
                    RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
                let mut config = ClientConfig::builder()
                    .with_root_certificates(roots)
                    .with_no_client_auth();
                config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
                Arc::new(config)
            })
            .clone()
    }

    pub fn ca_der(&self) -> CertificateDer<'static> {
        self.certificate_der.clone()
    }
}

/// Stable FNV-1a 64-bit hash rendered as 16 lowercase hex chars.
/// Used for on-disk leaf names so distinct hosts never collide after
/// sanitization. Implemented inline to avoid a new dependency.
fn fnv1a64_hex(input: &str) -> String {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0100_0000_01b3;
    let mut hash = OFFSET;
    for byte in input.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(PRIME);
    }
    format!("{hash:016x}")
}

fn host_file_stem(normalized: &str) -> String {
    let safe: String = normalized
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '-') {
                character
            } else {
                '_'
            }
        })
        .collect();
    let prefix: String = safe.chars().take(32).collect();
    format!("{prefix}-{}", fnv1a64_hex(normalized))
}

#[derive(Debug)]
pub struct DynamicCertResolver {
    authority: Arc<CertificateAuthority>,
}

impl ResolvesServerCert for DynamicCertResolver {
    fn resolve(&self, client_hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        let host = client_hello.server_name()?;
        self.authority.certified_key(host).ok()
    }
}

#[derive(Debug)]
struct FixedCertResolver {
    certified_key: Arc<CertifiedKey>,
}

impl ResolvesServerCert for FixedCertResolver {
    fn resolve(&self, _: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        Some(self.certified_key.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_persists_and_loads_ca() {
        let directory = tempfile::tempdir().unwrap();
        let first = CertificateAuthority::load_or_create(directory.path()).unwrap();
        assert_eq!(
            CertificateAuthority::status(directory.path()),
            CertificateStatus::Present
        );
        let original = first.ca_der();
        drop(first);
        let loaded = CertificateAuthority::load_or_create(directory.path()).unwrap();
        assert_eq!(loaded.ca_der(), original);
    }

    #[test]
    fn generated_leaf_is_cached() {
        let directory = tempfile::tempdir().unwrap();
        let authority = CertificateAuthority::load_or_create(directory.path()).unwrap();
        let first = authority.certified_key("example.test").unwrap();
        let second = authority.certified_key("example.test").unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        let stem = host_file_stem("example.test");
        assert!(directory.path().join(format!("hosts/{stem}.pem")).exists());
    }

    #[test]
    fn distinct_hosts_never_share_a_filename() {
        // `a/b` and `a_b` sanitize identically; the hash suffix must differ.
        assert_ne!(host_file_stem("a/b"), host_file_stem("a_b"));
        assert_ne!(
            host_file_stem("EXAMPLE.test"),
            host_file_stem("example.test.")
        );
    }
}
