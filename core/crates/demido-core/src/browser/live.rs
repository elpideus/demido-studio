//! The person's view of a session's browser, and their hands on it (DECISIONS Z12 to Z16).
//!
//! Over one WebSocket: the tab in front is drawn as the browser draws it (Chromium's screencast, a JPEG
//! picture each time the page changes, sent on only when the window took the one before, so a slow window is
//! never flooded); the tabs and their addresses and titles as they change. From the window: the mouse, the
//! keys, text pasted, an address, back, forward, reload, the tabs, and a point or a box to describe for the
//! person's marks. What the person does goes to the same tab the model's tools use; the person can go anywhere a
//! model's page can (the public internet, through Demido's proxy), with no robots.txt to keep to, since it is the
//! person browsing.

use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket};
use serde_json::{Value, json};

use super::Browser;
use super::cdp::Cdp;

/// Describes the element at a point, or the smallest one that holds a box, in words for a model: its kind, its
/// words, where it leads, and a selector to find it again. Run in Demido's own world on the page.
pub const DESCRIBE: &str = r#"(x, y, w, h) => {
  let el = document.elementFromPoint(w ? x + w / 2 : x, h ? y + h / 2 : y);
  if (!el) return null;
  // A drawing's insides are the drawing.
  if (el.closest('svg')) el = el.closest('svg');
  // A box: the smallest element that covers most of it (a box drawn by hand is never exact).
  const cover = (e) => {
    const r = e.getBoundingClientRect();
    const ix = Math.max(0, Math.min(r.right, x + w) - Math.max(r.left, x));
    const iy = Math.max(0, Math.min(r.bottom, y + h) - Math.max(r.top, y));
    return (ix * iy) / (w * h);
  };
  if (w && h) while (el.parentElement && cover(el) < 0.7) el = el.parentElement;
  const tidy = (s) => (s || '').replace(/\s+/g, ' ').trim();
  // A box's words are those inside it; a point's, those of what is there.
  const within = () => {
    const parts = [];
    const walk = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
    const range = document.createRange();
    for (let n = walk.nextNode(); n && parts.join(' ').length < 400; n = walk.nextNode()) {
      if (!n.textContent.trim()) continue;
      range.selectNodeContents(n);
      const r = range.getBoundingClientRect();
      const cx = r.left + r.width / 2, cy = r.top + r.height / 2;
      if (cx >= x && cx <= x + w && cy >= y && cy <= y + h) parts.push(n.textContent);
    }
    return tidy(parts.join(' '));
  };
  const path = (e) => {
    const parts = [];
    while (e && e.nodeType === 1 && e !== document.body && parts.length < 4) {
      let p = e.tagName.toLowerCase();
      if (e.id) { parts.unshift(p + '#' + CSS.escape(e.id)); break; }
      p += [...e.classList].slice(0, 2).map((c) => '.' + CSS.escape(c)).join('');
      const same = e.parentElement ? [...e.parentElement.children].filter((c) => c.tagName === e.tagName) : [];
      if (same.length > 1) p += ':nth-of-type(' + (same.indexOf(e) + 1) + ')';
      parts.unshift(p);
      e = e.parentElement;
    }
    return parts.join(' > ');
  };
  const r = el.getBoundingClientRect();
  const link = el.closest('a[href]');
  return {
    tag: el.tagName.toLowerCase(),
    role: el.getAttribute('role'),
    label: el.getAttribute('aria-label') || el.getAttribute('title') || el.getAttribute('alt') || (el.tagName.toLowerCase() === 'svg' ? tidy(el.querySelector('title')?.textContent) || null : null),
    text: (w && h ? within() : tidy(el.innerText || el.value || el.textContent)).slice(0, 300),
    href: link ? link.href : null,
    selector: path(el),
    box: [r.left, r.top, r.width, r.height],
  };
}"#;

/// The cursor a browser shows at a point: the page's own, else the text bar over words and fields, the pointer
/// over a link, the arrow elsewhere. Run in Demido's own world on the page.
pub const CURSOR: &str = r#"(x, y) => {
  const el = document.elementFromPoint(x, y);
  if (!el) return 'default';
  const own = getComputedStyle(el).cursor;
  if (own && own !== 'auto') return own.split(',').pop().trim();
  if (el.closest('a[href]')) return 'pointer';
  if (el.isContentEditable || el.matches('textarea, input:not([type=button], [type=submit], [type=reset], [type=checkbox], [type=radio], [type=range], [type=color], [type=file], [type=image])')) return 'text';
  const at = document.caretRangeFromPoint ? document.caretRangeFromPoint(x, y) : null;
  if (at && at.startContainer.nodeType === 3 && at.startContainer.textContent.trim()) {
    const words = document.createRange();
    words.selectNodeContents(at.startContainer);
    for (const r of words.getClientRects()) if (x >= r.left && x <= r.right && y >= r.top && y <= r.bottom) return 'text';
  }
  return 'default';
}"#;

