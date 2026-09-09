//! The fetcher's resume and cancel behaviour, against a real (if tiny and
//! local) HTTP server. `docs/rules/runtimes.md`'s own text says the fetcher
//! gets no seam for HTTP: this is that decision honoured rather than routed
//! around. There is exactly one fetch implementation, `demido_runtimes::fetch`,
//! it is just pointed at `127.0.0.1` here instead of GitHub.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::path::PathBuf;

use demido_runtimes::fetch::{fetch, Fetchable};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const BODY: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz-the-rest-of-a-tiny-fixture-archive";

struct Served {
    name: String,
    url: String,
}

impl Fetchable for Served {
    fn name(&self) -> &str {
        &self.name
    }
    fn url(&self) -> String {
        self.url.clone()
    }
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join("demido-runtimes-resume-tests")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// Handle one connection: read the request line and headers, honour a
/// `Range` header if present, write however many bytes of `body` the caller
/// asked this handler to send before it stops (simulating a connection that
/// dies partway through).
async fn serve_one(listener: &TcpListener, body: &'static [u8], send_at_most: usize) {
    let (mut socket, _) = listener.accept().await.expect("accepted");
    let mut buf = vec![0u8; 4096];
    let n = socket.read(&mut buf).await.expect("read the request");
    let request = String::from_utf8_lossy(&buf[..n]);

    let range = request
        .lines()
        .find(|l| l.to_ascii_lowercase().starts_with("range:"))
        .and_then(|l| l.split("bytes=").nth(1))
        .and_then(|r| r.trim_end_matches('-').trim().parse::<usize>().ok());

    let (status, start) = match range {
        Some(start) => ("206 Partial Content", start),
        None => ("200 OK", 0),
    };
    let remaining = &body[start..];
    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        remaining.len()
    );
    socket
        .write_all(header.as_bytes())
        .await
        .expect("wrote header");
    let to_send = &remaining[..remaining.len().min(send_at_most)];
    socket.write_all(to_send).await.expect("wrote body");
    // Dropping the socket here (without sending the rest, when send_at_most
    // is short) is the "connection died mid archive" this suite exercises.
}

#[tokio::test]
async fn a_connection_that_dies_partway_resumes_from_where_it_stopped() {
    let dir = scratch("resume");
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bound");
    let addr = listener.local_addr().expect("addr");
    let url = format!("http://{addr}/archive.bin");
    let item = Served {
        name: "archive.bin".into(),
        url,
    };

    let client = reqwest::Client::new();
    let cancel = tokio_util::sync::CancellationToken::new();

    // First attempt: the server sends only the first 20 bytes, then the
    // connection drops. reqwest surfaces that as a stream error partway
    // through, which is exactly a fetch failing mid archive.
    let server = tokio::spawn({
        let body = BODY;
        async move {
            serve_one(&listener, body, 20).await;
            listener
        }
    });
    let first = fetch(&item, &dir, &client, |_| {}, &cancel).await;
    assert!(
        first.is_err(),
        "a truncated response is a failure, not a silent success"
    );
    let listener = server.await.expect("server task");

    let part = dir.join("archive.bin.part");
    assert!(part.exists(), "the partial file is the resume state");
    let partial_len = std::fs::metadata(&part).expect("stat").len() as usize;
    assert!(partial_len > 0 && partial_len < BODY.len());

    // Second attempt: same file, and this time the server sees the Range
    // header and answers 206, sending the rest.
    let server = tokio::spawn(async move {
        serve_one(&listener, BODY, BODY.len()).await;
    });
    let second = fetch(&item, &dir, &client, |_| {}, &cancel)
        .await
        .expect("resumed to completion");
    server.await.expect("server task");

    let bytes = std::fs::read(&second).expect("read the finished file");
    assert_eq!(
        bytes, BODY,
        "resume did not just append the tail, it produced the whole file"
    );
    assert!(
        !part.exists(),
        "the .part file is gone once the fetch completes"
    );
}

#[tokio::test]
async fn cancelling_leaves_the_partial_file_on_disk() {
    let dir = scratch("cancel");
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bound");
    let addr = listener.local_addr().expect("addr");
    let url = format!("http://{addr}/archive.bin");
    let item = Served {
        name: "archive.bin".into(),
        url,
    };

    let client = reqwest::Client::new();
    let cancel = tokio_util::sync::CancellationToken::new();
    let cancel_from_inside = cancel.clone();

    let server = tokio::spawn({
        let body = BODY;
        async move {
            serve_one(&listener, body, body.len()).await;
        }
    });

    // Cancel as soon as the first byte of progress is reported, well before
    // the small fixture body could finish.
    let result = fetch(
        &item,
        &dir,
        &client,
        move |_progress| cancel_from_inside.cancel(),
        &cancel,
    )
    .await;
    server.await.expect("server task");

    assert!(
        matches!(result, Err(e) if matches!(e, demido_runtimes::fetch::FetchError::Cancelled { .. }))
    );
    assert!(
        dir.join("archive.bin.part").exists() || dir.join("archive.bin").exists(),
        "cancelling never deletes what was already on disk"
    );
}
