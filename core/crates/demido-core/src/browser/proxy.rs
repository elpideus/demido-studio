//! The proxy every request of Demido's browser goes through (DECISIONS Z4).
//!
//! It listens on this computer only, and lets through only the public internet: the browser looks no name
//! up itself (`--host-resolver-rules`), so the proxy does, and keeps only public numbers, for every request,
//! a picture or a script as much as the page (N3). A tunnel whose first bytes are a STUN message, which is
//! how TURN over TCP starts, is refused: WebRTC could otherwise carry what a page holds to any address
//! (research note 31). Plain HTTP is passed on with `Connection: close`; HTTPS goes through as a tunnel, which
//! Demido does not look inside.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::web::guard;

/// The most a request's head may be.
const MOST_OF_A_HEAD: usize = 32 * 1024;

/// What the proxy has let through and refused since it started.
#[derive(Default)]
pub struct Counts {
    pub through: AtomicU64,
    pub refused: AtomicU64,
}

pub struct Proxy {
    pub port: u16,
    pub counts: Arc<Counts>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Proxy {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Whether the first bytes of a tunnel are a STUN message: two bytes of type whose first two bits are 0, a
/// length, and the magic cookie `0x2112A442` (RFC 5389). TLS starts with `0x16 0x03`.
pub fn is_stun(first: &[u8]) -> bool {
    first.len() >= 8 && first[0] & 0xC0 == 0 && first[4..8] == [0x21, 0x12, 0xA4, 0x42]
}

/// The host and port a request names: `CONNECT host:port`, or the address of `GET http://host/…`.
fn target(line: &str) -> Option<(bool, String, u16, String)> {
    let mut parts = line.split_whitespace();
    let (method, what, version) = (parts.next()?, parts.next()?, parts.next()?);
    if method.eq_ignore_ascii_case("CONNECT") {
        let (host, port) = what.rsplit_once(':')?;
        return Some((true, host.trim_matches(['[', ']']).to_string(), port.parse().ok()?, String::new()));
    }
    let url = url::Url::parse(what).ok()?;
    if url.scheme() != "http" {
        return None;
    }
    let path = match url.query() {
        Some(q) => format!("{}?{q}", url.path()),
        None => url.path().to_string(),
    };
    Some((false, url.host_str()?.trim_matches(['[', ']']).to_string(), url.port_or_known_default()?, format!("{method} {path} {version}")))
}

/// The public numbers a name stands for; none when it is on this computer or its network. `allowed` are
/// addresses on this computer that count as public: a test's stand-in only.
async fn public(host: &str, port: u16, allowed: &[String]) -> Vec<SocketAddr> {
    if allowed.iter().any(|a| *a == format!("{host}:{port}")) {
        return tokio::net::lookup_host((host, port)).await.map(|a| a.collect()).unwrap_or_default();
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        return if guard::is_public(ip) { vec![SocketAddr::new(ip, port)] } else { Vec::new() };
    }
    if host.eq_ignore_ascii_case("localhost") || host.to_ascii_lowercase().ends_with(".localhost") {
        return Vec::new();
    }
    match tokio::net::lookup_host((host, port)).await {
        // A name that leads to this computer or its network is refused whole, not only those numbers of it.
        Ok(found) => {
            let found: Vec<SocketAddr> = found.collect();
            if found.iter().all(|a| guard::is_public(a.ip())) { found } else { Vec::new() }
        }
        Err(_) => Vec::new(),
    }
}

async fn refuse(client: &mut TcpStream, why: &str, counts: &Counts) {
    counts.refused.fetch_add(1, Ordering::Relaxed);
    let body = format!("Demido's browser does not go there: {why}.\n");
    let _ = client.write_all(format!("HTTP/1.1 403 Forbidden\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await;
}

async fn serve(mut client: TcpStream, allowed: Arc<Vec<String>>, counts: Arc<Counts>) {
    let mut head = Vec::new();
    let mut piece = [0u8; 4096];
    let end = loop {
        let n = match tokio::time::timeout(Duration::from_secs(20), client.read(&mut piece)).await {
            Ok(Ok(n)) if n > 0 => n,
            _ => return,
        };
        head.extend_from_slice(&piece[..n]);
        if let Some(at) = head.windows(4).position(|w| w == b"\r\n\r\n") {
            break at + 4;
        }
        if head.len() > MOST_OF_A_HEAD {
            return refuse(&mut client, "the request was too long", &counts).await;
        }
    };
    let text = String::from_utf8_lossy(&head[..end]).into_owned();
    let mut lines = text.split("\r\n");
    let Some((tunnel, host, port, first_line)) = lines.next().and_then(target) else {
        return refuse(&mut client, "only web pages are opened", &counts).await;
    };
    let addresses = public(&host, port, &allowed).await;
    let Some(upstream_at) = addresses.first().copied() else {
        return refuse(&mut client, &format!("{host} {}", guard::NOT_PUBLIC), &counts).await;
    };
    let Ok(Ok(mut upstream)) = tokio::time::timeout(Duration::from_secs(10), TcpStream::connect(upstream_at)).await else {
        let _ = client.write_all(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
        return;
    };
    if tunnel {
        if client.write_all(b"HTTP/1.1 200 Connection established\r\n\r\n").await.is_err() {
            return;
        }
        // What comes first decides: a STUN message is WebRTC's TURN, and goes nowhere.
        let mut first = head[end..].to_vec();
        if first.len() < 8 {
            let mut more = [0u8; 4096];
            if let Ok(Ok(n)) = tokio::time::timeout(Duration::from_secs(10), client.read(&mut more)).await {
                first.extend_from_slice(&more[..n]);
            }
        }
        if is_stun(&first) {
            counts.refused.fetch_add(1, Ordering::Relaxed);
            return;
        }
        counts.through.fetch_add(1, Ordering::Relaxed);
        if upstream.write_all(&first).await.is_err() {
            return;
        }
    } else {
        // Plain HTTP: the request is passed on in the form a server expects, and the connection closed after.
        let mut out = format!("{first_line}\r\n");
        for line in lines.filter(|l| !l.is_empty()) {
            let name = line.split(':').next().unwrap_or_default().trim().to_ascii_lowercase();
            if !matches!(name.as_str(), "proxy-connection" | "proxy-authorization" | "connection" | "keep-alive") {
                out.push_str(line);
                out.push_str("\r\n");
            }
        }
        out.push_str("Connection: close\r\n\r\n");
        counts.through.fetch_add(1, Ordering::Relaxed);
        if upstream.write_all(out.as_bytes()).await.is_err() || upstream.write_all(&head[end..]).await.is_err() {
            return;
        }
    }
    let _ = tokio::io::copy_bidirectional(&mut client, &mut upstream).await;
}

impl Proxy {
    /// Starts the proxy on this computer, on a port of its own. `allowed`: addresses on this computer that
    /// count as the public internet, for a test's stand-in; empty otherwise.
    pub async fn start(allowed: Vec<String>) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        let (allowed, counts) = (Arc::new(allowed), Arc::new(Counts::default()));
        let told = counts.clone();
        let task = tokio::spawn(async move {
            while let Ok((client, from)) = listener.accept().await {
                // Only the browser on this computer talks to it.
                if !from.ip().is_loopback() {
                    continue;
                }
                tokio::spawn(serve(client, allowed.clone(), told.clone()));
            }
        });
        Ok(Self { port, counts, task })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_turn_message_is_told_from_tls() {
        // A TURN Allocate request, as Chromium sent it in the trial (research note 31).
        assert!(is_stun(&[0x00, 0x03, 0x00, 0x08, 0x21, 0x12, 0xA4, 0x42, 0x47, 0x2B]));
        // The first bytes of a TLS ClientHello.
        assert!(!is_stun(&[0x16, 0x03, 0x01, 0x02, 0x00, 0x01, 0x00, 0x01, 0xFC]));
        assert!(!is_stun(b"GET / HT"));
        assert!(!is_stun(&[0x00, 0x01]));
    }

    #[test]
    fn what_a_request_names_is_read() {
        assert_eq!(target("CONNECT example.org:443 HTTP/1.1"), Some((true, "example.org".into(), 443, String::new())));
        assert_eq!(target("CONNECT [2606:4700::1]:443 HTTP/1.1"), Some((true, "2606:4700::1".into(), 443, String::new())));
        assert_eq!(target("GET http://example.org/a?b=1 HTTP/1.1"), Some((false, "example.org".into(), 80, "GET /a?b=1 HTTP/1.1".into())));
        assert_eq!(target("GET ftp://example.org/ HTTP/1.1"), None);
        assert_eq!(target("nonsense"), None);
    }

    async fn ask(port: u16, request: &str) -> String {
        let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        s.write_all(request.as_bytes()).await.unwrap();
        let mut got = Vec::new();
        let _ = tokio::time::timeout(Duration::from_secs(5), s.read_to_end(&mut got)).await;
        String::from_utf8_lossy(&got).into_owned()
    }

    #[tokio::test]
    async fn only_the_public_internet_goes_through_and_a_turn_tunnel_goes_nowhere() {
        // A stand-in site on this computer, which counts as public for this test only.
        let site = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let at = site.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((mut c, _)) = site.accept().await {
                tokio::spawn(async move {
                    let mut b = [0u8; 4096];
                    let n = c.read(&mut b).await.unwrap_or(0);
                    let said = String::from_utf8_lossy(&b[..n]).into_owned();
                    let first = said.lines().next().unwrap_or_default().to_string();
                    let _ = c.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{first}", first.len()).as_bytes()).await;
                });
            }
        });
        let proxy = Proxy::start(vec![at.to_string()]).await.unwrap();
        // Plain HTTP is passed on, in the form a server expects.
        let got = ask(proxy.port, &format!("GET http://{at}/page?x=1 HTTP/1.1\r\nHost: {at}\r\nProxy-Connection: keep-alive\r\n\r\n")).await;
        assert!(got.starts_with("HTTP/1.1 200") && got.ends_with("GET /page?x=1 HTTP/1.1"), "{got}");
        // This computer and its network are not reached, by number or by name.
        for host in ["127.0.0.1:7341", "192.168.1.1:80", "169.254.169.254:80", "localhost:7341", "[::1]:7341"] {
            let got = ask(proxy.port, &format!("CONNECT {host} HTTP/1.1\r\nHost: {host}\r\n\r\n")).await;
            assert!(got.starts_with("HTTP/1.1 403") && got.contains(guard::NOT_PUBLIC), "{host}: {got}");
        }
        // A tunnel that starts as TURN does: closed with nothing passed on.
        let mut s = TcpStream::connect(("127.0.0.1", proxy.port)).await.unwrap();
        s.write_all(format!("CONNECT {at} HTTP/1.1\r\n\r\n").as_bytes()).await.unwrap();
        let mut b = [0u8; 64];
        let n = s.read(&mut b).await.unwrap();
        assert!(String::from_utf8_lossy(&b[..n]).starts_with("HTTP/1.1 200"));
        s.write_all(&[0x00, 0x03, 0x00, 0x08, 0x21, 0x12, 0xA4, 0x42, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]).await.unwrap();
        let mut rest = Vec::new();
        let _ = tokio::time::timeout(Duration::from_secs(5), s.read_to_end(&mut rest)).await;
        assert!(rest.is_empty(), "nothing came back: {rest:?}");
        // A tunnel that starts as anything else goes through.
        let mut s = TcpStream::connect(("127.0.0.1", proxy.port)).await.unwrap();
        s.write_all(format!("CONNECT {at} HTTP/1.1\r\n\r\nGET /tunnelled HTTP/1.1\r\n\r\n").as_bytes()).await.unwrap();
        let mut got = Vec::new();
        let _ = tokio::time::timeout(Duration::from_secs(5), s.read_to_end(&mut got)).await;
        assert!(String::from_utf8_lossy(&got).ends_with("GET /tunnelled HTTP/1.1"), "{}", String::from_utf8_lossy(&got));
        assert_eq!((proxy.counts.through.load(Ordering::Relaxed), proxy.counts.refused.load(Ordering::Relaxed)), (2, 6));
    }
}