/// The cursors the window shows: CSS's own names, never a picture from the page.
const CURSORS: &[&str] = &[
    "default",
    "pointer",
    "text",
    "vertical-text",
    "crosshair",
    "move",
    "grab",
    "grabbing",
    "not-allowed",
    "no-drop",
    "help",
    "wait",
    "progress",
    "copy",
    "alias",
    "cell",
    "context-menu",
    "zoom-in",
    "zoom-out",
    "col-resize",
    "row-resize",
    "ew-resize",
    "ns-resize",
    "nesw-resize",
    "nwse-resize",
    "n-resize",
    "s-resize",
    "e-resize",
    "w-resize",
    "ne-resize",
    "nw-resize",
    "se-resize",
    "sw-resize",
    "all-scroll",
    "none",
];

/// How long without a word from the window before its view counts as gone.
const QUIET_AT_MOST: Duration = Duration::from_secs(120);

fn said(kind: &str, data: Value) -> Message {
    let mut v = data;
    v["type"] = json!(kind);
    Message::Text(v.to_string().into())
}

/// The screencast of one tab, at the size the window shows it.
async fn watch(cdp: &Cdp, page: &str, size: (u32, u32), scale: f64) {
    let _ = cdp
        .call(
            "Page.startScreencast",
            json!({ "format": "jpeg", "quality": 70, "maxWidth": (f64::from(size.0) * scale) as u32, "maxHeight": (f64::from(size.1) * scale) as u32, "everyNthFrame": 1 }),
            Some(page),
        )
        .await;
}

/// The tabs, as the window lists them.
async fn tabs_said(browser: &Browser, session: &str) -> Message {
    let (tabs, front) = browser.tabs(session).await;
    said("tabs", json!({ "tabs": tabs, "front": front }))
}

/// Back or forward in the tab's history.
async fn step(cdp: &Cdp, page: &str, by: i64) -> Result<(), String> {
    let history = cdp.call("Page.getNavigationHistory", json!({}), Some(page)).await?;
    let at = history.get("currentIndex").and_then(Value::as_i64).unwrap_or(0) + by;
    let entries = history.get("entries").and_then(Value::as_array).cloned().unwrap_or_default();
    if let Some(entry) = usize::try_from(at).ok().and_then(|i| entries.get(i)) {
        cdp.call("Page.navigateToHistoryEntry", json!({ "entryId": entry.get("id") }), Some(page)).await?;
    }
    Ok(())
}

