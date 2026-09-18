//! The queue against a real socket, a real filesystem and a server that
//! misbehaves on purpose.
//!
//! Everything worth asserting about a download is in what a unit test cannot
//! reach: what the server was asked for on a retry, what is on disk after a
//! connection dies mid-body, whether a partial file survives a pause and not a
//! cancel. So the server is real and badly behaved, and the assertions are
//! about bytes on disk and requests on the wire.
//!
//! Hand-rolled rather than a mock-server crate, as v2's was: the server is a
//! hundred lines, and a dependency here is one in every build that downloads.
//! The bodies are real GGUF files, because a finished download is checked to be
//! one before it is offered.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use demido_download::file::{Entry, Held};
use demido_download::{Failure, Files, Item, Piece, Queue, State};
use demido_models::index::File;
use demido_models::{choices, Folders, Library};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

// --- the server --------------------------------------------------------------

/// What the server does with one request for a path.
#[derive(Clone, Debug)]
enum Act {
    /// Serve the body, honouring `Range`.
    Serve,
    /// Promise the rest of the body, send this many bytes of it and close the
    /// connection cleanly: a truncated response.
    Truncate(usize),
    /// Send this many bytes and then reset the connection.
    Reset(usize),
    /// Send the whole body with `200`, whatever `Range` said.
    IgnoreRange,
    /// Answer with this status and a short body.
    Status(u16),
    /// Claim a `Content-Length` that is not the file's.
    WrongLength(u64),
    /// Redirect somewhere else on this server.
    Redirect(&'static str),
    /// A login page, `200` and `text/html`.
    Html,
    /// The body a kilobyte at a time, so there is a transfer to pause.
    Slow,
    /// Headers and a little of the body, then nothing, forever.
    Trickle,
    /// Promise this many bytes, send a kilobyte, then nothing, forever: a
    /// large transfer that stays running for as long as a test needs.
    Promise(u64),
}

struct Route {
    body: Vec<u8>,
    acts: VecDeque<Act>,
}

/// One request the server saw: its path and where it asked to start.
#[derive(Clone, Debug)]
struct Seen {
    path: String,
    from: Option<u64>,
}

struct Server {
    base: String,
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl Server {
    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    fn requests(&self, path: &str) -> Vec<Option<u64>> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .filter(|seen| seen.path == path)
            .map(|seen| seen.from)
            .collect()
    }
}

/// A server answering each path from its own script, one act per request, and
/// serving normally once a script runs out. Per path rather than per
/// connection, so two items downloading at once each meet their own script.
async fn serve(routes: Vec<(&str, Vec<u8>, Vec<Act>)>) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bound");
    let base = format!("http://{}", listener.local_addr().expect("addressed"));
    let routes: Arc<Mutex<HashMap<String, Route>>> = Arc::new(Mutex::new(
        routes
            .into_iter()
            .map(|(path, body, acts)| {
                (
                    path.to_owned(),
                    Route {
                        body,
                        acts: acts.into(),
                    },
                )
            })
            .collect(),
    ));
    let seen = Arc::new(Mutex::new(Vec::new()));

    let recorded = seen.clone();
    tokio::spawn(async move {
        loop {
            let Ok((socket, _)) = listener.accept().await else {
                return;
            };
            let routes = routes.clone();
            let seen = recorded.clone();
            tokio::spawn(async move { answer(socket, routes, seen).await });
        }
    });
    Server { base, seen }
}

