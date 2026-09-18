//! The index, against payloads Hugging Face really sent and a local server
//! that answers the way it can.
//!
//! The parsers are pure functions over bytes (#70), so a repository whose file
//! list confuses the browser is reproduced here by committing the payload under
//! `tests/fixtures/index/`, captured keyless with `curl` on 2026-09-18 and
//! never edited. A repository changing its layout upstream is then a new
//! fixture and a failing test rather than a mystery.
//!
//! The requests go to `127.0.0.1` rather than through a seam: the index is one
//! host and gets no trait (#36), so what is tested is the one implementation,
//! pointed somewhere else.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use demido_models::index::{self, Answer, Cause, Index, Repo, LISTING};
use demido_models::Fact;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

fn fixture(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/index")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn listing(name: &str) -> Vec<Repo> {
    index::parse_listing(&fixture(name)).expect("a real listing parses")
}

fn paths(files: &[index::File]) -> Vec<&str> {
    files.iter().map(|file| file.path.as_str()).collect()
}

// --- the listing -------------------------------------------------------------

#[test]
fn a_search_reads_every_repository_it_was_sent() {
    let repos = listing("search-gemma-3-4b-it-qat.json");
    assert_eq!(repos.len(), 30);
    assert_eq!(repos[0].id, "unsloth/gemma-3-4b-it-qat-GGUF");
    assert_eq!(repos[0].author.as_deref(), Some("unsloth"));
    assert_eq!(repos[0].downloads, 10_168);
    assert_eq!(repos[0].likes, 33);
}

/// Most downloaded first. Not alphabetical, and not whatever order the server
/// happened to use: a listing sent in some other order is put in this one.
#[test]
fn a_listing_is_ordered_by_downloads() {
    for name in ["search-gemma-3-4b-it-qat.json", "search-front-page.json"] {
        let repos = listing(name);
        assert!(
            repos
                .windows(2)
                .all(|pair| pair[0].downloads >= pair[1].downloads),
            "{name}"
        );
    }

    let shuffled = br#"[
        {"id":"a/least","downloads":3},
        {"id":"z/most","downloads":900},
        {"id":"m/middle","downloads":40}
    ]"#;
    let ids: Vec<String> = index::parse_listing(shuffled)
        .expect("parses")
        .into_iter()
        .map(|repo| repo.id)
        .collect();
    assert_eq!(ids, ["z/most", "m/middle", "a/least"]);
}

