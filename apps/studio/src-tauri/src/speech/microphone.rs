//! The microphone: allowed to the app's own pages, and whether Windows lets desktop apps use it.

/// The origins of the app's own pages: the bundled UI, and Vite's server while developing.
#[cfg_attr(not(any(windows, target_os = "linux")), allow(dead_code))]
fn is_app_origin(uri: &str) -> bool {
    let Ok(url) = url::Url::parse(uri) else {
        return false;
    };
    // tauri://localhost, the page on Linux and macOS, is a custom scheme: to the url crate its
    // origin is opaque ("null"), so it is matched by scheme and host.
    if url.scheme() == "tauri" {
        return url.host_str() == Some("localhost") && url.port().is_none();
    }
    let origin = url.origin().ascii_serialization();
    matches!(origin.as_str(), "http://tauri.localhost" | "https://tauri.localhost")
        || (cfg!(debug_assertions) && origin == "http://localhost:1420")
}

/// Answers WebView2's microphone requests: allowed for the app's own pages, so recording needs no
/// browser prompt, and refused for any other page (a web page a tool opened, say). wry answers
/// only clipboard requests; every other request goes on to WebView2 as before.
#[cfg(windows)]
pub fn allow_for_app(window: &tauri::WebviewWindow) {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_PERMISSION_KIND, COREWEBVIEW2_PERMISSION_KIND_MICROPHONE, COREWEBVIEW2_PERMISSION_STATE_ALLOW,
        COREWEBVIEW2_PERMISSION_STATE_DENY,
    };
    use webview2_com::{PermissionRequestedEventHandler, take_pwstr};
    use windows::core::PWSTR;

    let hooked = window.with_webview(|webview| {
        // SAFETY: WebView2 calls on the window's thread, where `with_webview` runs this; the
        // arguments are valid for the length of the call.
        let result = unsafe {
            webview.controller().CoreWebView2().and_then(|core| {
                let mut token = Default::default();
                core.add_PermissionRequested(
                    &PermissionRequestedEventHandler::create(Box::new(|_, args| {
                        let Some(args) = args else {
                            return Ok(());
                        };
                        let mut kind = COREWEBVIEW2_PERMISSION_KIND::default();
                        args.PermissionKind(&mut kind)?;
                        if kind != COREWEBVIEW2_PERMISSION_KIND_MICROPHONE {
                            return Ok(());
                        }
                        let mut uri = PWSTR::null();
                        args.Uri(&mut uri)?;
                        let uri = take_pwstr(uri);
                        let ours = is_app_origin(&uri);
                        if !ours {
                            tracing::info!(%uri, "a page that is not the app's asked for the microphone; refused");
                        }
                        args.SetState(if ours {
                            COREWEBVIEW2_PERMISSION_STATE_ALLOW
                        } else {
                            COREWEBVIEW2_PERMISSION_STATE_DENY
                        })
                    })),
                    &mut token,
                )
            })
        };
        if let Err(e) = result {
            tracing::warn!("could not answer microphone requests: {e}");
        }
    });
    if let Err(e) = hooked {
        tracing::warn!("could not reach the webview for microphone requests: {e}");
    }
}

/// Answers WebKitGTK's microphone requests the same way. WebKitGTK keeps `getUserMedia` off
/// unless the view's settings turn it on, which wry does not, and refuses any request no handler
/// answers. Naming the microphones (`enumerateDevices`, for Settings) is a request of its own.
#[cfg(target_os = "linux")]
pub fn allow_for_app(window: &tauri::WebviewWindow) {
    use webkit2gtk::glib::prelude::{Cast, ObjectExt};
    use webkit2gtk::{
        DeviceInfoPermissionRequest, PermissionRequestExt, SettingsExt, UserMediaPermissionRequest,
        UserMediaPermissionRequestExt, WebViewExt,
    };

    let hooked = window.with_webview(|webview| {
        let view = webview.inner();
        if let Some(settings) = WebViewExt::settings(&view) {
            settings.set_enable_media_stream(true);
        }
        view.connect_permission_request(|view, request| {
            let microphone = request
                .downcast_ref::<UserMediaPermissionRequest>()
                .is_some_and(|r| r.is_for_audio_device() && !r.is_for_video_device());
            if !microphone && !request.is::<DeviceInfoPermissionRequest>() {
                // Not a microphone request: WebKitGTK refuses it, as it did before.
                return false;
            }
            let uri = view.uri().unwrap_or_default();
            if is_app_origin(&uri) {
                request.allow();
            } else {
                tracing::info!(%uri, "a page that is not the app's asked for the microphone; refused");
                request.deny();
            }
            true
        });
    });
    if let Err(e) = hooked {
        tracing::warn!("could not reach the webview for microphone requests: {e}");
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn allow_for_app(_window: &tauri::WebviewWindow) {}

/// Whether Windows' privacy settings keep desktop apps from the microphone: for this computer,
/// for every app, or for desktop apps (Settings, Privacy & security, Microphone). A recording
/// then gets only silence, so the composer says where to turn it on.
#[cfg(windows)]
pub fn blocked_by_windows() -> bool {
    const KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone";
    let denied = |root: &windows_registry::Key, path: &str| {
        root.open(path)
            .and_then(|k| k.get_string("Value"))
            .is_ok_and(|v| v.eq_ignore_ascii_case("Deny"))
    };
    denied(windows_registry::LOCAL_MACHINE, KEY)
        || denied(windows_registry::CURRENT_USER, KEY)
        || denied(windows_registry::CURRENT_USER, &format!(r"{KEY}\NonPackaged"))
}

#[cfg(not(windows))]
pub fn blocked_by_windows() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_apps_own_pages_get_the_microphone() {
        assert!(is_app_origin("http://tauri.localhost/"));
        assert!(is_app_origin("https://tauri.localhost/index.html"));
        assert!(!is_app_origin("https://tauri.localhost.example.com/"));
        assert!(is_app_origin("tauri://localhost"));
        assert!(is_app_origin("tauri://localhost/index.html"));
        assert!(!is_app_origin("tauri://localhost.example.com/"));
        assert!(!is_app_origin("tauri://example.com/"));
        assert!(!is_app_origin("https://www.tradingview.com/chart/"));
        assert!(!is_app_origin("not a url"));
        assert_eq!(is_app_origin("http://localhost:1420/"), cfg!(debug_assertions));
        assert!(!is_app_origin("http://localhost:8080/"));
    }
}