async fn answer(
    mut socket: TcpStream,
    routes: Arc<Mutex<HashMap<String, Route>>>,
    seen: Arc<Mutex<Vec<Seen>>>,
) {
    socket.set_nodelay(true).ok();
    let request = read_request(&mut socket).await;
    let path = request.split_whitespace().nth(1).unwrap_or("/").to_owned();
    let from = range(&request);
    seen.lock().unwrap().push(Seen {
        path: path.clone(),
        from,
    });

    let (body, act) = {
        let mut routes = routes.lock().unwrap();
        match routes.get_mut(&path) {
            Some(route) => (
                route.body.clone(),
                route.acts.pop_front().unwrap_or(Act::Serve),
            ),
            None => (Vec::new(), Act::Status(404)),
        }
    };
    let start = from.unwrap_or(0) as usize;

    match act {
        Act::Serve => {
            socket.write_all(head(&body, start).as_bytes()).await.ok();
            socket.write_all(&body[start..]).await.ok();
        }
        Act::Truncate(sent) => {
            socket.write_all(head(&body, start).as_bytes()).await.ok();
            let end = (start + sent).min(body.len());
            socket.write_all(&body[start..end]).await.ok();
            socket.flush().await.ok();
            socket.shutdown().await.ok();
            // Drained, so the close is a clean one rather than a reset.
            let mut rest = Vec::new();
            socket.read_to_end(&mut rest).await.ok();
            return;
        }
        Act::Reset(sent) => {
            socket.write_all(head(&body, start).as_bytes()).await.ok();
            let end = (start + sent).min(body.len());
            socket.write_all(&body[start..end]).await.ok();
            socket.flush().await.ok();
            tokio::time::sleep(Duration::from_millis(100)).await;
            // A zero linger turns the close into a reset.
            socket2::SockRef::from(&socket)
                .set_linger(Some(Duration::ZERO))
                .ok();
            drop(socket);
            return;
        }
        Act::IgnoreRange => {
            socket.write_all(head(&body, 0).as_bytes()).await.ok();
            socket.write_all(&body).await.ok();
        }
        Act::Status(status) => {
            let head =
                format!("HTTP/1.1 {status} No\r\nContent-Length: 2\r\nConnection: close\r\n\r\nno");
            socket.write_all(head.as_bytes()).await.ok();
        }
        Act::WrongLength(length) => {
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {length}\r\nConnection: close\r\n\r\n"
            );
            socket.write_all(head.as_bytes()).await.ok();
            socket.write_all(&body).await.ok();
        }
        Act::Redirect(to) => {
            let head =
                format!("HTTP/1.1 302 Found\r\nLocation: {to}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            socket.write_all(head.as_bytes()).await.ok();
        }
        Act::Html => {
            let page = b"<!doctype html><title>Log in</title><form>Sign in to continue</form>";
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                page.len()
            );
            socket.write_all(head.as_bytes()).await.ok();
            socket.write_all(page).await.ok();
        }
        Act::Slow => {
            socket.write_all(head(&body, start).as_bytes()).await.ok();
            for chunk in body[start..].chunks(1024) {
                // The client hanging up is how a pause ends this.
                if socket.write_all(chunk).await.is_err() {
                    return;
                }
                socket.flush().await.ok();
                tokio::time::sleep(Duration::from_millis(15)).await;
            }
        }
        Act::Promise(length) => {
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {length}\r\nConnection: close\r\n\r\n"
            );
            socket.write_all(head.as_bytes()).await.ok();
            socket.write_all(&[0u8; 1024]).await.ok();
            socket.flush().await.ok();
            std::future::pending::<()>().await;
        }
        Act::Trickle => {
            socket.write_all(head(&body, start).as_bytes()).await.ok();
            socket.write_all(&body[start..start + 100]).await.ok();
            socket.flush().await.ok();
            std::future::pending::<()>().await;
        }
    }
    socket.shutdown().await.ok();
}

/// Headers for the body from `start`: `200` from zero, `206` otherwise.
fn head(body: &[u8], start: usize) -> String {
    if start == 0 {
        return format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n",
            body.len()
        );
    }
    format!(
        "HTTP/1.1 206 Partial Content\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nContent-Range: bytes {start}-{}/{}\r\nConnection: close\r\n\r\n",
        body.len() - start,
        body.len() - 1,
        body.len()
    )
}

async fn read_request(socket: &mut TcpStream) -> String {
    let mut request = Vec::new();
    let mut buffer = [0u8; 2048];
    loop {
        let read = socket.read(&mut buffer).await.unwrap_or(0);
        request.extend_from_slice(&buffer[..read]);
        if read == 0 || request.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
    }
    String::from_utf8_lossy(&request).into_owned()
}

fn range(request: &str) -> Option<u64> {
    request.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        if !name.eq_ignore_ascii_case("range") {
            return None;
        }
        value
            .trim()
            .strip_prefix("bytes=")?
            .split('-')
            .next()?
            .parse()
            .ok()
    })
}

// --- the files ---------------------------------------------------------------

