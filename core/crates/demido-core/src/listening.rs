//! What the server may listen on, and how (DECISIONS F14 to F18; research notes 05, section 3.5, and 35).
//!
//! - **This computer's own address, in plain HTTP**: the command line, the desktop app, a browser on this
//!   computer. `server.address` must be a loopback address; anything else is refused, with no flag to say
//!   otherwise ("Plain HTTP on a non-loopback address: Refuse. No flag.", note 05).
//! - **Any other address only over HTTPS**, on a second listener (`server.remote`), with a certificate and its
//!   key (`server.tlsCert`, `server.tlsKey`, PEM files). Without both, the server does not start.
//! - **The names it answers to** are this computer's own and those given (`server.names`: a domain, a Tailscale
//!   name, a reverse proxy's), on both listeners; a request for any other `Host` is refused (DNS rebinding). An
//!   address that is every address (`0.0.0.0`) needs names, since no `Host` would then be its own.
//! - **Not as root** on Linux and macOS: what a model runs would run as root too.
//! - `X-Forwarded-*` headers are never read: nothing a proxy says changes who a request is from.

use std::net::{SocketAddr, ToSocketAddrs};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::TlsAcceptor;
use tokio_rustls::rustls;
use tokio_rustls::server::TlsStream;

use crate::{Error, Result};

/// How long a connection has to finish its TLS handshake.
const HANDSHAKE_WITHIN: Duration = Duration::from_secs(10);
/// Handshakes under way at once; more wait.
const HANDSHAKES_AT_ONCE: usize = 64;

/// What the server listens on, once every rule above holds.
pub struct Plan {
    pub local: SocketAddr,
    pub remote: Option<(SocketAddr, Arc<rustls::ServerConfig>)>,
    /// The names given, without their port.
    pub names: Vec<String>,
}

fn one_address(what: &str, given: &str) -> Result<Vec<SocketAddr>> {
    let found: Vec<SocketAddr> = given.to_socket_addrs().map_err(|e| Error::Invalid(format!("{what} \"{given}\" is not an address and a port: {e}")))?.collect();
    if found.is_empty() {
        return Err(Error::Invalid(format!("{what} \"{given}\" names no address.")));
    }
    Ok(found)
}

/// Checks what was asked for against the rules, and loads the certificate.
pub fn plan(listen: &str, remote: &str, cert: &str, key: &str, names: &str) -> Result<Plan> {
    let local = one_address("server.address", listen)?;
    if let Some(beyond) = local.iter().find(|a| !a.ip().is_loopback()) {
        return Err(Error::Invalid(format!(
            "Demido answers in plain HTTP only on this computer's own address, and {} is not one. To be reached from other devices, give server.remote an address with a certificate (server.tlsCert, server.tlsKey), or reach this computer's own address through an SSH tunnel or Tailscale Serve.",
            beyond.ip()
        )));
    }
    let names: Vec<String> = names.split([',', ' ']).map(|n| n.trim().trim_end_matches('.').to_ascii_lowercase()).filter(|n| !n.is_empty()).collect();
    if let Some(bad) = names.iter().find(|n| !n.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '[' | ']'))) {
        return Err(Error::Invalid(format!("\"{bad}\" in server.names is not a name.")));
    }
    let remote = match remote.trim() {
        "" => None,
        given => {
            let at = one_address("server.remote", given)?[0];
            if at.ip().is_unspecified() && names.is_empty() {
                return Err(Error::Invalid(format!(
                    "server.remote is every address ({at}), so say which names Demido answers to there (server.names: a domain, or this computer's address on the network)."
                )));
            }
            if cert.trim().is_empty() || key.trim().is_empty() {
                return Err(Error::Invalid(
                    "server.remote listens beyond this computer, which is only ever over HTTPS: give its certificate and key (server.tlsCert, server.tlsKey, PEM files). Demido never answers in plain HTTP there.".into(),
                ));
            }
            Some((at, Arc::new(tls_config(Path::new(cert.trim()), Path::new(key.trim()))?)))
        }
    };
    Ok(Plan { local: local[0], remote, names })
}

