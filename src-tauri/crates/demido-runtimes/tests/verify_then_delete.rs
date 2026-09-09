//! Section 2 and section 3 of `docs/rules/runtimes.md`, end to end: real
//! archives, over a real socket, unpacked onto a real disk, with only the
//! verification command standing in.
//!
//! "Deleting the old pin on the strength of a successful unzip is how a user
//! ends up with two broken halves and nothing to run", so the interesting
//! cases here are the refusals: what survives when the replacement does not
//! work.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::collections::HashMap;
use std::future::Future;
use std::io::Write as _;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use demido_runtimes::{Fetchable, Files, Installed, Outcome, RowState, Runtimes};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// One pinned archive, served from `127.0.0.1` instead of from GitHub. The
/// fetcher is the same one a release uses; only the host changes.
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
        .join("demido-runtimes-fetch-tests")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// One file inside an archive: what it is called and what is in it.
type Entry = (&'static str, &'static [u8]);

/// One archive the server offers, under the name the manifest pins it by.
type Offered = (&'static str, &'static [Entry]);

fn zip_of(files: &[Entry]) -> Vec<u8> {
    let mut buffer = std::io::Cursor::new(Vec::new());
    let mut writer = zip::ZipWriter::new(&mut buffer);
    let options = zip::write::SimpleFileOptions::default();
    for (name, contents) in files {
        writer.start_file(*name, options).expect("started an entry");
        writer.write_all(contents).expect("wrote an entry");
    }
    writer.finish().expect("finished the archive");
    buffer.into_inner()
}

/// Serve a fixed set of paths until the test drops the handle.
async fn serve(files: HashMap<String, Vec<u8>>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bound");
    let addr = listener.local_addr().expect("an address");
    let files = Arc::new(files);

    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let files = Arc::clone(&files);
            tokio::spawn(async move {
                let mut buffer = vec![0u8; 4096];
                let read = socket.read(&mut buffer).await.unwrap_or(0);
                let request = String::from_utf8_lossy(&buffer[..read]).to_string();
                let path = request.split_whitespace().nth(1).unwrap_or("/").to_owned();

                let response = match files.get(path.trim_start_matches('/')) {
                    Some(body) => {
                        let mut head = format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        )
                        .into_bytes();
                        head.extend_from_slice(body);
                        head
                    }
                    None => {
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                            .to_vec()
                    }
                };
                let _ = socket.write_all(&response).await;
            });
        }
    });

    format!("http://{addr}")
}

type Verified = Pin<Box<dyn Future<Output = Result<(), String>> + Send>>;

fn passes(_installed: Installed) -> Verified {
    Box::pin(async { Ok(()) })
}

fn refuses(_installed: Installed) -> Verified {
    Box::pin(async { Err("it unpacked and then loaded no model".to_owned()) })
}

struct Rig {
    dir: PathBuf,
    root: String,
    runtimes: Runtimes<Files>,
}

impl Rig {
    async fn new(case: &str, archives: &[Offered], verify: fn(Installed) -> Verified) -> Self {
        let dir = scratch(case);
        let files = archives
            .iter()
            .map(|(name, entries)| ((*name).to_owned(), zip_of(entries)))
            .collect();
        let root = serve(files).await;
        let runtimes = Runtimes::new(Files::in_profile(&dir), dir.join("runtimes"), verify);
        Self {
            dir,
            root,
            runtimes,
        }
    }

    /// The same profile and the same server, with a different answer from
    /// verification. A row's verification does not change in production; this
    /// is how one test covers a fetch that works followed by one that does
    /// not.
    fn verifying(self, verify: fn(Installed) -> Verified) -> Self {
        Self {
            runtimes: Runtimes::new(
                Files::in_profile(&self.dir),
                self.dir.join("runtimes"),
                verify,
            ),
            ..self
        }
    }

    fn served(&self, name: &str) -> Served {
        Served {
            name: name.to_owned(),
            url: format!("{}/{name}", self.root),
        }
    }

    async fn fetch(&self, pin: &str, names: &[&str]) -> Outcome {
        let items: Vec<Served> = names.iter().map(|name| self.served(name)).collect();
        self.runtimes
            .fetch_row(
                "llama.cpp",
                pin,
                &items,
                |_, _| {},
                &demido_runtimes::Cancel::new(),
            )
            .await
            .expect("the fetch machinery itself works")
    }

    fn row_dir(&self, pin: &str) -> PathBuf {
        self.dir.join("runtimes").join(format!("llama.cpp-{pin}"))
    }

    fn state(&self, id: &str) -> RowState {
        self.runtimes
            .read()
            .expect("read")
            .state(id)
            .cloned()
            .unwrap_or(RowState::Absent { reason: None })
    }
}

const BUILD: &str = "llama-b10816-bin-win-cuda-13.3-x64.zip";
const CUDART: &str = "cudart-llama-bin-win-cuda-13.3-x64.zip";

fn required() -> Vec<Offered> {
    vec![
        (BUILD, &[("llama-server.exe", b"a build" as &[u8])]),
        (
            CUDART,
            &[("cublasLt64_13.dll", b"the runtime it links against")],
        ),
    ]
}