/// A real GGUF: a header, one F32 tensor of `values`, and its data. What the
/// library verifies, so what a finished download has to be.
fn gguf(values: u64) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"GGUF");
    out.extend_from_slice(&3u32.to_le_bytes());
    out.extend_from_slice(&1u64.to_le_bytes()); // one tensor
    out.extend_from_slice(&1u64.to_le_bytes()); // one key
    let key = "general.architecture";
    out.extend_from_slice(&(key.len() as u64).to_le_bytes());
    out.extend_from_slice(key.as_bytes());
    out.extend_from_slice(&8u32.to_le_bytes()); // a string
    out.extend_from_slice(&5u64.to_le_bytes());
    out.extend_from_slice(b"llama");
    let name = "token_embd.weight";
    out.extend_from_slice(&(name.len() as u64).to_le_bytes());
    out.extend_from_slice(name.as_bytes());
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&values.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // F32
    out.extend_from_slice(&0u64.to_le_bytes());
    while out.len() % 32 != 0 {
        out.push(0);
    }
    // Not all zeros, so a body written at the wrong offset is visibly wrong.
    out.extend((0..values * 4).map(|at| (at % 251) as u8));
    out
}

/// Sixty-four kilobytes of model: long enough to be cut in the middle.
fn model() -> Vec<u8> {
    gguf(16 * 1024)
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join("demido-download-over-http")
        .join(format!("{name}-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).expect("scratch");
    dir
}

/// A one-file item from `url` into `dir`, with the size the index would state.
fn item(url: String, dir: &Path, file: &str, body: &[u8]) -> Item {
    Item {
        repo: "test/model-GGUF".into(),
        name: file.trim_end_matches(".gguf").into(),
        files: vec![Piece {
            url,
            destination: dir.join("test").join("model-GGUF").join(file),
            bytes: body.len() as u64,
            sha256: None,
        }],
    }
}

fn queue(dir: &Path) -> Queue {
    Queue::with(Files::in_profile(dir), 2, Duration::from_millis(500))
}

async fn settled(queue: &Queue, id: demido_download::Id) -> Option<State> {
    tokio::time::timeout(Duration::from_secs(20), queue.settled(id))
        .await
        .expect("the item settled")
}

/// Until the item has bytes on disk, so a pause or a cancel meets a running
/// transfer rather than one waiting for a slot.
async fn moving(queue: &Queue, id: demido_download::Id) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while !queue
            .row(id)
            .is_some_and(|row| row.state == State::Running && row.received > 0)
        {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("the transfer moved");
}

fn failure(state: Option<State>) -> Failure {
    match state {
        Some(State::Failed { failure }) => failure,
        other => panic!("expected a failure, got {other:?}"),
    }
}

// --- the cases ---------------------------------------------------------------

#[tokio::test]
async fn a_finished_item_is_verified_renamed_into_place_and_offered_by_the_library() {
    let dir = scratch("plain");
    let body = model();
    let server = serve(vec![("/m.gguf", body.clone(), vec![])]).await;
    let queue = queue(&dir);

    let item = item(server.url("/m.gguf"), &dir, "m.gguf", &body);
    let piece = item.files[0].clone();
    let id = queue.enqueue(item);
    assert_eq!(settled(&queue, id).await, Some(State::Done));

    assert_eq!(std::fs::read(&piece.destination).expect("landed"), body);
    assert!(
        !piece.partial().exists(),
        "the partial file is renamed, not left"
    );
    let scan = Library::open(&Folders {
        download: dir.clone(),
        scan: vec![],
    })
    .scan();
    assert_eq!(scan.models.len(), 1, "damaged: {:?}", scan.damaged);
    assert_eq!(scan.models[0].path, piece.destination);
    assert_eq!(
        queue.row(id).expect("the finished row stays").path,
        scan.models[0].path,
        "the row names the model it arrived as, so the window can offer it"
    );
    assert!(
        Files::in_profile(&dir).read().is_empty(),
        "a finished item leaves the queue file: the library is its record"
    );
}

#[tokio::test]
async fn a_truncated_response_fails_as_one_and_the_retry_resumes_from_the_bytes_on_disk() {
    let dir = scratch("truncated");
    let body = model();
    let server = serve(vec![("/m.gguf", body.clone(), vec![Act::Truncate(20_000)])]).await;
    let queue = queue(&dir);

    let item = item(server.url("/m.gguf"), &dir, "m.gguf", &body);
    let piece = item.files[0].clone();
    let id = queue.enqueue(item);

    assert_eq!(
        failure(settled(&queue, id).await),
        Failure::EndedEarly {
            received: 20_000,
            expected: body.len() as u64
        }
    );
    assert!(
        !piece.destination.exists(),
        "an incomplete file never appears"
    );
    assert_eq!(
        std::fs::metadata(piece.partial()).expect("kept").len(),
        20_000
    );

    queue.resume(id);
    assert_eq!(settled(&queue, id).await, Some(State::Done));
    assert_eq!(std::fs::read(&piece.destination).expect("landed"), body);
    assert_eq!(server.requests("/m.gguf"), vec![None, Some(20_000)]);
}

#[tokio::test]
async fn a_connection_reset_mid_body_is_named_as_a_reset_and_resumes() {
    let dir = scratch("reset");
    let body = model();
    let server = serve(vec![("/m.gguf", body.clone(), vec![Act::Reset(30_000)])]).await;
    let queue = queue(&dir);

    let item = item(server.url("/m.gguf"), &dir, "m.gguf", &body);
    let piece = item.files[0].clone();
    let id = queue.enqueue(item);

    let Failure::Reset { received } = failure(settled(&queue, id).await) else {
        panic!("a reset reads as a reset");
    };
    let kept = std::fs::metadata(piece.partial()).expect("kept").len();
    assert_eq!(kept, received);

    queue.resume(id);
    assert_eq!(settled(&queue, id).await, Some(State::Done));
    assert_eq!(std::fs::read(&piece.destination).expect("landed"), body);
    let asked = server.requests("/m.gguf");
    assert_eq!(
        asked[1],
        (kept > 0).then_some(kept),
        "the retry asks for the rest"
    );
}

#[tokio::test]
async fn a_server_that_ignores_range_is_written_from_zero_rather_than_appended() {
    let dir = scratch("ignores-range");
    let body = model();
    let server = serve(vec![("/m.gguf", body.clone(), vec![Act::IgnoreRange])]).await;
    let queue = queue(&dir);

    let item = item(server.url("/m.gguf"), &dir, "m.gguf", &body);
    let piece = item.files[0].clone();
    std::fs::create_dir_all(piece.destination.parent().unwrap()).unwrap();
    std::fs::write(piece.partial(), &body[..10_000]).expect("an earlier attempt");

    let id = queue.enqueue(item);
    assert_eq!(settled(&queue, id).await, Some(State::Done));
    assert_eq!(server.requests("/m.gguf"), vec![Some(10_000)]);
    assert_eq!(
        std::fs::read(&piece.destination).expect("landed"),
        body,
        "a 200 replaces the partial file instead of being appended to it"
    );
}

#[tokio::test]
async fn a_content_length_that_is_not_the_listed_size_is_refused_before_it_is_written() {
    let dir = scratch("wrong-length");
    let body = model();
    let server = serve(vec![(
        "/m.gguf",
        body.clone(),
        vec![Act::WrongLength(body.len() as u64 + 4096)],
    )])
    .await;
    let queue = queue(&dir);

    let item = item(server.url("/m.gguf"), &dir, "m.gguf", &body);
    let piece = item.files[0].clone();
    let id = queue.enqueue(item);

    assert_eq!(
        failure(settled(&queue, id).await),
        Failure::WrongSize {
            stated: body.len() as u64 + 4096,
            expected: body.len() as u64
        }
    );
    assert!(!piece.partial().exists());
    assert!(!piece.destination.exists());
}

#[tokio::test]
async fn a_login_page_behind_a_redirect_is_not_saved_as_a_model() {
    let dir = scratch("login");
    let body = model();
    let server = serve(vec![
        ("/m.gguf", body.clone(), vec![Act::Redirect("/login")]),
        ("/login", Vec::new(), vec![Act::Html]),
    ])
    .await;
    let queue = queue(&dir);

    let item = item(server.url("/m.gguf"), &dir, "m.gguf", &body);
    let piece = item.files[0].clone();
    let id = queue.enqueue(item);

    assert!(matches!(
        failure(settled(&queue, id).await),
        Failure::NotTheFile { content_type } if content_type.starts_with("text/html")
    ));
    assert!(!piece.partial().exists(), "not a byte of the page is kept");
    assert!(!piece.destination.exists());
}

#[tokio::test]
async fn a_page_that_hides_its_type_is_still_not_offered() {
    let dir = scratch("disguised");
    // As long as the listing says, served as bytes, and not a model.
    let page = vec![b'<'; model().len()];
    let server = serve(vec![("/m.gguf", page.clone(), vec![])]).await;
    let queue = queue(&dir);

    let item = item(server.url("/m.gguf"), &dir, "m.gguf", &page);
    let piece = item.files[0].clone();
    let id = queue.enqueue(item);

    assert!(matches!(
        failure(settled(&queue, id).await),
        Failure::Damaged { .. }
    ));
    assert!(!piece.destination.exists(), "verified before it is offered");
    assert!(!piece.partial().exists(), "and not resumed from either");
}

#[tokio::test]
async fn a_gated_file_a_missing_one_and_a_digest_mismatch_read_differently() {
    let dir = scratch("causes");
    let body = model();
    let server = serve(vec![
        ("/gated.gguf", body.clone(), vec![Act::Status(401)]),
        ("/gone.gguf", body.clone(), vec![Act::Status(404)]),
        ("/wrong.gguf", body.clone(), vec![]),
    ])
    .await;
    let queue = queue(&dir);

    let gated = queue.enqueue(item(server.url("/gated.gguf"), &dir, "gated.gguf", &body));
    let gone = queue.enqueue(item(server.url("/gone.gguf"), &dir, "gone.gguf", &body));
    let mut wrong = item(server.url("/wrong.gguf"), &dir, "wrong.gguf", &body);
    wrong.files[0].sha256 = Some("0".repeat(64));
    let partial = wrong.files[0].partial();
    let wrong = queue.enqueue(wrong);

    assert_eq!(
        failure(settled(&queue, gated).await),
        Failure::Gated { status: 401 }
    );
    assert_eq!(failure(settled(&queue, gone).await), Failure::Missing);
    assert_eq!(failure(settled(&queue, wrong).await), Failure::Corrupt);
    assert!(
        !partial.exists(),
        "bytes that failed their digest are not resumed from"
    );
}

#[tokio::test]
async fn a_silent_server_is_a_stall_rather_than_a_hang() {
    let dir = scratch("stall");
    let body = model();
    let server = serve(vec![("/m.gguf", body.clone(), vec![Act::Trickle])]).await;
    let queue = queue(&dir);

    let id = queue.enqueue(item(server.url("/m.gguf"), &dir, "m.gguf", &body));
    assert!(matches!(
        failure(settled(&queue, id).await),
        Failure::Stalled { .. }
    ));
}

#[tokio::test]
async fn one_failure_leaves_the_other_downloads_running() {
    let dir = scratch("isolated");
    let body = model();
    let server = serve(vec![
        ("/bad.gguf", body.clone(), vec![Act::Reset(5_000)]),
        ("/good.gguf", body.clone(), vec![Act::Slow]),
    ])
    .await;
    let queue = queue(&dir);

    let good = queue.enqueue(item(server.url("/good.gguf"), &dir, "good.gguf", &body));
    let bad = queue.enqueue(item(server.url("/bad.gguf"), &dir, "bad.gguf", &body));

    assert!(matches!(
        failure(settled(&queue, bad).await),
        Failure::Reset { .. }
    ));
    assert!(
        matches!(queue.row(good).map(|row| row.state), Some(State::Running)),
        "the other download is still moving when the first fails"
    );
    assert_eq!(settled(&queue, good).await, Some(State::Done));
}

#[tokio::test]
async fn pausing_keeps_the_bytes_and_resuming_asks_for_the_rest() {
    let dir = scratch("pause");
    let body = model();
    let server = serve(vec![("/m.gguf", body.clone(), vec![Act::Slow])]).await;
    let queue = queue(&dir);

    let item = item(server.url("/m.gguf"), &dir, "m.gguf", &body);
    let piece = item.files[0].clone();
    let id = queue.enqueue(item);
    moving(&queue, id).await;
    queue.pause(id);

    assert_eq!(settled(&queue, id).await, Some(State::Paused));
    let kept = std::fs::metadata(piece.partial()).expect("kept").len();
    assert!(kept > 0);
    assert_eq!(
        queue.row(id).unwrap().received,
        kept,
        "the row is the bytes on disk"
    );
    assert!(!piece.destination.exists());

    queue.resume(id);
    assert_eq!(settled(&queue, id).await, Some(State::Done));
    assert_eq!(std::fs::read(&piece.destination).expect("landed"), body);
    assert_eq!(server.requests("/m.gguf"), vec![None, Some(kept)]);
}

#[tokio::test]
async fn pause_all_pauses_every_item_including_one_waiting_for_a_slot() {
    let dir = scratch("pause-all");
    let body = model();
    let server = serve(vec![
        ("/a.gguf", body.clone(), vec![Act::Slow]),
        ("/b.gguf", body.clone(), vec![Act::Slow]),
        ("/c.gguf", body.clone(), vec![Act::Slow]),
    ])
    .await;
    // Two slots, three items: one of them is queued, not running.
    let queue = queue(&dir);
    let ids: Vec<_> = ["a", "b", "c"]
        .iter()
        .map(|name| {
            queue.enqueue(item(
                server.url(&format!("/{name}.gguf")),
                &dir,
                &format!("{name}.gguf"),
                &body,
            ))
        })
        .collect();
    moving(&queue, ids[0]).await;
    moving(&queue, ids[1]).await;

    queue.pause_all();
    for id in &ids {
        assert_eq!(settled(&queue, *id).await, Some(State::Paused));
    }
    assert!(
        Files::in_profile(&dir)
            .read()
            .iter()
            .all(|entry| entry.held == Some(Held::Paused)),
        "and a restart keeps them paused"
    );
}

#[tokio::test]
async fn a_cancel_removes_the_partial_file_and_the_row() {
    let dir = scratch("cancel");
    let body = model();
    let server = serve(vec![("/m.gguf", body.clone(), vec![Act::Slow])]).await;
    let queue = queue(&dir);

    let item = item(server.url("/m.gguf"), &dir, "m.gguf", &body);
    let piece = item.files[0].clone();
    let id = queue.enqueue(item);
    moving(&queue, id).await;
    assert!(piece.partial().exists());

    queue.cancel(id);
    assert_eq!(settled(&queue, id).await, None, "the row left the queue");
    assert!(
        !piece.partial().exists(),
        "cancelling does not keep the bytes"
    );
    assert!(!piece.destination.exists());
    assert!(Files::in_profile(&dir).read().is_empty());
}

#[tokio::test]
async fn a_cancel_of_a_paused_or_failed_item_removes_its_bytes_too() {
    let dir = scratch("cancel-held");
    let body = model();
    let server = serve(vec![("/m.gguf", body.clone(), vec![Act::Truncate(9_000)])]).await;
    let queue = queue(&dir);

    let item = item(server.url("/m.gguf"), &dir, "m.gguf", &body);
    let piece = item.files[0].clone();
    let id = queue.enqueue(item);
    failure(settled(&queue, id).await);
    assert!(piece.partial().exists());

    queue.cancel(id);
    assert!(queue.row(id).is_none());
    assert!(!piece.partial().exists());
}

/// What a process killed mid-download leaves: an entry nobody paused, and a
/// partial file. The next launch asks for the rest.
#[tokio::test]
async fn a_restart_picks_up_from_the_bytes_on_disk() {
    let dir = scratch("restart");
    let body = model();
    let server = serve(vec![("/m.gguf", body.clone(), vec![])]).await;

    let item = item(server.url("/m.gguf"), &dir, "m.gguf", &body);
    let piece = item.files[0].clone();
    std::fs::create_dir_all(piece.destination.parent().unwrap()).unwrap();
    std::fs::write(piece.partial(), &body[..25_000]).expect("what the last run got");
    Files::in_profile(&dir)
        .write(&[Entry {
            id: 7,
            item,
            held: None,
        }])
        .expect("what the last run asked for");

    let queue = queue(&dir);
    let rows = queue.rows();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].received, 25_000, "progress is read off the disk");
    assert_eq!(rows[0].state, State::Queued);

    queue.start();
    let id = rows[0].id;
    assert_eq!(settled(&queue, id).await, Some(State::Done));
    assert_eq!(std::fs::read(&piece.destination).expect("landed"), body);
    assert_eq!(server.requests("/m.gguf"), vec![Some(25_000)]);
}