/// A server that ignores `limit` does not flood the browser.
#[test]
fn a_listing_is_capped_at_a_listing_s_worth() {
    let many: Vec<String> = (0..LISTING + 12)
        .map(|n| format!(r#"{{"id":"someone/model-{n}","downloads":{n}}}"#))
        .collect();
    let body = format!("[{}]", many.join(","));
    let repos = index::parse_listing(body.as_bytes()).expect("parses");
    assert_eq!(repos.len(), LISTING);
    assert_eq!(
        repos[0].downloads,
        (LISTING + 11) as u64,
        "the most downloaded are kept"
    );
}

/// Gating is `false`, `"auto"` or `"manual"`: a boolean when it is off and a
/// string when it is on. Only the first shape is open.
#[test]
fn a_gated_repository_is_identified_at_listing_time() {
    let repos = listing("search-gemma-3-4b-it-qat.json");
    let gated: Vec<&str> = repos
        .iter()
        .filter(|repo| repo.gated)
        .map(|repo| repo.id.as_str())
        .collect();
    assert_eq!(gated, ["google/gemma-3-4b-it-qat-q4_0-gguf"]);

    let shapes = br#"[
        {"id":"a/auto","gated":"auto"},
        {"id":"b/open","gated":false},
        {"id":"c/unsaid"}
    ]"#;
    let repos = index::parse_listing(shapes).expect("parses");
    let gated = |id: &str| repos.iter().find(|repo| repo.id == id).expect(id).gated;
    assert!(gated("a/auto"));
    assert!(!gated("b/open"));
    assert!(!gated("c/unsaid"), "an absent field is not a gate");
}

/// Hugging Face's own fields, exactly as sent: every tag, in its order,
/// including the ones a person would never read.
#[test]
fn publisher_fields_are_passed_through_verbatim() {
    let raw: serde_json::Value =
        serde_json::from_slice(&fixture("search-front-page.json")).expect("json");
    let raw = raw.as_array().expect("an array");

    for repo in listing("search-front-page.json") {
        let sent = raw
            .iter()
            .find(|item| item["id"] == repo.id.as_str())
            .expect("every repository came from the payload");
        let tags: Vec<&str> = sent["tags"]
            .as_array()
            .map(|tags| tags.iter().filter_map(|tag| tag.as_str()).collect())
            .unwrap_or_default();
        assert_eq!(repo.tags, tags, "{}", repo.id);
        assert_eq!(
            repo.pipeline.as_deref(),
            sent["pipeline_tag"].as_str(),
            "{}",
            repo.id
        );
        assert_eq!(
            repo.library.as_deref(),
            sent["library_name"].as_str(),
            "{}",
            repo.id
        );
    }
}

/// A repository crosses to the window with these keys and no others. The
/// capabilities cross as `stated`, which is a statement the publisher made or
/// nothing: an absent option is left out, never replaced by something else.
#[test]
fn a_repository_crosses_with_the_publisher_s_fields_and_nothing_else() {
    for name in [
        "search-gemma-3-4b-it-qat.json",
        "search-gemma-4-e4b-expanded.json",
    ] {
        for repo in listing(name) {
            let value = serde_json::to_value(&repo).expect("serialises");
            let mut keys: Vec<&str> = value
                .as_object()
                .expect("an object")
                .keys()
                .map(String::as_str)
                .collect();
            keys.sort_unstable();
            let expected = [
                "architecture",
                "author",
                "context",
                "downloads",
                "gated",
                "id",
                "library",
                "likes",
                "params",
                "pipeline",
                "stated",
                "tags",
            ];
            assert!(
                keys.iter().all(|key| expected.contains(key)),
                "{}: {keys:?}",
                repo.id
            );
        }
    }
}

// --- what the publisher states ---------------------------------------------

/// Params, architecture and trained context, as the Hub reads them out of the
/// GGUF the publisher uploaded. Nothing is computed from a name.
#[test]
fn a_repository_carries_the_facts_its_publisher_s_file_states() {
    let repos = listing("search-gemma-4-e4b-expanded.json");
    let repo = repos
        .iter()
        .find(|repo| repo.id == "ggml-org/gemma-4-E4B-it-GGUF")
        .expect("in the payload");
    assert_eq!(repo.params, Some(7_518_069_290));
    assert_eq!(repo.architecture.as_deref(), Some("gemma4"));
    assert_eq!(repo.context, Some(131_072));

    let unsaid =
        index::parse_listing(br#"[{"id":"a/none"},{"id":"b/empty","gguf":{}}]"#).expect("parses");
    for repo in unsaid {
        assert_eq!(repo.params, None, "{}", repo.id);
        assert_eq!(repo.architecture, None, "{}", repo.id);
        assert_eq!(repo.context, None, "{}", repo.id);
    }
}

/// `design/shell.md` draws capabilities as tags; v2 refused to badge a
/// repository it could not verify. Both hold (#75): a repository in the index
/// carries what its publisher states in its pipeline or its tags, and anything
/// unstated is `Unknown`. The index never says `No`, because nothing in a
/// listing can state an absence.
#[test]
fn a_capability_is_what_the_publisher_states_and_nothing_else() {
    let repos = listing("search-gemma-4-e4b-expanded.json");
    let stated = |id: &str| repos.iter().find(|repo| repo.id == id).expect(id).stated;

    // Pipeline `image-text-to-text`, tags `vision` and `audio`.
    let both = stated("HauhauCS/Gemma-4-E4B-Uncensored-HauhauCS-Aggressive");
    assert_eq!(both.vision, Fact::Yes);
    assert_eq!(both.audio, Fact::Yes);
    assert_eq!(both.tools, Fact::Unknown);
    assert_eq!(both.reasoning, Fact::Unknown);

    // A tag says tool use, and nothing says vision.
    let tools = stated("shafire/Zero-Gemma4-E4B-OpenZero-GGUF");
    assert_eq!(tools.tools, Fact::Yes);
    assert_eq!(tools.vision, Fact::Unknown);

    // `audio-text-to-text` as a tag, beside a `text-generation` pipeline.
    let audio = stated("bartowski/huihui-ai_Huihui-gemma-3n-E4B-it-abliterated-GGUF");
    assert_eq!(audio.audio, Fact::Yes);

    // `any-to-any` states no particular input, so it states nothing here.
    let any = stated("ggml-org/gemma-4-E4B-it-GGUF");
    assert_eq!(
        [any.vision, any.tools, any.reasoning, any.audio],
        [Fact::Unknown; 4]
    );

    let reasoning =
        index::parse_listing(br#"[{"id":"a/think","tags":["thinking"]}]"#).expect("parses");
    assert_eq!(reasoning[0].stated.reasoning, Fact::Yes);

    for name in ["search-gemma-3-4b-it-qat.json", "search-front-page.json"] {
        for repo in listing(name).into_iter().chain(repos.clone()) {
            let facts = [
                repo.stated.vision,
                repo.stated.tools,
                repo.stated.reasoning,
                repo.stated.audio,
            ];
            assert!(!facts.contains(&Fact::No), "{}: {facts:?}", repo.id);
        }
    }
}

#[test]
fn a_listing_that_is_not_one_is_malformed_rather_than_empty() {
    for body in [
        &b"<html>rate limited</html>"[..],
        b"{\"error\":\"Invalid username or password.\"}",
        b"",
    ] {
        assert!(index::parse_listing(body).is_err(), "{body:?}");
    }
    assert_eq!(index::parse_listing(b"[]").expect("parses"), []);
}

// --- the files ---------------------------------------------------------------

/// A README, a config and an importance matrix sit beside the weights. Only a
/// `.gguf` is something llama.cpp can load, so only a `.gguf` is listed.
#[test]
fn a_file_listing_keeps_the_gguf_files_and_nothing_else() {
    let files = index::parse_files(&fixture("tree-Qwen--Qwen3-0.6B-GGUF.json")).expect("parses");
    assert_eq!(
        paths(&files),
        ["Qwen3-0.6B-Q8_0.gguf"],
        "the one GGUF, without .gitattributes, README.md, LICENSE or params"
    );
}

/// The size is the one the server sends for the object, not the size of the
/// pointer file in Git, which is about 135 bytes and would make a gigabyte
/// look instant.
#[test]
fn a_file_s_size_and_digest_are_the_ones_the_server_sends() {
    let files =
        index::parse_files(&fixture("tree-ggml-org--gemma-3-4b-it-GGUF.json")).expect("parses");
    let q4 = files
        .iter()
        .find(|file| file.path == "gemma-3-4b-it-Q4_K_M.gguf")
        .expect("listed");
    assert_eq!(q4.bytes, 2_489_757_856);
    assert_eq!(
        q4.sha256.as_deref(),
        Some("882e8d2db44dc554fb0ea5077cb7e4bc49e7342a1f0da57901c0802ea21a0863")
    );
}

/// The projector is a `.gguf` like the weights and is listed like them. Which
/// file is a companion of which is #71's, read off this list.
#[test]
fn a_repository_with_a_projector_lists_it_beside_the_weights() {
    let files =
        index::parse_files(&fixture("tree-ggml-org--gemma-3-4b-it-GGUF.json")).expect("parses");
    assert_eq!(
        paths(&files),
        [
            "gemma-3-4b-it-Q4_K_M.gguf",
            "gemma-3-4b-it-Q8_0.gguf",
            "gemma-3-4b-it-f16.gguf",
            "mmproj-model-f16.gguf",
        ]
    );
}

/// Large models are published one quantisation per directory, each in pieces.
/// Every piece is listed with its whole path, because the path is the URL.
#[test]
fn a_split_model_lists_every_piece_with_its_directory() {
    let files =
        index::parse_files(&fixture("tree-unsloth--gpt-oss-120b-GGUF.json")).expect("parses");
    let q4: Vec<&str> = paths(&files)
        .into_iter()
        .filter(|path| path.starts_with("Q4_K_M/"))
        .collect();
    assert_eq!(
        q4,
        [
            "Q4_K_M/gpt-oss-120b-Q4_K_M-00001-of-00002.gguf",
            "Q4_K_M/gpt-oss-120b-Q4_K_M-00002-of-00002.gguf",
        ]
    );
    assert!(
        files.iter().all(|file| file.bytes > 1_000_000),
        "no directory entry and no pointer size"
    );
}

/// `UD-Q4_K_XL` is nobody's published quantisation. The listing keeps the name
/// exactly as written; what it means, and where it sorts, is #71's.
#[test]
fn an_unpublished_quantisation_label_survives_exactly_as_written() {
    let files = index::parse_files(&fixture("tree-unsloth--Qwen3-0.6B-GGUF.json")).expect("parses");
    for path in [
        "Qwen3-0.6B-UD-Q4_K_XL.gguf",
        "Qwen3-0.6B-UD-IQ1_M.gguf",
        "Qwen3-0.6B-Q2_K_L.gguf",
    ] {
        assert!(paths(&files).contains(&path), "{path}");
    }
    assert!(
        !paths(&files).iter().any(|path| path.ends_with(".dat")),
        "the importance matrix is stored in LFS and is still not a model"
    );
}

/// A gated repository answers the listing and masks the digest. A mask is not
/// a digest, so the file has none rather than a string of asterisks the queue
/// would verify against and fail.
#[test]
fn a_gated_repository_s_masked_digest_is_no_digest() {
    let files = index::parse_files(&fixture("tree-google--gemma-3-4b-it-qat-q4_0-gguf.json"))
        .expect("parses");
    assert_eq!(
        paths(&files),
        ["gemma-3-4b-it-q4_0.gguf", "mmproj-model-f16-4B.gguf"]
    );
    assert!(files.iter().all(|file| file.sha256.is_none()));
    assert_eq!(files[0].bytes, 3_155_051_328);
}

#[test]
fn a_file_listing_that_is_not_one_is_malformed_rather_than_empty() {
    assert!(index::parse_files(b"not json").is_err());
    assert!(index::parse_files(b"{\"error\":\"Invalid username or password.\"}").is_err());
}

// --- over the wire -----------------------------------------------------------

/// What one request looked like when it arrived.
struct Arrived {
    line: String,
    headers: Vec<(String, String)>,
}

impl Arrived {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

/// One answer the server gives: a status, any extra header lines (`{host}`
/// stands for the server's own address), and a body.
struct Reply {
    status: &'static str,
    headers: String,
    body: Vec<u8>,
}

/// Answer one connection per reply, in order, and hand back what was asked.
async fn serve(replies: Vec<Reply>) -> (String, tokio::task::JoinHandle<Vec<Arrived>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bound");
    let host = format!("http://{}", listener.local_addr().expect("addr"));
    let at = host.clone();
    let served = tokio::spawn(async move {
        let mut arrived = Vec::new();
        for reply in replies {
            let (mut socket, _) = listener.accept().await.expect("accepted");
            let mut buf = vec![0u8; 16 * 1024];
            let n = socket.read(&mut buf).await.expect("read the request");
            let request = String::from_utf8_lossy(&buf[..n]).into_owned();
            let mut lines = request.lines();
            let line = lines.next().unwrap_or_default().to_owned();
            let headers = lines
                .take_while(|line| !line.is_empty())
                .filter_map(|line| line.split_once(':'))
                .map(|(key, value)| (key.trim().to_owned(), value.trim().to_owned()))
                .collect();
            let head = format!(
                "HTTP/1.1 {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{}Connection: close\r\n\r\n",
                reply.status,
                reply.body.len(),
                reply.headers.replace("{host}", &at),
            );
            socket.write_all(head.as_bytes()).await.expect("wrote head");
            socket.write_all(&reply.body).await.expect("wrote body");
            arrived.push(Arrived { line, headers });
        }
        arrived
    });
    (host, served)
}

/// Answer one request with `status` and `body`, and hand back what was asked.
async fn serve_once(
    status: &'static str,
    body: Vec<u8>,
) -> (String, tokio::task::JoinHandle<Arrived>) {
    let (host, served) = serve(vec![Reply {
        status,
        headers: String::new(),
        body,
    }])
    .await;
    let one =
        tokio::spawn(async move { served.await.expect("served").pop().expect("one request") });
    (host, one)
}

fn found<T: std::fmt::Debug>(answer: Answer<T>) -> T {
    match answer {
        Answer::Read { found } => found,
        other => panic!("expected a reading, got {other:?}"),
    }
}

fn cause<T: std::fmt::Debug>(answer: &Answer<T>) -> Cause {
    match answer {
        Answer::Unreadable { cause, .. } => *cause,
        other => panic!("expected a stated condition, got {other:?}"),
    }
}

#[tokio::test]
async fn the_request_identifies_the_app_and_carries_no_key() {
    let (host, served) = serve_once("200 OK", fixture("search-gemma-3-4b-it-qat.json")).await;
    let repos = found(Index::at(&host).search("gemma 3/qat").await);
    assert_eq!(repos.len(), 30);

    let arrived = served.await.expect("served");
    let agent = arrived.header("user-agent").expect("a user agent");
    assert!(agent.starts_with("demido-studio/"), "{agent}");
    assert!(
        agent.contains("github.com/elpideus/demido-studio"),
        "{agent}"
    );
    assert_eq!(arrived.header("authorization"), None, "keyless");
    assert_eq!(arrived.header("cookie"), None, "keyless");

    let line = &arrived.line;
    assert!(line.starts_with("GET /api/models?"), "{line}");
    for part in [
        "filter=gguf",
        "sort=downloads",
        "direction=-1",
        &format!("limit={LISTING}"),
        "search=gemma+3%2Fqat",
        "expand[]=gated",
        "expand[]=gguf",
    ] {
        assert!(line.contains(part), "{part} in {line}");
    }
}

/// An empty query is the front page, not a search for nothing.
#[tokio::test]
async fn an_empty_query_asks_for_the_front_page() {
    let (host, served) = serve_once("200 OK", fixture("search-front-page.json")).await;
    found(Index::at(&host).search("   ").await);
    let arrived = served.await.expect("served");
    assert!(!arrived.line.contains("search="), "{}", arrived.line);
}

#[tokio::test]
async fn a_repository_s_files_are_asked_for_by_its_id() {
    let (host, served) =
        serve_once("200 OK", fixture("tree-ggml-org--gemma-3-4b-it-GGUF.json")).await;
    let files = found(Index::at(&host).files("ggml-org/gemma-3-4b-it-GGUF").await);
    assert_eq!(files.len(), 4);
    let arrived = served.await.expect("served");
    assert!(
        arrived
            .line
            .starts_with("GET /api/models/ggml-org/gemma-3-4b-it-GGUF/tree/main?recursive=true "),
        "{}",
        arrived.line
    );
}

/// The tree is paged. A repository of split quantisations can run past one
/// page, and a list cut short would be offered as the whole repository.
#[tokio::test]
async fn every_page_of_a_file_list_is_read() {
    let first = br#"[{"type":"file","path":"Q4_K_M/m-Q4_K_M-00001-of-00002.gguf","size":10}]"#;
    let second = br#"[{"type":"file","path":"Q4_K_M/m-Q4_K_M-00002-of-00002.gguf","size":20}]"#;
    let (host, served) = serve(vec![
        Reply {
            status: "200 OK",
            headers: "Link: <{host}/api/models/a/m/tree/main?recursive=true&cursor=Zz%3D>; rel=\"next\"\r\n".into(),
            body: first.to_vec(),
        },
        Reply {
            status: "200 OK",
            headers: String::new(),
            body: second.to_vec(),
        },
    ])
    .await;

    let files = found(Index::at(&host).files("a/m").await);
    assert_eq!(
        paths(&files),
        [
            "Q4_K_M/m-Q4_K_M-00001-of-00002.gguf",
            "Q4_K_M/m-Q4_K_M-00002-of-00002.gguf",
        ]
    );
    let arrived = served.await.expect("served");
    assert!(
        arrived[1].line.contains("cursor=Zz%3D"),
        "{}",
        arrived[1].line
    );
}

/// A `Link` to some other host is not where this repository's files are.
#[tokio::test]
async fn a_next_page_elsewhere_is_not_followed() {
    let (host, _served) = serve(vec![Reply {
        status: "200 OK",
        headers: "Link: <http://elsewhere.invalid/more>; rel=\"next\"\r\n".into(),
        body: fixture("tree-ggml-org--gemma-3-4b-it-GGUF.json"),
    }])
    .await;
    let files = found(Index::at(&host).files("ggml-org/gemma-3-4b-it-GGUF").await);
    assert_eq!(files.len(), 4);
}

/// Nothing listening is the machine offline, and it is an answer, not an
/// error: the caller still has its own library to show beside it.
#[tokio::test]
async fn an_unreachable_index_is_a_stated_condition() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bound");
    let host = format!("http://{}", listener.local_addr().expect("addr"));
    drop(listener);

    let answer = Index::at(&host).search("qwen").await;
    assert_eq!(cause(&answer), Cause::Offline);
    let Answer::Unreadable { reason, .. } = answer else {
        unreachable!()
    };
    assert!(!reason.is_empty());
}

#[tokio::test]
async fn each_refusal_says_what_actually_happened() {
    let cases: [(&str, &[u8], Cause); 5] = [
        (
            "429 Too Many Requests",
            b"{\"error\":\"slow down\"}",
            Cause::RateLimited,
        ),
        // What Hugging Face really sends, keyless, for a repository that is
        // private or does not exist: not a 404.
        (
            "401 Unauthorized",
            b"{\"error\":\"Invalid username or password.\"}",
            Cause::Missing,
        ),
        (
            "404 Not Found",
            b"{\"error\":\"Repository not found\"}",
            Cause::Missing,
        ),
        // Somebody refusing, a proxy or a block, is not a missing repository.
        ("403 Forbidden", b"blocked", Cause::Refused),
        ("503 Service Unavailable", b"upstream down", Cause::Refused),
    ];
    for (status, body, expected) in cases {
        let (host, _served) = serve_once(status, body.to_vec()).await;
        let answer = Index::at(&host).files("someone/private").await;
        assert_eq!(cause(&answer), expected, "{status}");
    }
}

#[tokio::test]
async fn a_success_that_is_not_json_is_malformed() {
    let (host, _served) = serve_once("200 OK", b"<html>a captive portal</html>".to_vec()).await;
    let answer = Index::at(&host).search("qwen").await;
    assert_eq!(cause(&answer), Cause::Malformed);
}

/// What the window receives: a tag it can switch on, never an `Err` that
/// would make the query fail and take the pane down with it.
#[tokio::test]
async fn an_answer_crosses_to_the_window_as_a_tagged_state() {
    let (host, _served) = serve_once("429 Too Many Requests", b"{}".to_vec()).await;
    let answer = Index::at(&host).search("qwen").await;
    let value = serde_json::to_value(&answer).expect("serialises");
    assert_eq!(value["state"], "unreadable");
    assert_eq!(value["cause"], "rate-limited");
    assert!(value["reason"]
        .as_str()
        .is_some_and(|reason| !reason.is_empty()));

    let read: Answer<Vec<Repo>> = Answer::Read { found: Vec::new() };
    let value = serde_json::to_value(&read).expect("serialises");
    assert_eq!(value["state"], "read");
    assert_eq!(value["found"], serde_json::json!([]));
}