/// The window's view of a session's browser, until it closes.
pub async fn serve(mut socket: WebSocket, browser: Arc<Browser>, session: String) {
    let (mut cdp, mut page, mut target) = match browser.front(&session).await {
        Ok(front) => front,
        Err(why) => {
            let _ = socket.send(said("problem", json!({ "message": why }))).await;
            return;
        }
    };
    let mut news = cdp.listen();
    let (mut size, mut scale) = ((1280u32, 800u32), 1.0f64);
    let mut watching = false;
    let _ = socket.send(tabs_said(&browser, &session).await).await;
    let mut tick = tokio::time::interval(Duration::from_secs(2));
    let mut last_word = std::time::Instant::now();
    // Where the mouse last was, to say which cursor the page wants there, a few times a second at most.
    let mut hovering: Option<(f64, f64)> = None;
    let mut shown_cursor = String::from("default");
    let mut look = tokio::time::interval(Duration::from_millis(90));
    look.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            got = socket.recv() => {
                let Some(Ok(message)) = got else { break };
                last_word = std::time::Instant::now();
                let Message::Text(text) = message else { continue };
                let Ok(asked) = serde_json::from_str::<Value>(&text) else { continue };
                let num = |k: &str| asked.get(k).and_then(Value::as_f64).unwrap_or(0.0);
                let text_of = |k: &str| asked.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
                match asked.get("type").and_then(Value::as_str).unwrap_or_default() {
                    "size" => {
                        size = ((num("width") as u32).max(320), (num("height") as u32).max(240));
                        scale = num("scale").clamp(1.0, 2.0);
                        browser.resize(&session, size.0, size.1).await;
                        let _ = cdp.call("Page.stopScreencast", json!({}), Some(&page)).await;
                        watch(&cdp, &page, size, scale).await;
                        watching = true;
                    }
                    "mouse" => {
                        let kind = text_of("event");
                        if !matches!(kind.as_str(), "mousePressed" | "mouseReleased" | "mouseMoved" | "mouseWheel") { continue }
                        let _ = cdp.call("Input.dispatchMouseEvent", json!({
                            "type": kind, "x": num("x"), "y": num("y"), "button": asked.get("button").cloned().unwrap_or(json!("none")),
                            "buttons": num("buttons") as i64, "clickCount": num("clickCount") as i64, "modifiers": num("modifiers") as i64,
                            "deltaX": num("deltaX"), "deltaY": num("deltaY"),
                        }), Some(&page)).await;
                        if kind == "mouseMoved" || kind == "mouseReleased" { hovering = Some((num("x"), num("y"))); }
                        // Whether the person is in a password box: the window says what the model can then use.
                        if kind == "mouseReleased" {
                            let focus = cdp.call("Runtime.evaluate", json!({ "expression": "document.activeElement ? document.activeElement.type === 'password' : false", "returnByValue": true }), Some(&page)).await;
                            let password = focus.ok().and_then(|f| f.pointer("/result/value").and_then(Value::as_bool)).unwrap_or(false);
                            let _ = socket.send(said("focus", json!({ "password": password }))).await;
                        }
                    }
                    "key" => {
                        let kind = text_of("event");
                        if !matches!(kind.as_str(), "keyDown" | "keyUp" | "rawKeyDown" | "char") { continue }
                        let mut key = json!({ "type": kind, "key": text_of("key"), "code": text_of("code"), "modifiers": num("modifiers") as i64,
                            "windowsVirtualKeyCode": num("keyCode") as i64, "nativeVirtualKeyCode": num("keyCode") as i64 });
                        let typed = text_of("text");
                        if !typed.is_empty() { key["text"] = json!(typed); key["unmodifiedText"] = json!(typed); }
                        let _ = cdp.call("Input.dispatchKeyEvent", key, Some(&page)).await;
                    }
                    "text" => { let _ = cdp.call("Input.insertText", json!({ "text": text_of("text") }), Some(&page)).await; }
                    "go" => match browser.person_may_go(&text_of("address")) {
                        Ok(url) => {
                            let went = cdp.call("Page.navigate", json!({ "url": url.as_str() }), Some(&page)).await;
                            if let Some(error) = went.ok().and_then(|w| w.get("errorText").and_then(Value::as_str).map(str::to_string)).filter(|e| !e.is_empty()) {
                                let _ = socket.send(said("problem", json!({ "message": super::why_not_loaded(&error) }))).await;
                            }
                        }
                        Err(why) => { let _ = socket.send(said("problem", json!({ "message": why }))).await; }
                    },
                    "back" => { let _ = step(&cdp, &page, -1).await; }
                    "forward" => { let _ = step(&cdp, &page, 1).await; }
                    "reload" => { let _ = cdp.call("Page.reload", json!({}), Some(&page)).await; }
                    "stop" => { let _ = cdp.call("Page.stopLoading", json!({}), Some(&page)).await; }
                    "newTab" => {
                        let address = Some(text_of("address")).filter(|a| !a.trim().is_empty());
                        if let Err(why) = browser.open_tab(&session, address.as_deref()).await {
                            let _ = socket.send(said("problem", json!({ "message": why }))).await;
                        }
                        tick.reset_immediately();
                    }
                    "activate" => { browser.activate(&session, &text_of("id")).await; tick.reset_immediately(); }
                    "closeTab" => { browser.close_tab(&session, &text_of("id")).await; tick.reset_immediately(); }
                    "describe" => {
                        let described = Browser::describe(&cdp, &page, num("x"), num("y"), num("w"), num("h")).await.unwrap_or(Value::Null);
                        let _ = socket.send(said("described", json!({ "ask": asked.get("ask"), "element": described }))).await;
                    }
                    // "here": the window is still watching, which the word itself says.
                    _ => {}
                }
            }
            heard = news.recv() => {
                let Ok(heard) = heard else { continue };
                match heard.method.as_str() {
                    "Page.screencastFrame" if heard.session.as_deref() == Some(page.as_str()) => {
                        let frame = &heard.params;
                        let sent = socket.send(said("frame", json!({ "data": frame.get("data"), "meta": frame.get("metadata") }))).await;
                        // Taken by the window: the browser may send the next.
                        let _ = cdp.call("Page.screencastFrameAck", json!({ "sessionId": frame.get("sessionId") }), Some(&page)).await;
                        if sent.is_err() { break }
                    }
                    "Target.targetInfoChanged" => {
                        let info = &heard.params["targetInfo"];
                        let id = info.get("targetId").and_then(Value::as_str).unwrap_or_default();
                        if browser.knows(&session, id, None, None).await.0 {
                            let (address, title) = (info.get("url").and_then(Value::as_str).unwrap_or_default(), info.get("title").and_then(Value::as_str).unwrap_or_default());
                            browser.heard(id, address, title).await;
                            let _ = socket.send(tabs_said(&browser, &session).await).await;
                        }
                    }
                    "Target.targetCreated" => {
                        let info = &heard.params["targetInfo"];
                        let (id, kind) = (info.get("targetId").and_then(Value::as_str).unwrap_or_default(), info.get("type").and_then(Value::as_str).unwrap_or_default());
                        let (opener, context) = (info.get("openerId").and_then(Value::as_str), info.get("browserContextId").and_then(Value::as_str));
                        if kind == "page" && browser.knows(&session, id, opener, context).await.1 && browser.adopt(&session, id).await.is_ok() {
                            tick.reset_immediately();
                        }
                    }
                    "Target.targetDestroyed" => { let _ = socket.send(tabs_said(&browser, &session).await).await; }
                    // A page that loaded says its title then, which its tab's news does not always carry.
                    "Page.domContentEventFired" | "Page.loadEventFired" if heard.session.as_deref() == Some(page.as_str()) => {
                        if let Ok(v) = cdp.call("Runtime.evaluate", json!({ "expression": "[location.href, document.title]", "returnByValue": true }), Some(&page)).await {
                            let (address, title) = (v.pointer("/result/value/0").and_then(Value::as_str).unwrap_or_default(), v.pointer("/result/value/1").and_then(Value::as_str).unwrap_or_default());
                            browser.heard(&target, address, title).await;
                            let _ = socket.send(tabs_said(&browser, &session).await).await;
                        }
                    }
                    // A page's own dialog would hold the tab: it is dismissed, and the person told what it said.
                    "Page.javascriptDialogOpening" if heard.session.as_deref() == Some(page.as_str()) => {
                        let _ = cdp.call("Page.handleJavaScriptDialog", json!({ "accept": false }), Some(&page)).await;
                        let _ = socket.send(said("dialog", json!({ "message": heard.params.get("message") }))).await;
                    }
                    _ => {}
                }
            }
            _ = look.tick(), if hovering.is_some() => {
                let (x, y) = hovering.take().expect("just looked");
                let wanted = Browser::in_our_world(&cdp, &page, &format!("({CURSOR})({x}, {y})")).await.ok();
                let wanted = wanted.as_ref().and_then(Value::as_str).filter(|c| CURSORS.contains(c)).unwrap_or("default");
                if wanted != shown_cursor {
                    shown_cursor = wanted.to_string();
                    let _ = socket.send(said("cursor", json!({ "cursor": wanted }))).await;
                }
            }
            _ = tick.tick() => {
                if last_word.elapsed() > QUIET_AT_MOST { break }
                // The tab in front changed (the person chose another, a model's tool, a closed tab): watch that one.
                match browser.front_if_any(&session).await {
                    Some((now_page, now_target)) if now_target != target => {
                        let _ = cdp.call("Page.stopScreencast", json!({}), Some(&page)).await;
                        (page, target) = (now_page, now_target);
                        if watching { watch(&cdp, &page, size, scale).await; }
                        let _ = socket.send(tabs_said(&browser, &session).await).await;
                    }
                    Some(_) => {}
                    // Closed, or the browser stopped: start again on a fresh tab.
                    None => match browser.front(&session).await {
                        Ok(front) => {
                            if !Arc::ptr_eq(&cdp, &front.0) { news = front.0.listen(); }
                            (cdp, page, target) = front;
                            browser.resize(&session, size.0, size.1).await;
                            if watching { watch(&cdp, &page, size, scale).await; }
                            let _ = socket.send(tabs_said(&browser, &session).await).await;
                        }
                        Err(why) => { let _ = socket.send(said("problem", json!({ "message": why }))).await; break }
                    },
                }
            }
        }
    }
    let _ = cdp.call("Page.stopScreencast", json!({}), Some(&page)).await;
}