#[tokio::test]
async fn a_paused_item_and_a_failed_one_come_back_as_they_were() {
    let dir = scratch("restart-held");
    let body = model();
    let server = serve(vec![
        ("/p.gguf", body.clone(), vec![Act::Slow]),
        ("/f.gguf", body.clone(), vec![Act::Status(403)]),
    ])
    .await;
    {
        let queue = queue(&dir);
        let paused = queue.enqueue(item(server.url("/p.gguf"), &dir, "p.gguf", &body));
        let failed = queue.enqueue(item(server.url("/f.gguf"), &dir, "f.gguf", &body));
        moving(&queue, paused).await;
        queue.pause(paused);
        settled(&queue, paused).await;
        failure(settled(&queue, failed).await);
    }

    let queue = queue(&dir);
    queue.start();
    let rows = queue.rows();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].state, State::Paused);
    assert!(rows[0].received > 0);
    assert_eq!(
        rows[1].state,
        State::Failed {
            failure: Failure::Gated { status: 403 }
        },
        "a failed row still says why after a restart"
    );
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        server.requests("/p.gguf").len(),
        1,
        "a paused item stays paused"
    );
}

#[tokio::test]
async fn the_queue_is_the_profiles_own() {
    let mine = scratch("profile-mine");
    let theirs = scratch("profile-theirs");
    let body = model();
    let server = serve(vec![("/m.gguf", body.clone(), vec![Act::Status(500)])]).await;

    let queue = queue(&mine);
    let id = queue.enqueue(item(server.url("/m.gguf"), &mine, "m.gguf", &body));
    failure(settled(&queue, id).await);

    assert_eq!(rows_in(&mine), 1);
    assert_eq!(rows_in(&theirs), 0);
}