/// The `Host` values the server answers to: this computer's three spellings of its own address, the remote
/// address when it is one, and each name given, with the port of each listener (and bare on 443).
pub fn hosts(local: SocketAddr, remote: Option<SocketAddr>, names: &[String]) -> Vec<String> {
    let port = local.port();
    let mut all = vec![format!("127.0.0.1:{port}"), format!("localhost:{port}"), format!("[::1]:{port}")];
    let mut ports = vec![port];
    if let Some(r) = remote {
        ports.push(r.port());
        if !r.ip().is_unspecified() {
            all.push(r.to_string());
            if r.port() == 443 {
                all.push(match r.ip() {
                    std::net::IpAddr::V4(ip) => ip.to_string(),
                    std::net::IpAddr::V6(ip) => format!("[{ip}]"),
                });
            }
        }
    }
    for name in names {
        for p in &ports {
            all.push(format!("{name}:{p}"));
        }
        // A proxy in front (HTTPS on 443) sends the name alone.
        all.push(name.clone());
    }
    all.dedup();
    all
}

/// Where a person elsewhere reaches the remote listener: its first name, or its address.
pub fn remote_url(remote: SocketAddr, names: &[String]) -> String {
    let host = names.first().cloned().unwrap_or_else(|| remote.ip().to_string());
    if remote.port() == 443 { format!("https://{host}") } else { format!("https://{host}:{}", remote.port()) }
}

fn tls_config(cert: &Path, key: &Path) -> Result<rustls::ServerConfig> {
    use rustls::pki_types::pem::PemObject;
    use rustls::pki_types::{CertificateDer, PrivateKeyDer};
    let chain: Vec<CertificateDer<'static>> = CertificateDer::pem_file_iter(cert)
        .map_err(|e| Error::Invalid(format!("The certificate {} cannot be read: {e}", cert.display())))?
        .collect::<std::result::Result<_, _>>()
        .map_err(|e| Error::Invalid(format!("The certificate {} is not PEM: {e}", cert.display())))?;
    if chain.is_empty() {
        return Err(Error::Invalid(format!("{} holds no certificate.", cert.display())));
    }
    let key = PrivateKeyDer::from_pem_file(key).map_err(|e| Error::Invalid(format!("The key {} cannot be read as a PEM private key: {e}", key.display())))?;
    let mut config = rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::aws_lc_rs::default_provider()))
        .with_safe_default_protocol_versions()
        .map_err(|e| Error::Other(e.to_string()))?
        .with_no_client_auth()
        .with_single_cert(chain, key)
        .map_err(|e| Error::Invalid(format!("The certificate and the key do not go together: {e}")))?;
    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    Ok(config)
}

/// Refuses to run as root, where there is such a thing.
pub fn not_as_root() -> Result<()> {
    #[cfg(unix)]
    // SAFETY: geteuid has no preconditions and cannot fail.
    if unsafe { libc::geteuid() } == 0 {
        return Err(Error::Invalid("Demido does not run as root: the programs a model runs would run as root too. Start it as an ordinary user.".into()));
    }
    Ok(())
}

/// The remote listener: TCP, each connection's TLS handshake done apart (so that a slow one holds up no other),
/// within a few seconds, and only finished connections handed to the server.
pub struct TlsListener {
    ready: tokio::sync::mpsc::Receiver<(TlsStream<TcpStream>, SocketAddr)>,
    local: SocketAddr,
}

impl TlsListener {
    pub async fn bind(at: SocketAddr, config: Arc<rustls::ServerConfig>) -> Result<Self> {
        let tcp = TcpListener::bind(at).await.map_err(|e| Error::Other(format!("Could not listen on {at}: {e}")))?;
        let local = tcp.local_addr()?;
        let acceptor = TlsAcceptor::from(config);
        let (give, ready) = tokio::sync::mpsc::channel(HANDSHAKES_AT_ONCE);
        let room = Arc::new(tokio::sync::Semaphore::new(HANDSHAKES_AT_ONCE));
        tokio::spawn(async move {
            loop {
                let Ok(permit) = room.clone().acquire_owned().await else { return };
                let Ok((stream, peer)) = tcp.accept().await else {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    continue;
                };
                let (acceptor, handed) = (acceptor.clone(), give.clone());
                tokio::spawn(async move {
                    let _permit = permit;
                    if let Ok(Ok(tls)) = tokio::time::timeout(HANDSHAKE_WITHIN, acceptor.accept(stream)).await {
                        let _ = handed.send((tls, peer)).await;
                    }
                });
                if give.is_closed() {
                    return;
                }
            }
        });
        Ok(Self { ready, local })
    }
}