/// The pair section 7 calls one row's worth of action: two archives, two
/// licenses, one directory, because `llama-server` resolves the DLL beside
/// itself.
#[tokio::test]
async fn a_build_and_its_companion_land_in_one_directory_and_the_zips_do_not() {
    let rig = Rig::new("verified", &required(), passes).await;

    assert_eq!(
        rig.fetch("b10816", &[BUILD, CUDART]).await,
        Outcome::Verified
    );

    let dir = rig.row_dir("b10816");
    assert!(dir.join("llama-server.exe").exists());
    assert!(dir.join("cublasLt64_13.dll").exists());
    assert!(
        !rig.dir.join("runtimes").join(BUILD).exists(),
        "the archive is this crate's litter once expanded, and is not counted twice"
    );

    match rig.state("llama.cpp") {
        RowState::Managed {
            pin,
            archives,
            on_disk_mib,
        } => {
            assert_eq!(pin, "b10816");
            assert_eq!(archives, vec![BUILD.to_owned(), CUDART.to_owned()]);
            assert!(on_disk_mib > 0.0, "measured from what unpacked");
        }
        other => panic!("expected a managed row, got {other:?}"),
    }
}

/// Section 2: at the first fetch there is nothing to delete, so verification
/// gates the row's state instead. A row that does not verify is absent with a
/// reason, never managed.
#[tokio::test]
async fn a_first_fetch_that_does_not_verify_is_absent_with_a_reason() {
    let rig = Rig::new("first-refused", &required(), refuses).await;

    let outcome = rig.fetch("b10816", &[BUILD, CUDART]).await;

    assert_eq!(
        outcome,
        Outcome::Refused {
            reason: "it unpacked and then loaded no model".to_owned()
        }
    );
    match rig.state("llama.cpp") {
        RowState::Absent {
            reason: Some(reason),
        } => assert!(reason.contains("no model")),
        other => panic!("expected absent with a reason, got {other:?}"),
    }
    assert!(
        !rig.row_dir("b10816").exists(),
        "bytes that never verified are not left looking finished"
    );
}

/// Section 3: the superseded pin goes the moment the new one verifies. Not
/// archived, not kept as a rollback, not moved to a `previous/` folder.
#[tokio::test]
async fn a_verified_replacement_takes_the_predecessor_with_it() {
    let mut archives = required();
    archives.push((
        "llama-b10900-bin-win-cuda-13.3-x64.zip",
        &[("llama-server.exe", b"a newer build" as &[u8])],
    ));
    let rig = Rig::new("replaced", &archives, passes).await;

    rig.fetch("b10816", &[BUILD, CUDART]).await;
    assert!(rig.row_dir("b10816").exists());

    rig.fetch("b10900", &["llama-b10900-bin-win-cuda-13.3-x64.zip"])
        .await;

    assert!(rig.row_dir("b10900").exists());
    assert!(
        !rig.row_dir("b10816").exists(),
        "no predecessor is retained: a rollback is a re-download of a pin the app still knows"
    );
    assert!(
        rig.runtimes.unused().expect("diffed").is_empty(),
        "and it left nothing behind for the Unused row to find"
    );
}

/// The other half of "verify, then delete", and the one that decides whether
/// a user ends up with two broken halves: a replacement that does not work
/// costs them nothing.
#[tokio::test]
async fn a_replacement_that_does_not_verify_leaves_the_working_pin_alone() {
    let mut archives = required();
    archives.push((
        "llama-b10900-bin-win-cuda-13.3-x64.zip",
        &[("llama-server.exe", b"a broken build" as &[u8])],
    ));
    let rig = Rig::new("replacement-refused", &archives, passes).await;
    rig.fetch("b10816", &[BUILD, CUDART]).await;

    // The same profile, now with a verification that refuses: the update is
    // fetched and unpacked exactly as before and then turned away.
    let rig = rig.verifying(refuses);
    let outcome = rig
        .fetch("b10900", &["llama-b10900-bin-win-cuda-13.3-x64.zip"])
        .await;

    assert!(matches!(outcome, Outcome::Refused { .. }));
    assert!(
        rig.row_dir("b10816").join("llama-server.exe").exists(),
        "the pin that already worked keeps working until a replacement verifies"
    );
    assert!(!rig.row_dir("b10900").exists());
    match rig.state("llama.cpp") {
        RowState::Managed { pin, .. } => assert_eq!(pin, "b10816"),
        other => panic!("the row still runs the old pin, got {other:?}"),
    }
}

/// "A failed fetch names which archive failed and offers a retry of that row
/// alone." Naming it is this crate's half; the row's retry button is the
/// page's.
#[tokio::test]
async fn a_failed_fetch_names_the_archive_that_failed() {
    let rig = Rig::new(
        "named-failure",
        &[(BUILD, &[("llama-server.exe", b"a build" as &[u8])])],
        passes,
    )
    .await;

    let items = [rig.served(BUILD), rig.served(CUDART)];
    let error = rig
        .runtimes
        .fetch_row(
            "llama.cpp",
            "b10816",
            &items,
            |_, _| {},
            &demido_runtimes::Cancel::new(),
        )
        .await
        .expect_err("the companion is not on the server");

    let message = error.to_string();
    assert!(
        message.contains(CUDART),
        "the report names the archive a retry would fetch again: {message}"
    );
    assert!(
        !message.contains(BUILD),
        "and does not blame the one that arrived: {message}"
    );
}

/// Nothing outside the profile's own runtimes folder is ever read or written,
/// which is the ledger half of "Demido prunes no cache it did not fill".
#[tokio::test]
async fn everything_a_fetch_writes_is_inside_the_profile() {
    let rig = Rig::new("contained", &required(), passes).await;
    rig.fetch("b10816", &[BUILD, CUDART]).await;

    let profile: Vec<String> = std::fs::read_dir(&rig.dir)
        .expect("read the profile")
        .map(|entry| entry.expect("entry").file_name().to_string_lossy().into())
        .collect();
    let mut sorted = profile.clone();
    sorted.sort();
    assert_eq!(
        sorted,
        vec!["runtimes".to_owned(), "runtimes.json".to_owned()],
        "one folder of bytes and one ledger, and nothing anywhere else"
    );
}
