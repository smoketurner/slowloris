//! Connection abstraction over plain TCP and (optionally) TLS.
//!
//! Connections are blocking with per-socket read/write timeouts. The traffic
//! pattern here is tiny (a few bytes per socket per interval), so a single
//! thread iterating the connection list is more than fast enough and keeps the
//! binary small and dependency-light.

use std::io::{self, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use socket2::{Domain, Protocol, Socket, Type};

use crate::cli::Target;

/// A live connection to the target.
pub enum Stream {
    Plain(TcpStream),
    #[cfg(feature = "tls")]
    Tls(Box<rustls::StreamOwned<rustls::ClientConnection, TcpStream>>),
}

impl Write for Stream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Stream::Plain(s) => s.write(buf),
            #[cfg(feature = "tls")]
            Stream::Tls(s) => s.write(buf),
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        match self {
            Stream::Plain(s) => s.flush(),
            #[cfg(feature = "tls")]
            Stream::Tls(s) => s.flush(),
        }
    }
}

impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Stream::Plain(s) => s.read(buf),
            #[cfg(feature = "tls")]
            Stream::Tls(s) => s.read(buf),
        }
    }
}

/// Builds connections to a fixed target with shared settings.
pub struct Connector {
    timeout: Duration,
    /// When set, `SO_RCVBUF` is shrunk to this many bytes (for slow-read).
    recv_buffer: Option<usize>,
    #[cfg(feature = "tls")]
    tls: Option<std::sync::Arc<rustls::ClientConfig>>,
}

impl Connector {
    pub fn new(
        target: &Target,
        timeout: Duration,
        recv_buffer: Option<usize>,
        insecure: bool,
    ) -> Result<Self, String> {
        #[cfg(feature = "tls")]
        let tls = if target.tls {
            Some(tls::client_config(insecure)?)
        } else {
            None
        };

        #[cfg(not(feature = "tls"))]
        if target.tls {
            let _ = insecure;
            return Err(
                "this binary was built without TLS support; rebuild with the `tls` feature".into(),
            );
        }

        Ok(Connector {
            timeout,
            recv_buffer,
            #[cfg(feature = "tls")]
            tls,
        })
    }

    /// Open one connection (TCP connect + TLS handshake, if applicable).
    pub fn connect(&self, target: &Target) -> io::Result<Stream> {
        let addr = (target.host.as_str(), target.port)
            .to_socket_addrs()?
            .next()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no addresses for host"))?;

        let domain = Domain::for_address(addr);
        let sock = Socket::new(domain, Type::STREAM, Some(Protocol::TCP))?;
        sock.connect_timeout(&addr.into(), self.timeout)?;
        sock.set_read_timeout(Some(self.timeout))?;
        sock.set_write_timeout(Some(self.timeout))?;
        sock.set_nodelay(true)?;
        if let Some(sz) = self.recv_buffer {
            // Best effort: the kernel may clamp this to a floor.
            let _ = sock.set_recv_buffer_size(sz);
        }

        let tcp: TcpStream = sock.into();

        #[cfg(feature = "tls")]
        if let Some(cfg) = &self.tls {
            let server_name = rustls::pki_types::ServerName::try_from(target.host.clone())
                .map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidInput, "invalid TLS server name")
                })?;
            let conn = rustls::ClientConnection::new(cfg.clone(), server_name)
                .map_err(io::Error::other)?;
            return Ok(Stream::Tls(Box::new(rustls::StreamOwned::new(conn, tcp))));
        }

        Ok(Stream::Plain(tcp))
    }
}

#[cfg(feature = "tls")]
mod tls {
    use std::sync::Arc;

    use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
    use rustls::crypto::aws_lc_rs;
    use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
    use rustls::{ClientConfig, DigitallySignedStruct, Error, RootCertStore, SignatureScheme};

    pub fn client_config(insecure: bool) -> Result<Arc<ClientConfig>, String> {
        let provider = Arc::new(aws_lc_rs::default_provider());

        if insecure {
            let cfg = ClientConfig::builder_with_provider(provider)
                .with_safe_default_protocol_versions()
                .map_err(|e| e.to_string())?
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(NoVerifier))
                .with_no_client_auth();
            return Ok(Arc::new(cfg));
        }

        let mut roots = RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let cfg = ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|e| e.to_string())?
            .with_root_certificates(roots)
            .with_no_client_auth();
        Ok(Arc::new(cfg))
    }

    /// Accepts any certificate. Only used behind `--insecure` for testing
    /// self-signed / internal hosts.
    #[derive(Debug)]
    struct NoVerifier;

    impl ServerCertVerifier for NoVerifier {
        fn verify_server_cert(
            &self,
            _end_entity: &CertificateDer<'_>,
            _intermediates: &[CertificateDer<'_>],
            _server_name: &ServerName<'_>,
            _ocsp_response: &[u8],
            _now: UnixTime,
        ) -> Result<ServerCertVerified, Error> {
            Ok(ServerCertVerified::assertion())
        }

        fn verify_tls12_signature(
            &self,
            _message: &[u8],
            _cert: &CertificateDer<'_>,
            _dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, Error> {
            Ok(HandshakeSignatureValid::assertion())
        }

        fn verify_tls13_signature(
            &self,
            _message: &[u8],
            _cert: &CertificateDer<'_>,
            _dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, Error> {
            Ok(HandshakeSignatureValid::assertion())
        }

        fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
            aws_lc_rs::default_provider()
                .signature_verification_algorithms
                .supported_schemes()
        }
    }
}