fn rows_in(profile: &Path) -> usize {
    queue(profile).rows().len()
}

#[tokio::test]
async fn free_space_is_checked_before_a_byte_is_asked_for() {
    let dir = scratch("room");
    let server = serve(vec![]).await;
    let queue = queue(&dir);

    let mut item = item(server.url("/huge.gguf"), &dir, "huge.gguf", &[]);
    // A petabyte: more than any volume this runs on has free.
    item.files[0].bytes = 1 << 50;
    let id = queue.enqueue(item);

    let Failure::NoRoom { needs, free } = failure(settled(&queue, id).await) else {
        panic!("refused for room");
    };
    assert_eq!(needs, 1 << 50);
    assert!(free < needs);
    assert!(
        server.requests("/huge.gguf").is_empty(),
        "refused while it was cheap"
    );
}

/// A split model and its projector, chosen from a listing, are one item, and
/// arrive as one model with its projector beside it.
#[tokio::test]
async fn a_split_model_and_its_projector_are_one_item_and_one_model() {
    let dir = scratch("split");
    let first = gguf(4096);
    let second = gguf(2048);
    let projector = gguf(1024);
    let repo = "test/split-GGUF";
    let path = |file: &str| format!("/{repo}/resolve/main/{file}");
    let server = serve(vec![
        (
            path("split-Q4_K_M-00001-of-00002.gguf").as_str(),
            first.clone(),
            vec![Act::Truncate(3_000)],
        ),
        (
            path("split-Q4_K_M-00002-of-00002.gguf").as_str(),
            second.clone(),
            vec![],
        ),
        (
            path("mmproj-model-f16.gguf").as_str(),
            projector.clone(),
            vec![],
        ),
    ])
    .await;

    let listed = |file: &str, body: &[u8]| File {
        path: file.to_owned(),
        bytes: body.len() as u64,
        sha256: None,
    };
    let choices = choices(vec![
        listed("split-Q4_K_M-00001-of-00002.gguf", &first),
        listed("split-Q4_K_M-00002-of-00002.gguf", &second),
        listed("mmproj-model-f16.gguf", &projector),
    ]);
    assert_eq!(choices.len(), 1);
    let folders = Folders {
        download: dir.join("models"),
        scan: vec![],
    };
    let library = Library::open(&folders);
    let item = Item::chosen(&server.base, repo, &choices[0], &library);
    assert_eq!(item.files.len(), 3);

    let queue = queue(&dir);
    let id = queue.enqueue(item.clone());
    assert!(matches!(
        failure(settled(&queue, id).await),
        Failure::EndedEarly { .. }
    ));
    assert!(
        Library::open(&folders).scan().models.is_empty(),
        "no piece is in the library until every piece is whole"
    );

    queue.resume(id);
    assert_eq!(settled(&queue, id).await, Some(State::Done));
    let scan = Library::open(&folders).scan();
    assert_eq!(scan.models.len(), 1, "damaged: {:?}", scan.damaged);
    assert_eq!(scan.models[0].path, item.files[0].destination);
    assert_eq!(
        scan.models[0].companions.len(),
        1,
        "the projector is beside it"
    );
}