impl axum::serve::Listener for TlsListener {
    type Io = TlsStream<TcpStream>;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        match self.ready.recv().await {
            Some(ready) => ready,
            // The accepting task ended: nothing more will come.
            None => std::future::pending().await,
        }
    }

    fn local_addr(&self) -> std::io::Result<Self::Addr> {
        Ok(self.local)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_http_is_for_this_computer_alone() {
        assert!(plan("127.0.0.1:7341", "", "", "", "").is_ok());
        assert!(plan("[::1]:7341", "", "", "", "").is_ok());
        let refused = plan("0.0.0.0:7341", "", "", "", "").err().unwrap().to_string();
        assert!(refused.contains("plain HTTP only on this computer's own address"), "{refused}");
        assert!(plan("192.168.1.20:7341", "", "", "", "").is_err());
        // Beyond this computer: never without a certificate, and every address only with names.
        assert!(plan("127.0.0.1:7341", "192.168.1.20:7443", "", "", "").err().unwrap().to_string().contains("only ever over HTTPS"));
        assert!(plan("127.0.0.1:7341", "0.0.0.0:7443", "/c.pem", "/k.pem", "").err().unwrap().to_string().contains("server.names"));
        assert!(plan("127.0.0.1:7341", "192.168.1.20:7443", "/no/such/cert.pem", "/no/such/key.pem", "").err().unwrap().to_string().contains("cannot be read"));
        assert!(plan("127.0.0.1:7341", "", "", "", "demido.example.com, bad/name").is_err());
    }

    #[test]
    fn the_names_it_answers_to() {
        let local: SocketAddr = "127.0.0.1:7341".parse().unwrap();
        let remote: SocketAddr = "192.168.1.20:7443".parse().unwrap();
        let h = hosts(local, Some(remote), &["box.tail1234.ts.net".into()]);
        for wanted in ["127.0.0.1:7341", "localhost:7341", "[::1]:7341", "192.168.1.20:7443", "box.tail1234.ts.net:7443", "box.tail1234.ts.net:7341", "box.tail1234.ts.net"] {
            assert!(h.contains(&wanted.to_string()), "{wanted} in {h:?}");
        }
        assert!(!h.iter().any(|x| x.starts_with("evil")));
        assert_eq!(remote_url(remote, &[]), "https://192.168.1.20:7443");
        assert_eq!(remote_url("0.0.0.0:443".parse().unwrap(), &["demido.example.com".into()]), "https://demido.example.com");
    }

    /// A certificate made for the test, served on this computer: HTTPS answers, plain HTTP gets nothing.
    #[tokio::test]
    async fn the_remote_listener_speaks_only_https() {
        let made = rcgen::generate_simple_self_signed(vec!["demido.test".to_string()]).unwrap();
        let dir = crate::testing::TempDir::new("demido-tls");
        let (cert, key) = (dir.join("cert.pem"), dir.join("key.pem"));
        std::fs::write(&cert, made.cert.pem()).unwrap();
        std::fs::write(&key, made.signing_key.serialize_pem()).unwrap();
        let p = plan("127.0.0.1:0", "127.0.0.1:0", cert.to_str().unwrap(), key.to_str().unwrap(), "demido.test").unwrap();
        let (at, config) = p.remote.unwrap();
        let listener = TlsListener::bind(at, config).await.unwrap();
        let port = listener.local.port();
        let app = axum::Router::new().route("/api/v1/health", axum::routing::get(|| async { "ok" }));
        tokio::spawn(async move { axum::serve(listener, app).await });

        let trusted = reqwest::Certificate::from_pem(made.cert.pem().as_bytes()).unwrap();
        let client = reqwest::Client::builder().add_root_certificate(trusted).resolve("demido.test", SocketAddr::from(([127, 0, 0, 1], port))).build().unwrap();
        let said = client.get(format!("https://demido.test:{port}/api/v1/health")).send().await.unwrap().text().await.unwrap();
        assert_eq!(said, "ok");
        // The same port in plain HTTP: no answer, only a connection that ends.
        let plain = reqwest::Client::new().get(format!("http://127.0.0.1:{port}/api/v1/health")).timeout(Duration::from_secs(15)).send().await;
        assert!(plain.is_err(), "plain HTTP was answered: {plain:?}");
    }
}
