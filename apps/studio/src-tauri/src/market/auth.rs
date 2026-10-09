//! Signing in to TradingView.
//!
//! A separate window shows TradingView's own sign-in page, in a browser profile of its own so
//! its cookies never mix with the app's. Once TradingView sets the `sessionid` and
//! `sessionid_sign` cookies, they are read from that profile, checked by the market service,
//! stored in the credential store and the window closes. The person never types a password into
//! Demido itself.

use std::sync::Arc;
use std::time::{Duration, Instant};

use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};
use tokio::sync::watch;

use super::{MarketService, Session};

pub const WINDOW_LABEL: &str = "tradingview-login";
const SIGN_IN_URL: &str = "https://www.tradingview.com/accounts/signin/";
const COOKIE_URL: &str = "https://www.tradingview.com/";
/// How long the window waits for the person before giving up on its own.
const FLOW_TIMEOUT: Duration = Duration::from_secs(20 * 60);

#[derive(Clone, Debug, PartialEq)]
pub enum LoginState {
    Waiting,
    Success(String),
    Failed(String),
    Closed,
}

/// Opens the sign-in window (or brings the open one forward) and returns a receiver that
/// settles when the flow ends.
pub async fn begin(service: &Arc<MarketService>) -> Result<watch::Receiver<LoginState>, String> {
    let mut slot = service.login.lock().await;
    if let Some(rx) = slot.as_ref()
        && *rx.borrow() == LoginState::Waiting
    {
        if let Some(w) = service.app().get_webview_window(WINDOW_LABEL) {
            let _ = w.unminimize();
            let _ = w.set_focus();
        }
        return Ok(rx.clone());
    }

    let app = service.app().clone();
    if let Some(existing) = app.get_webview_window(WINDOW_LABEL) {
        let _ = existing.close();
    }
    let url = SIGN_IN_URL.parse().map_err(|e| format!("{e}"))?;
    let window = WebviewWindowBuilder::new(&app, WINDOW_LABEL, WebviewUrl::External(url))
        .title("Sign in to TradingView · Demido Studio")
        .inner_size(500.0, 720.0)
        .min_inner_size(400.0, 560.0)
        .center()
        .theme(Some(tauri::Theme::Dark))
        .data_directory(service.profile_dir.clone())
        .build()
        .map_err(|e| format!("could not open the sign-in window: {e}"))?;
    // TradingView's pages are not the app's: they are refused the microphone, without a prompt.
    crate::speech::microphone::allow_for_app(&window);
    let _ = window.set_focus();

    let (tx, rx) = watch::channel(LoginState::Waiting);
    *slot = Some(rx.clone());
    drop(slot);
    service.update_status(|s| s.login_pending = true);

    let service = service.clone();
    tokio::spawn(async move {
        let outcome = poll(&service).await;
        if let Some(w) = service.app().get_webview_window(WINDOW_LABEL) {
            let _ = w.close();
        }
        service.update_status(|s| s.login_pending = false);
        let _ = tx.send(outcome);
    });
    Ok(rx)
}

async fn poll(service: &Arc<MarketService>) -> LoginState {
    let started = Instant::now();
    let url: url::Url = COOKIE_URL.parse().expect("valid url");
    let mut rejected: Option<String> = None;
    let mut failures = 0u64;
    loop {
        tokio::time::sleep(Duration::from_millis(900)).await;
        if started.elapsed() > FLOW_TIMEOUT {
            tracing::info!("the TradingView sign-in window timed out");
            return LoginState::Failed("The sign-in window timed out.".into());
        }
        let Some(window) = service.app().get_webview_window(WINDOW_LABEL) else {
            tracing::info!("the TradingView sign-in window was closed");
            return LoginState::Closed;
        };
        // Reading cookies blocks until the webview answers; keep it off the async workers.
        let url = url.clone();
        let cookies = tokio::task::spawn_blocking(move || window.cookies_for_url(url))
            .await
            .ok()
            .and_then(Result::ok)
            .unwrap_or_default();
        let find = |name: &str| {
            cookies
                .iter()
                .find(|c| c.name() == name && !c.value().is_empty())
                .map(|c| c.value().to_string())
        };
        let (Some(session), Some(signature)) = (find("sessionid"), find("sessionid_sign")) else {
            continue;
        };
        if rejected.as_deref() == Some(session.as_str()) {
            continue; // Stale cookie from an expired login: wait for a fresh one.
        }
        tracing::info!("TradingView session cookies found; checking them");
        match service
            .apply_session(&Session {
                session: session.clone(),
                signature,
            })
            .await
        {
            Ok(username) => {
                tracing::info!("signed in to TradingView as {username}");
                return LoginState::Success(username);
            }
            Err(e) if e.is_auth() => {
                tracing::warn!("TradingView rejected the session ({}): {}", e.code, e.message);
                service.update_status(|s| s.last_error = Some(e.message.clone()));
                rejected = Some(session);
            }
            Err(e) => {
                // A network hiccup must not lose a sign-in the person already completed: the
                // cookies stay in the window's profile, so keep the window and try again.
                failures += 1;
                tracing::warn!(
                    "checking the TradingView session failed ({}), attempt {failures}: {}",
                    e.code,
                    e.message
                );
                service.update_status(|s| s.last_error = Some(e.message.clone()));
                if failures >= 6 {
                    return LoginState::Failed(e.message);
                }
                tokio::time::sleep(Duration::from_secs(2 * failures)).await;
            }
        }
    }
}

/// Waits for a sign-in flow up to `timeout`. `Ok(username)` on success.
pub async fn wait(mut rx: watch::Receiver<LoginState>, timeout: Duration) -> Result<String, LoginState> {
    let result = tokio::time::timeout(timeout, async {
        loop {
            let state = rx.borrow().clone();
            if state != LoginState::Waiting {
                return state;
            }
            if rx.changed().await.is_err() {
                return LoginState::Closed;
            }
        }
    })
    .await;
    match result {
        Ok(LoginState::Success(user)) => Ok(user),
        Ok(other) => Err(other),
        Err(_) => Err(LoginState::Waiting),
    }
}

/// Deletes the sign-in window's browser profile (its cookies included).
pub async fn clear_profile(service: &MarketService) {
    if let Some(w) = service.app().get_webview_window(WINDOW_LABEL) {
        let _ = w.close();
    }
    let dir = service.profile_dir.clone();
    // The browser process may hold the folder for a moment after the window closes.
    for _ in 0..10 {
        if !dir.exists() || std::fs::remove_dir_all(&dir).is_ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    tracing::warn!("could not clear {}", dir.display());
}