// --- the review of #73: transitions that arrive while another is landing ----

#[tokio::test]
async fn a_resume_straight_after_a_pause_is_not_lost() {
    let dir = scratch("pause-resume");
    let body = model();
    let server = serve(vec![("/m.gguf", body.clone(), vec![Act::Slow])]).await;
    let queue = queue(&dir);

    let item = item(server.url("/m.gguf"), &dir, "m.gguf", &body);
    let piece = item.files[0].clone();
    let id = queue.enqueue(item);
    moving(&queue, id).await;
    // Both before the transfer can have noticed the first.
    queue.pause(id);
    queue.resume(id);

    assert_eq!(settled(&queue, id).await, Some(State::Done));
    assert_eq!(std::fs::read(&piece.destination).expect("landed"), body);
}

#[tokio::test]
async fn asking_again_for_a_model_being_cancelled_downloads_it() {
    let dir = scratch("cancel-again");
    let body = model();
    let server = serve(vec![("/m.gguf", body.clone(), vec![Act::Slow])]).await;
    let queue = queue(&dir);

    let item = item(server.url("/m.gguf"), &dir, "m.gguf", &body);
    let piece = item.files[0].clone();
    let id = queue.enqueue(item.clone());
    moving(&queue, id).await;
    queue.cancel(id);
    let again = queue.enqueue(item);

    assert_eq!(again, id, "the same model is the same row");
    assert_eq!(settled(&queue, id).await, Some(State::Done));
    assert_eq!(std::fs::read(&piece.destination).expect("landed"), body);
}

