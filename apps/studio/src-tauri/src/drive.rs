//! Lets `scripts/drive.mjs` drive the app in development on Linux, where WebKitGTK has no DevTools
//! protocol (on Windows the scripts use WebView2's). Debug builds only, and off unless
//! `DEMIDO_DRIVE_PORT` names a port: the app then listens on 127.0.0.1 there and answers
//!
//! ```text
//! POST /targets                         [{"label": "main", "url": "tauri://localhost/"}, …]
//! POST /eval {target, script}           {"value": …} or {"error": "…"}; `script` is an
//!                                       expression, awaited when it is a promise
//! POST /screenshot {target}             {"png": "<base64>"}: what the webview shows
//! POST /resize {target, width, height}  {}: the window's size, in logical pixels
//! ```
//!
//! `target` is a window's label, "main" when left out. Every request must carry the header
//! `x-demido-drive: 1` and no `Origin`: a web page in a browser can send neither without the
//! browser asking first, so only programs on this computer can drive the app.

use std::time::Duration;

use base64::Engine;
use serde_json::{Value, json};
use tauri::{AppHandle, Manager};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::oneshot;

/// The environment variable that turns the bridge on, with the port to listen on.
pub const PORT_ENV: &str = "DEMIDO_DRIVE_PORT";

/// How long an evaluation or a snapshot may take.
const TIMEOUT: Duration = Duration::from_secs(60);

/// Starts listening when [`PORT_ENV`] names a port.
pub fn start(app: AppHandle) {
    let Some(port) = std::env::var(PORT_ENV).ok().and_then(|p| p.trim().parse::<u16>().ok()) else {
        return;
    };
    tauri::async_runtime::spawn(async move {
        let listener = match tokio::net::TcpListener::bind(("127.0.0.1", port)).await {
            Ok(listener) => listener,
            Err(e) => {
                tracing::warn!("drive: cannot listen on 127.0.0.1:{port}: {e}");
                return;
            }
        };
        tracing::info!("drive: listening on 127.0.0.1:{port}");
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                continue;
            };
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = serve(stream, &app).await {
                    tracing::debug!("drive: {e}");
                }
            });
        }
    });
}

/// One request, one answer, then the connection closes.
async fn serve(stream: TcpStream, app: &AppHandle) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).await?;
    let path = line.split_whitespace().nth(1).unwrap_or("").to_owned();
    let (mut length, mut allowed, mut origin) = (0usize, false, false);
    loop {
        line.clear();
        if reader.read_line(&mut line).await? == 0 || line.trim().is_empty() {
            break;
        }
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        match name.trim().to_ascii_lowercase().as_str() {
            "content-length" => length = value.trim().parse().unwrap_or(0),
            "x-demido-drive" => allowed = value.trim() == "1",
            "origin" => origin = true,
            _ => {}
        }
    }
    let mut body = vec![0; length.min(16 << 20)];
    reader.read_exact(&mut body).await?;
    let (status, answer) = if !allowed || origin {
        ("403 Forbidden", json!({"error": "not a drive request"}))
    } else {
        let request: Value = serde_json::from_slice(&body).unwrap_or_default();
        match answer(app, &path, &request).await {
            Ok(value) => ("200 OK", value),
            Err(error) => ("200 OK", json!({ "error": error })),
        }
    };
    let text = answer.to_string();
    let mut stream = reader.into_inner();
    stream
        .write_all(
            format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
                text.len()
            )
            .as_bytes(),
        )
        .await?;
    stream.shutdown().await
}

async fn answer(app: &AppHandle, path: &str, request: &Value) -> Result<Value, String> {
    if path == "/targets" {
        let targets: Vec<Value> = app
            .webview_windows()
            .into_iter()
            .map(|(label, window)| {
                let url = window.url().map(|u| u.to_string()).unwrap_or_default();
                json!({ "label": label, "url": url })
            })
            .collect();
        return Ok(Value::Array(targets));
    }
    let label = request["target"].as_str().unwrap_or("main");
    let window = app
        .get_webview_window(label)
        .ok_or_else(|| format!("no window is labelled {label}"))?;
    match path {
        "/eval" => {
            let script = request["script"].as_str().ok_or("no script")?;
            evaluate(&window, script).await.map(|value| json!({ "value": value }))
        }
        "/screenshot" => {
            let png = snapshot(&window).await?;
            Ok(json!({ "png": base64::engine::general_purpose::STANDARD.encode(png) }))
        }
        "/resize" => {
            let (Some(width), Some(height)) = (request["width"].as_f64(), request["height"].as_f64()) else {
                return Err("width and height are needed".into());
            };
            window
                .set_size(tauri::LogicalSize::new(width, height))
                .map_err(|e| e.to_string())?;
            Ok(json!({}))
        }
        _ => Err(format!("unknown request {path}")),
    }
}

/// Runs `script` in the page and returns its value, awaited, as JSON (`null` for `undefined`).
async fn evaluate(window: &tauri::WebviewWindow, script: &str) -> Result<Value, String> {
    use javascriptcore::ValueExt;
    use webkit2gtk::WebViewExt;

    let body = format!("return await (\n{script}\n);");
    let (tx, rx) = oneshot::channel::<Result<String, String>>();
    window
        .with_webview(move |webview| {
            webview.inner().call_async_javascript_function(
                &body,
                None,
                None,
                None,
                None::<&webkit2gtk::gio::Cancellable>,
                move |result| {
                    let _ = tx.send(
                        result
                            .map(|value| value.to_json(0).map(|j| j.to_string()).unwrap_or_else(|| "null".into()))
                            .map_err(|e| e.message().to_owned()),
                    );
                },
            );
        })
        .map_err(|e| e.to_string())?;
    let json = tokio::time::timeout(TIMEOUT, rx)
        .await
        .map_err(|_| "the page did not answer in time".to_owned())?
        .map_err(|_| "the page went away".to_owned())??;
    Ok(serde_json::from_str(&json).unwrap_or(Value::Null))
}

/// A PNG of what the webview shows.
async fn snapshot(window: &tauri::WebviewWindow) -> Result<Vec<u8>, String> {
    use webkit2gtk::{SnapshotOptions, SnapshotRegion, WebViewExt};

    let (tx, rx) = oneshot::channel::<Result<Vec<u8>, String>>();
    window
        .with_webview(move |webview| {
            webview.inner().snapshot(
                SnapshotRegion::Visible,
                SnapshotOptions::NONE,
                None::<&webkit2gtk::gio::Cancellable>,
                move |result| {
                    let png = result.map_err(|e| e.message().to_owned()).and_then(|surface| {
                        let image = cairo::ImageSurface::try_from(surface).map_err(|_| "not an image".to_owned())?;
                        let mut png = Vec::new();
                        image.write_to_png(&mut png).map_err(|e| e.to_string())?;
                        Ok(png)
                    });
                    let _ = tx.send(png);
                },
            );
        })
        .map_err(|e| e.to_string())?;
    tokio::time::timeout(TIMEOUT, rx)
        .await
        .map_err(|_| "the snapshot did not come in time".to_owned())?
        .map_err(|_| "the page went away".to_owned())?
}
