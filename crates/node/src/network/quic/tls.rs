use crate::BoxError;
use crate::identity::Uuid;
use quinn::crypto::rustls::{QuicClientConfig, QuicServerConfig};
use quinn::{ClientConfig, ServerConfig};
use rcgen::generate_simple_self_signed;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, Error as RustlsError, SignatureScheme};
use std::sync::Arc;

/// Generates a self-signed P2P TLS certificate and private key in DER format for a given set of SANs.
pub fn generate_self_signed_cert(
    subject_alt_names: Vec<String>,
) -> Result<(CertificateDer<'static>, PrivateKeyDer<'static>), BoxError> {
    let cert = generate_simple_self_signed(subject_alt_names)?;
    let cert_der = CertificateDer::from(cert.cert.der().to_vec());
    let key_der = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(cert.key_pair.serialize_der()));
    Ok((cert_der, key_der))
}

/// Generates an ephemeral self-signed P2P TLS certificate and private key bound to a persistent [`Uuid`].
///
/// The node's unique ID is embedded into the Subject Alternative Names (SAN) of the X.509 certificate.
pub fn generate_node_cert(
    node_uuid: Uuid,
) -> Result<(CertificateDer<'static>, PrivateKeyDer<'static>), BoxError> {
    let node_hex = format!("{node_uuid}");
    let subject_alt_names = vec![
        node_hex.clone(),
        format!("node-{node_hex}"),
        "localhost".to_string(),
        "127.0.0.1".to_string(),
    ];
    generate_self_signed_cert(subject_alt_names)
}

/// Web-of-Trust P2P Server Certificate Verifier.
///
/// Skips Web PKI CA chain verification and validates peer certificate integrity
/// directly for peer-to-peer authenticated communication.
#[derive(Debug)]
pub struct P2pServerCertVerifier {
    supported_schemes: Vec<SignatureScheme>,
}

impl Default for P2pServerCertVerifier {
    fn default() -> Self {
        Self::new()
    }
}

impl P2pServerCertVerifier {
    pub fn new() -> Self {
        Self {
            supported_schemes: rustls::crypto::ring::default_provider()
                .signature_verification_algorithms
                .supported_schemes(),
        }
    }
}

impl ServerCertVerifier for P2pServerCertVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, RustlsError> {
        let expected = server_name.to_str();
        // In P2P Web of Trust: If connect_node specifies an expected node identity (UUID or "node-<uuid>"),
        // enforce that the peer's X.509 certificate SAN contains the expected identity.
        if expected.starts_with("node-") || (expected.len() == 32 && expected.is_ascii()) {
            let cert_bytes = end_entity.as_ref();
            let san_names = extract_san_names(cert_bytes);
            let matches = if !san_names.is_empty() {
                let exp_str = expected.as_ref();
                san_names.iter().any(|name| {
                    name.as_str() == exp_str
                        || (exp_str.starts_with("node-") && name.as_str() == exp_str.trim_start_matches("node-"))
                        || (name.starts_with("node-") && name.trim_start_matches("node-") == exp_str)
                })
            } else {
                let needle = expected.as_bytes();
                cert_bytes.windows(needle.len()).any(|window| window == needle)
            };

            if !matches {
                return Err(RustlsError::InvalidCertificate(
                    rustls::CertificateError::NotValidForName,
                ));
            }
        }

        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &rustls::crypto::ring::default_provider().signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &rustls::crypto::ring::default_provider().signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.supported_schemes.clone()
    }
}

/// Builds a Quinn [`ServerConfig`] using the specified self-signed P2P TLS certificate and key.
pub fn build_p2p_server_config(
    cert: CertificateDer<'static>,
    key: PrivateKeyDer<'static>,
) -> Result<ServerConfig, BoxError> {
    let mut server_crypto = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()?
    .with_no_client_auth()
    .with_single_cert(vec![cert], key)?;

    server_crypto.alpn_protocols = vec![b"aaron-p2p/1".to_vec()];

    let quic_server_config = QuicServerConfig::try_from(server_crypto)?;
    let server_config = ServerConfig::with_crypto(Arc::new(quic_server_config));

    Ok(server_config)
}

/// Builds a Quinn [`ClientConfig`] using the Web-of-Trust P2P TLS certificate verifier.
pub fn build_p2p_client_config() -> Result<ClientConfig, BoxError> {
    let mut client_crypto = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()?
    .dangerous()
    .with_custom_certificate_verifier(Arc::new(P2pServerCertVerifier::new()))
    .with_no_client_auth();

    client_crypto.alpn_protocols = vec![b"aaron-p2p/1".to_vec()];

    let quic_client_config = QuicClientConfig::try_from(client_crypto)?;
    let client_config = ClientConfig::new(Arc::new(quic_client_config));

    Ok(client_config)
}

/// Extracts all Subject Alternative Names (dNSNames and iPAddresses) from an X.509 DER certificate.
pub fn extract_san_names(cert_der: &[u8]) -> Vec<String> {
    let mut names = Vec::new();
    // Subject Alternative Name OID: 2.5.29.17 -> 06 03 55 1d 11
    let san_oid = [0x06, 0x03, 0x55, 0x1d, 0x11];
    let mut pos = 0;
    while let Some(found) = cert_der[pos..].windows(san_oid.len()).position(|w| w == san_oid) {
        let mut idx = pos + found + san_oid.len();
        // Optional critical boolean: 01 01 00 or 01 01 ff
        if idx + 3 <= cert_der.len() && cert_der[idx] == 0x01 && cert_der[idx + 1] == 0x01 {
            idx += 3;
        }
        // ExtnValue: OCTET STRING 0x04
        if idx < cert_der.len() && cert_der[idx] == 0x04 {
            idx += 1;
            let (octet_len, next_idx) = parse_asn1_length(cert_der, idx);
            idx = next_idx;
            let end = (idx + octet_len).min(cert_der.len());
            // Inside octet string, GeneralNames is a SEQUENCE 0x30
            if idx < end && cert_der[idx] == 0x30 {
                idx += 1;
                let (_, seq_idx) = parse_asn1_length(cert_der, idx);
                idx = seq_idx;
                while idx < end {
                    let tag = cert_der[idx];
                    idx += 1;
                    let (item_len, next_idx) = parse_asn1_length(cert_der, idx);
                    idx = next_idx;
                    if idx + item_len <= end {
                        // 0x82 is Context-Specific Tag [2] for dNSName
                        if (tag == 0x82 || tag == 0x87)
                            && let Ok(s) = std::str::from_utf8(&cert_der[idx..idx + item_len])
                        {
                            names.push(s.to_string());
                        }
                        idx += item_len;
                    } else {
                        break;
                    }
                }
            }
        }
        pos += found + san_oid.len();
    }
    names
}

fn parse_asn1_length(buf: &[u8], mut idx: usize) -> (usize, usize) {
    if idx >= buf.len() {
        return (0, idx);
    }
    let b = buf[idx];
    idx += 1;
    if b < 0x80 {
        (b as usize, idx)
    } else {
        let num_bytes = (b & 0x7f) as usize;
        let mut len = 0usize;
        for _ in 0..num_bytes {
            if idx < buf.len() {
                len = (len << 8) | (buf[idx] as usize);
                idx += 1;
            }
        }
        (len, idx)
    }
}