#[tokio::test]
async fn two_quick_asks_for_one_model_are_one_row() {
    let dir = scratch("double-click");
    let body = model();
    let server = serve(vec![("/m.gguf", body.clone(), vec![])]).await;
    let queue = queue(&dir);

    let item = item(server.url("/m.gguf"), &dir, "m.gguf", &body);
    let (first, second) = tokio::join!(async { queue.enqueue(item.clone()) }, async {
        queue.enqueue(item.clone())
    });
    assert_eq!(first, second);
    assert_eq!(queue.rows().len(), 1);
    assert_eq!(settled(&queue, first).await, Some(State::Done));
}

/// Two models that each fit and do not fit together: the second is refused
/// before it starts, not when the disk fills under both.
#[tokio::test]
async fn free_space_counts_what_the_running_items_are_still_to_write() {
    let dir = scratch("room-shared");
    let free = demido_download::room::free(&dir).expect("this volume says");
    let half = free / 2 + free / 8;
    let server = serve(vec![("/a.gguf", Vec::new(), vec![Act::Promise(half)])]).await;
    let queue = Queue::with(Files::in_profile(&dir), 2, Duration::from_secs(30));

    let mut first = item(server.url("/a.gguf"), &dir, "a.gguf", &[]);
    first.files[0].bytes = half;
    let first = queue.enqueue(first);
    moving(&queue, first).await;

    let mut second = item(server.url("/b.gguf"), &dir, "b.gguf", &[]);
    second.files[0].bytes = half;
    let second = queue.enqueue(second);
    assert!(matches!(
        failure(settled(&queue, second).await),
        Failure::NoRoom { .. }
    ));
    assert!(server.requests("/b.gguf").is_empty());
    queue.cancel(first);
}

#[tokio::test]
async fn a_file_already_at_the_destination_is_checked_rather_than_trusted() {
    let dir = scratch("already-there");
    let body = model();
    let server = serve(vec![("/m.gguf", body.clone(), vec![])]).await;
    let queue = queue(&dir);

    let item = item(server.url("/m.gguf"), &dir, "m.gguf", &body);
    let piece = item.files[0].clone();
    std::fs::create_dir_all(piece.destination.parent().unwrap()).unwrap();
    // The right length, and not a model.
    std::fs::write(&piece.destination, vec![b'<'; body.len()]).unwrap();

    let id = queue.enqueue(item.clone());
    assert!(matches!(
        failure(settled(&queue, id).await),
        Failure::Damaged { .. }
    ));
    assert!(server.requests("/m.gguf").is_empty());

    // The damaged file is gone, so the retry fetches the real one.
    queue.resume(id);
    assert_eq!(settled(&queue, id).await, Some(State::Done));
    assert_eq!(std::fs::read(&piece.destination).expect("landed"), body);
}
