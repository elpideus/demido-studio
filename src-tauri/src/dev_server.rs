//! The second trap of the window gate, refused at startup.
//!
//! `docs/rules/done.md`: "a debug build loads `devUrl`, so running the binary
//! without the frontend dev server serves a blank page with the right window
//! title. Blank window, no error."
//!
//! So a debug build looks for the dev server before it opens anything. A TCP
//! connect is the whole check: it is the same failure the webview would hit,
//! one layer earlier, where there is still somewhere to print.

use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use demido_core::{Error, Result};

/// Long enough for a loopback connect on a busy machine, short enough that a
/// developer reads it as a startup check rather than as a hang.
const TIMEOUT: Duration = Duration::from_millis(750);

/// Fail with a sentence if the configured `devUrl` has nothing listening.
///
/// A configuration with no `devUrl` is a build serving `frontendDist`, which
/// has no dev server to want.
pub fn require(config: &tauri::Config) -> Result<()> {
    let Some(url) = config.build.dev_url.as_ref() else {
        return Ok(());
    };

    let host = url
        .host_str()
        .ok_or_else(|| Error::invalid("devUrl", format!("{url} has no host")))?;
    let port = url
        .port_or_known_default()
        .ok_or_else(|| Error::invalid("devUrl", format!("{url} has no port")))?;

    let mut addresses = (host, port)
        .to_socket_addrs()
        .map_err(|source| Error::io(format!("resolving {host}"), source))?;

    let reachable = addresses.any(|address| TcpStream::connect_timeout(&address, TIMEOUT).is_ok());
    if reachable {
        return Ok(());
    }

    Err(Error::unavailable(
        "the frontend dev server",
        format!(
            "nothing is listening on {url}. A debug build loads that URL, so it would have opened \
             a blank window under the right title. Run `pnpm dev`, which starts both, or `pnpm \
             dev:web` in another terminal first."
        ),
    ))
}
