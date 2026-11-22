//! Finding and downloading models, against a stand-in Hugging Face on this computer.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use sha2::{Digest, Sha256};

use super::*;
use crate::db::Db;

/// A machine like the owner's: a 12 GB card, 32 GB of memory, 0.9 GB kept free, 8,192 tokens.
const MACHINE: Machine = Machine { card_mb: 12 * 1024, ram_mb: 32 * 1024, margin_mb: 922, context: 8192 };

#[test]
fn a_files_name_says_its_quantization_and_its_shards_are_one() {
    assert_eq!(quant_of("Qwen3-4B-Q4_K_M.gguf"), Some("Q4_K_M"));
    assert_eq!(quant_of("gemma-3-4b-it-f16.gguf"), Some("F16"));
    assert_eq!(quant_of("Qwen3.8-27B-UD-Q4_K_XL.gguf"), Some("Q4_K_XL"));
    assert_eq!(quant_of("model.IQ4_XS.gguf"), Some("IQ4_XS"));
    assert_eq!(quant_of("Q8_0/Llama-70B-Q8_0-00001-of-00002.gguf"), Some("Q8_0"));
    assert_eq!(quant_of("tinystories.gguf"), None);
    assert_eq!(quant_of("SmolLM2-135M-Instruct-Q4_0_8_8.gguf"), Some("Q4_0_8_8"));
    assert_eq!(without_shard("Q8_0/Llama-70B-Q8_0-00002-of-00002.gguf"), "Q8_0/Llama-70B-Q8_0.gguf");
    assert_eq!(without_shard("Qwen3-4B-Q4_K_M.gguf"), "Qwen3-4B-Q4_K_M.gguf");
    assert_eq!(without_shard("model-v2-of-3.gguf"), "model-v2-of-3.gguf", "only numbers make a shard");
    assert!(rank_of("Q8_0") < rank_of("Q6_K") && rank_of("Q6_K") < rank_of("Q4_K_M") && rank_of("Q4_K_M") < rank_of("Q2_K") && rank_of("Q2_K") < rank_of("WHATEVER"));
    assert!(is_repo("Qwen/Qwen3-4B-GGUF") && is_repo("ggml-org/gemma-3-4b-it-GGUF"));
    for bad in ["Qwen", "a/b/c", "../etc", "a/..", "a/b c", "a/b?x=1", ""] {
        assert!(!is_repo(bad), "{bad}");
    }
}

#[test]
fn what_a_model_needs_is_its_file_its_cache_and_a_little_and_it_fits_or_it_does_not() {
    // Qwen3 4B in Q4_K_M: 2.4 GB of file, about 1.2 GB of cache at 8,192 tokens.
    let qwen = needs_mb(2_497_280_256, Some(4_022_468_096), 8192);
    assert!((3_800..=4_000).contains(&qwen), "{qwen}");
    assert_eq!(fits(qwen, &MACHINE), "fits");
    // Twice the context, twice the cache.
    assert!(needs_mb(2_497_280_256, Some(4_022_468_096), 16384) > qwen + 1_100);
    // A 27B model in Q4: on the card with the margin it does not fit; partly on the processor it does.
    let big = needs_mb(16_500_000_000, Some(27_000_000_000), 8192);
    assert_eq!(fits(big, &MACHINE), "slow");
    assert_eq!(fits(needs_mb(140_000_000_000, Some(235_000_000_000), 8192), &MACHINE), "no");
    // Without a card, everything runs on the processor, if it fits at all.
    let no_card = Machine { card_mb: 0, ..MACHINE };
    assert_eq!(fits(qwen, &no_card), "slow");
}

fn sibling(path: &str, size: u64) -> Value {
    json!({ "rfilename": path, "size": size, "lfs": { "sha256": format!("{:064x}", size), "size": size } })
}

#[test]
fn files_are_grouped_into_quantizations_and_the_best_that_fits_is_recommended() {
    let gb = 1_000_000_000u64;
    let siblings = vec![
        sibling("README.md", 100),
        sibling("gemma-3-4b-it-Q4_K_M.gguf", 2_489_757_856),
        sibling("gemma-3-4b-it-Q8_0.gguf", 4_130_226_336),
        sibling("gemma-3-4b-it-f16.gguf", 7_767_474_336),
        sibling("mmproj-model-f16.gguf", 851_251_104),
        sibling("mmproj-model-f32.gguf", 1_700_000_000),
    ];
    let (quants, vision, others) = quants_from(&siblings, Some(3_880_099_328), &MACHINE, &|name| name == "gemma-3-4b-it-Q8_0");
    assert!(others.is_empty());
    assert_eq!(vision.as_ref().map(|v| v.path.as_str()), Some("mmproj-model-f16.gguf"), "the vision part in half precision");
    assert_eq!(quants.iter().map(|q| q.name.as_str()).collect::<Vec<_>>(), ["F16", "Q8_0", "Q4_K_M"], "the best quality first");
    let q8 = &quants[1];
    assert!(q8.recommended, "Q8_0 fits, and an unquantized file is not recommended over it: {quants:#?}");
    assert_eq!((q8.local_id.as_str(), q8.on_disk), ("gemma-3-4b-it-Q8_0", true), "with a vision part it becomes a folder");
    assert_eq!(q8.size, 4_130_226_336 + 851_251_104, "the vision part is counted with every quantization");
    assert!(!quants[0].recommended && !quants[2].recommended);

    // Shards are one quantization; when nothing fits on the card, the smallest that runs at all.
    let siblings = vec![sibling("Q4_K_M/big-Q4_K_M-00001-of-00002.gguf", 9 * gb), sibling("Q4_K_M/big-Q4_K_M-00002-of-00002.gguf", 8 * gb), sibling("big-Q2_K.gguf", 11 * gb)];
    let (quants, vision, _) = quants_from(&siblings, Some(30 * gb), &MACHINE, &|_| false);
    assert!(vision.is_none());
    assert_eq!(quants.iter().map(|q| (q.name.as_str(), q.files.len(), q.fits.as_str())).collect::<Vec<_>>(), [("Q4_K_M", 2, "slow"), ("Q2_K", 1, "slow")]);
    assert_eq!(quants[0].local_id, "big-Q4_K_M", "shards become a folder named after the model");
    assert!(quants[1].recommended && !quants[0].recommended, "the smaller one, when neither fits on the card");

    // What a real repository held (unsloth/Inkling-2-Mini-GGUF, 26B): the model in many sizes, and in a folder
    // of its own a 1 GB draft model in Q8_0 and BF16, and calibration data. Only the model is offered.
    let siblings = vec![
        sibling("Inkling-2-Mini-UD-Q4_K_M.gguf", 16_100_000_000),
        sibling("Inkling-2-Mini-UD-IQ1_S.gguf", 7_800_000_000),
        sibling("dspark/dspark-Inkling-2-Mini-Q8_0.gguf", 996_000_000),
        sibling("dspark/dspark-Inkling-2-Mini-BF16.gguf", 1_200_000_000),
        sibling("imatrix_unsloth.gguf", 5_000_000),
        sibling("mmproj-BF16.gguf", 612_000_000),
    ];
    let (quants, _, others) = quants_from(&siblings, Some(26_100_000_000), &MACHINE, &|_| false);
    assert_eq!(quants.iter().map(|q| q.name.as_str()).collect::<Vec<_>>(), ["Q4_K_M", "IQ1_S"]);
    assert_eq!(others.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(), ["imatrix_unsloth.gguf", "dspark/dspark-Inkling-2-Mini-Q8_0.gguf", "dspark/dspark-Inkling-2-Mini-BF16.gguf"]);
    assert!(quants.iter().all(|q| !q.recommended || q.fits != "fits"), "neither fits on a 12 GB card: {quants:#?}");
}

#[test]
fn what_a_model_does_is_read_from_its_own_files() {
    let item = json!({
        "id": "ggml-org/gemma-3-4b-it-GGUF", "author": "ggml-org", "downloads": 1000, "likes": 50, "trendingScore": 12.5, "gated": false,
        "pipeline_tag": "image-text-to-text", "tags": ["gguf"], "cardData": { "license": "gemma" },
        "gguf": { "total": 3_880_099_328u64, "architecture": "gemma3", "context_length": 131072, "chat_template": "{% if tools %}…{% endif %}{% for m in messages %}{{ m.content }}{% endfor %}" }
    });
    let model = model_from(&item, &MACHINE).unwrap();
    assert_eq!((model.takes, model.does), (vec!["images".to_string()], vec!["tools".to_string()]));
    assert_eq!((model.licence.as_deref(), model.params, model.context_length, model.gated), (Some("gemma"), Some(3_880_099_328), Some(131072), false));
    assert_eq!(model.fits.as_deref(), Some("fits"));
    let thinking = json!({ "id": "a/b", "gguf": { "chat_template": "<think>\n{{ reasoning_content }}</think>" }, "gated": "manual", "cardData": { "license": ["apache-2.0"] } });
    let model = model_from(&thinking, &MACHINE).unwrap();
    assert_eq!((model.does, model.gated, model.licence.as_deref(), model.fits), (vec!["thinking".to_string()], true, Some("apache-2.0"), None));
    let embedding = json!({ "id": "a/e", "pipeline_tag": "feature-extraction" });
    assert_eq!(model_from(&embedding, &MACHINE).unwrap().does, ["embedding"]);
}

#[test]
fn a_card_loses_its_settings_block_and_the_next_page_is_found_in_the_link() {
    assert_eq!(card_from("---\nlicense: apache-2.0\ntags: [gguf]\n---\n\n# Qwen3 4B\n\nA model."), "# Qwen3 4B\n\nA model.");
    assert_eq!(card_from("# No settings\n"), "# No settings");
    assert!(card_from(&"word ".repeat(10_000)).ends_with("(the rest of the card is on Hugging Face)"));
    let link = "<https://huggingface.co/api/models?filter=gguf&cursor=eyJ4IjoxfQ%3D%3D>; rel=\"next\"";
    assert_eq!(next_cursor(link).as_deref(), Some("eyJ4IjoxfQ=="));
    assert_eq!(next_cursor("<https://x/?a=1>; rel=\"prev\""), None);
}

// ----- against a stand-in Hugging Face -------------------------------------------------------------------

const TINY: &[u8] = b"GGUF\x03\x00\x00\x00 a tiny model, for a test, and nothing more than that.";
const VISION: &[u8] = b"GGUF\x03\x00\x00\x00 the part that sees.";

fn sha(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

#[derive(Default)]
struct Seen {
    asked: HashMap<String, usize>,
    ranges: Vec<String>,
    too_much: bool,
    /// The stand-in sends the wrong bytes for the tiny model.
    wrong: bool,
    /// The `Authorization` each file was asked with, by path.
    authorized: HashMap<String, String>,
}

/// The token the stand-in takes for its gated model, as Hugging Face takes a token whose account accepted the terms.
const GOOD_TOKEN: &str = "hf_test_token_that_accepted_the_terms";

async fn site(State(seen): State<Arc<Mutex<Seen>>>, headers: HeaderMap, request: Request) -> Response {
    let path = request.uri().path().to_string();
    let (too_much, wrong) = {
        let mut seen = seen.lock().unwrap();
        *seen.asked.entry(path.clone()).or_default() += 1;
        if let Some(range) = headers.get(header::RANGE).and_then(|r| r.to_str().ok()) {
            seen.ranges.push(range.to_string());
        }
        if let Some(auth) = headers.get(header::AUTHORIZATION).and_then(|a| a.to_str().ok()) {
            seen.authorized.insert(path.clone(), auth.to_string());
        }
        (seen.too_much, seen.wrong)
    };
    if too_much {
        return (StatusCode::TOO_MANY_REQUESTS, [("ratelimit", "\"api\";r=0;t=42")], "").into_response();
    }
    let json = |value: Value| ([(header::CONTENT_TYPE, "application/json")], value.to_string()).into_response();
    let file = |bytes: &[u8]| {
        // From a byte on, as a download that goes on asks.
        match headers.get(header::RANGE).and_then(|r| r.to_str().ok()).and_then(|r| r.strip_prefix("bytes=")).and_then(|r| r.trim_end_matches('-').parse::<usize>().ok()) {
            Some(from) if from < bytes.len() => (StatusCode::PARTIAL_CONTENT, bytes[from..].to_vec()).into_response(),
            _ => bytes.to_vec().into_response(),
        }
    };
    let model = |id: &str, gated: Value, siblings: Vec<Value>| {
        json(
            json!({ "id": id, "author": id.split('/').next(), "sha": "abc123", "gated": gated, "downloads": 10, "likes": 2, "pipeline_tag": "text-generation", "cardData": { "license": "apache-2.0", "base_model": [id.trim_end_matches("-GGUF")] }, "gguf": { "total": 40u64, "architecture": "llama", "context_length": 2048, "chat_template": "{{ messages }}" }, "siblings": siblings }),
        )
    };
    let entry = |name: &str, bytes: &[u8]| json!({ "rfilename": name, "size": bytes.len(), "lfs": { "sha256": sha(bytes), "size": bytes.len() } });
    match path.as_str() {
        "/api/models" => {
            let here = headers.get(header::HOST).and_then(|h| h.to_str().ok()).unwrap_or_default().to_string();
            let page = json!([
                { "id": "test/tiny-GGUF", "author": "test", "downloads": 10, "likes": 2, "trendingScore": 3.0, "gated": false, "gguf": { "total": 15_000_000u64, "chat_template": "{% if tools %}{% endif %}" }, "cardData": { "license": "mit" } },
                { "id": "test/huge-GGUF", "author": "test", "downloads": 5, "likes": 1, "gated": false, "gguf": { "total": 400_000_000_000u64 } },
                { "id": "test/pictures-GGUF", "author": "test", "pipeline_tag": "text-to-image", "gguf": { "total": 7_000_000_000u64 } },
            ]);
            {
                let link = format!("<http://{here}/api/models?cursor=page2>; rel=\"next\"");
                ([(header::CONTENT_TYPE, "application/json".to_string()), (header::LINK, link)], page.to_string()).into_response()
            }
        }
        "/api/models/test/tiny-GGUF" => model("test/tiny-GGUF", json!(false), vec![entry("README.md", b"x"), entry("tiny-Q8_0.gguf", TINY)]),
        "/api/models/test/seeing-GGUF" => model("test/seeing-GGUF", json!(false), vec![entry("seeing-Q4_K_M.gguf", TINY), entry("mmproj-seeing-f16.gguf", VISION)]),
        "/api/models/test/gated-GGUF" => model("test/gated-GGUF", json!("manual"), vec![entry("gated-Q4_K_M.gguf", TINY)]),
        "/test/tiny-GGUF/resolve/abc123/README.md" => "---\nlicense: mit\n---\n# Tiny\n\nA model for tests.".into_response(),
        // As long as the real file, and not it: something between here and there changed it.
        "/test/tiny-GGUF/resolve/abc123/tiny-Q8_0.gguf" if wrong => file(&[b"X".as_slice(), &TINY[1..]].concat()),
        "/test/tiny-GGUF/resolve/abc123/tiny-Q8_0.gguf" | "/test/seeing-GGUF/resolve/abc123/seeing-Q4_K_M.gguf" => file(TINY),
        "/test/seeing-GGUF/resolve/abc123/mmproj-seeing-f16.gguf" => file(VISION),
        // A gated model's file: only with a token whose account accepted the terms.
        "/test/gated-GGUF/resolve/abc123/gated-Q4_K_M.gguf" => {
            if headers.get(header::AUTHORIZATION).and_then(|a| a.to_str().ok()) == Some(format!("Bearer {GOOD_TOKEN}").as_str()) {
                file(TINY)
            } else {
                (StatusCode::FORBIDDEN, "Access to this model is restricted. You must have access to it and be authenticated.").into_response()
            }
        }
        // The model the tiny one was made from, with its maker's settings.
        "/test/tiny/resolve/main/generation_config.json" => json(json!({ "do_sample": true, "temperature": 0.5, "top_k": 30, "top_p": 0.9, "eos_token_id": 2 })),
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

struct World {
    hub: Arc<Hub>,
    seen: Arc<Mutex<Seen>>,
    models: PathBuf,
    events: Arc<EventLog>,
    /// Where the stand-in answers.
    base: String,
    /// The folder the models are kept in, removed with the world.
    _home: crate::testing::TempDir,
}

impl World {
    fn asked(&self, path: &str) -> usize {
        self.seen.lock().unwrap().asked.get(path).copied().unwrap_or(0)
    }

    fn hub_base(&self) -> String {
        self.base.clone()
    }
}

async fn world() -> World {
    let seen = Arc::new(Mutex::new(Seen::default()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let door = listener.local_addr().unwrap();
    let app = Router::new().fallback(site).with_state(seen.clone());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let home = crate::testing::TempDir::new("demido-hub");
    let models = home.join("models");
    std::fs::create_dir_all(&models).unwrap();
    let events = Arc::new(EventLog::new(Arc::new(Db::memory().unwrap())));
    World { hub: Arc::new(Hub::at(format!("http://{door}"), models.clone(), events.clone())), seen, models, events, base: format!("http://{door}"), _home: home }
}

#[tokio::test]
async fn a_search_lists_gguf_models_with_what_they_do_and_whether_they_fit_and_is_kept() {
    let w = world().await;
    let page = w.hub.search("tiny", "trending", &[], &[], false, None, MACHINE).await.unwrap();
    assert_eq!(
        page.models.iter().map(|m| (m.id.as_str(), m.fits.as_deref())).collect::<Vec<_>>(),
        [("test/tiny-GGUF", Some("fits")), ("test/huge-GGUF", Some("no"))],
        "a model that makes pictures is left out"
    );
    assert_eq!((page.models[0].does.clone(), page.models[0].licence.as_deref()), (vec!["tools".to_string()], Some("mit")));
    assert_eq!(page.next.as_deref(), Some("page2"));
    // Only what fits, and only what calls tools: kept out here, not asked again.
    let fitting = w.hub.search("tiny", "trending", &[], &["tools".to_string()], true, None, MACHINE).await.unwrap();
    assert_eq!(fitting.models.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["test/tiny-GGUF"]);
    assert_eq!(w.asked("/api/models"), 1, "the same search within ten minutes asks nobody");
    // A search with other words is asked; Hugging Face saying it has had enough is said in words.
    w.seen.lock().unwrap().too_much = true;
    let said = w.hub.search("other", "downloads", &[], &[], false, None, MACHINE).await.unwrap_err().to_string();
    assert!(said.contains("asked too much") && said.contains("42 seconds"), "{said}");
    w.seen.lock().unwrap().too_much = false;
    let said = w.hub.search("third", "downloads", &[], &[], false, None, MACHINE).await.unwrap_err().to_string();
    assert!(said.contains("left alone"), "it is not asked while it said to wait: {said}");
    assert_eq!(w.asked("/api/models"), 2);
}

#[tokio::test]
async fn a_model_page_has_its_files_by_quantization_and_its_card() {
    let w = world().await;
    let detail = w.hub.detail("test/tiny-GGUF", MACHINE).await.unwrap();
    assert_eq!((detail.revision.as_str(), detail.page.as_str()), ("abc123", "https://huggingface.co/test/tiny-GGUF"));
    assert_eq!(detail.quants.iter().map(|q| (q.name.as_str(), q.recommended, q.on_disk)).collect::<Vec<_>>(), [("Q8_0", true, false)]);
    assert_eq!(detail.card.as_deref(), Some("# Tiny\n\nA model for tests."));
    let seeing = w.hub.detail("test/seeing-GGUF", MACHINE).await.unwrap();
    assert_eq!(seeing.model.takes, ["images"], "a vision part says it sees, whatever its tags say");
    assert!(w.hub.detail("not a name", MACHINE).await.is_err() && w.hub.detail("test/none-GGUF", MACHINE).await.is_err());
}

#[tokio::test]
async fn a_model_is_downloaded_checked_put_in_place_and_written_down() {
    let w = world().await;
    let started = w.hub.start("test/tiny-GGUF", "Q8_0", MACHINE).await.unwrap();
    assert_eq!((started.local_id.as_str(), started.total, started.files.clone()), ("tiny-Q8_0", TINY.len() as u64, vec!["tiny-Q8_0.gguf".to_string()]));
    let done = w.hub.wait_for(&started.id, Duration::from_secs(10)).await.unwrap();
    assert_eq!((done.state.as_str(), done.done, done.problem), ("done", TINY.len() as u64, None));
    // One file stays a file, where the runtime finds it; nothing is left half done.
    assert_eq!(std::fs::read(w.models.join("tiny-Q8_0.gguf")).unwrap(), TINY);
    assert!(!w.models.join(INCOMING).join("tiny-Q8_0").exists());
    let sources: Value = serde_json::from_slice(&std::fs::read(w.models.join(SOURCES)).unwrap()).unwrap();
    assert_eq!(
        (sources["tiny-Q8_0"]["repo"].as_str(), sources["tiny-Q8_0"]["revision"].as_str(), sources["tiny-Q8_0"]["licence"].as_str()),
        (Some("test/tiny-GGUF"), Some("abc123"), Some("apache-2.0"))
    );
    // The model it was made from, and the settings its maker gives it, for asking it the maker's way.
    assert_eq!(sources["tiny-Q8_0"]["base_model"], "test/tiny");
    assert_eq!(sources["tiny-Q8_0"]["maker_sampling"], json!({ "temperature": 0.5, "top_p": 0.9, "top_k": 30 }));
    let kinds: Vec<String> = w.events.after(0, 50).unwrap().into_iter().map(|e| e.kind).collect();
    assert_eq!(kinds, ["model.download.started", "model.downloaded"]);
    // Now it is on disk, and the page says so; a second download of it is refused.
    let detail = w.hub.detail("test/tiny-GGUF", MACHINE).await.unwrap();
    assert!(detail.quants[0].on_disk);
    assert!(w.hub.start("test/tiny-GGUF", "Q8_0", MACHINE).await.unwrap_err().to_string().contains("already"));
    assert!(w.hub.start("test/tiny-GGUF", "Q2_K", MACHINE).await.unwrap_err().to_string().contains("has no Q2_K. It has: Q8_0"));
}

#[tokio::test]
async fn a_model_with_a_vision_part_becomes_a_folder_of_both() {
    let w = world().await;
    let started = w.hub.start("test/seeing-GGUF", "Q4_K_M", MACHINE).await.unwrap();
    assert_eq!(started.files, ["seeing-Q4_K_M.gguf", "mmproj-seeing-f16.gguf"]);
    let done = w.hub.wait_for(&started.id, Duration::from_secs(10)).await.unwrap();
    assert_eq!(done.state, "done", "{:?}", done.problem);
    assert_eq!(std::fs::read(w.models.join("seeing-Q4_K_M/seeing-Q4_K_M.gguf")).unwrap(), TINY);
    assert_eq!(std::fs::read(w.models.join("seeing-Q4_K_M/mmproj-seeing-f16.gguf")).unwrap(), VISION);
}

#[tokio::test]
async fn a_download_goes_on_from_where_it_stopped() {
    let w = world().await;
    // What an earlier try left: the first bytes of the file.
    let incoming = w.models.join(INCOMING).join("tiny-Q8_0");
    std::fs::create_dir_all(&incoming).unwrap();
    std::fs::write(incoming.join("tiny-Q8_0.gguf.part"), &TINY[..10]).unwrap();
    let started = w.hub.start("test/tiny-GGUF", "Q8_0", MACHINE).await.unwrap();
    let done = w.hub.wait_for(&started.id, Duration::from_secs(10)).await.unwrap();
    assert_eq!(done.state, "done", "{:?}", done.problem);
    assert_eq!(std::fs::read(w.models.join("tiny-Q8_0.gguf")).unwrap(), TINY);
    assert_eq!(w.seen.lock().unwrap().ranges, ["bytes=10-"], "only the rest was asked for");
}

#[tokio::test]
async fn a_file_that_is_not_the_one_published_is_not_kept_and_a_gated_model_is_not_downloaded() {
    let w = world().await;
    // The page lists the real file's checksum; the bytes sent are other bytes.
    w.seen.lock().unwrap().wrong = true;
    let started = w.hub.start("test/tiny-GGUF", "Q8_0", MACHINE).await.unwrap();
    let ended = w.hub.wait_for(&started.id, Duration::from_secs(10)).await.unwrap();
    assert_eq!(ended.state, "failed");
    assert!(ended.problem.as_deref().unwrap().contains("is not the file that was published"), "{:?}", ended.problem);
    assert!(!w.models.join("tiny-Q8_0.gguf").exists());
    assert!(w.events.after(0, 50).unwrap().iter().any(|e| e.kind == "model.download.failed"));

    // A gated model, with no Hugging Face token kept: refused before anything is asked for.
    let said = w.hub.start("test/gated-GGUF", "Q4_K_M", MACHINE).await.unwrap_err().to_string();
    assert!(said.contains("is gated") && said.contains("add your Hugging Face token"), "{said}");
    assert_eq!(w.asked("/test/gated-GGUF/resolve/abc123/gated-Q4_K_M.gguf"), 0);
}

#[tokio::test]
async fn a_gated_model_is_downloaded_with_the_persons_token_and_the_token_goes_only_with_it() {
    let w = world().await;
    let home = w.models.parent().unwrap().to_path_buf();
    let vault = Arc::new(crate::vault::Vault::in_file(&home));
    let hub = Arc::new(Hub::at(w.hub_base(), w.models.clone(), w.events.clone()).with_vault(vault.clone()));
    // A token whose account did not accept the terms: Hugging Face says no, and the person is told why.
    vault.put(TOKEN, "hf_test_token_without_the_terms").unwrap();
    let started = hub.start("test/gated-GGUF", "Q4_K_M", MACHINE).await.unwrap();
    let ended = hub.wait_for(&started.id, Duration::from_secs(10)).await.unwrap();
    assert_eq!(ended.state, "failed");
    assert!(ended.problem.as_deref().unwrap().contains("Accept its maker's terms"), "{:?}", ended.problem);
    // The right one: it downloads, the file is asked for with the token, and nothing else is.
    vault.put(TOKEN, GOOD_TOKEN).unwrap();
    let started = hub.start("test/gated-GGUF", "Q4_K_M", MACHINE).await.unwrap();
    let done = hub.wait_for(&started.id, Duration::from_secs(10)).await.unwrap();
    assert_eq!(done.state, "done", "{:?}", done.problem);
    assert_eq!(std::fs::read(w.models.join("gated-Q4_K_M.gguf")).unwrap(), TINY);
    let authorized = w.seen.lock().unwrap().authorized.clone();
    assert_eq!(authorized.get("/test/gated-GGUF/resolve/abc123/gated-Q4_K_M.gguf").map(String::as_str), Some(format!("Bearer {GOOD_TOKEN}").as_str()));
    assert!(authorized.keys().all(|p| p.starts_with("/test/gated-GGUF/resolve/")), "the token went with nothing but the gated model's files: {authorized:?}");
    // Another model's files are asked for without it.
    let started = hub.start("test/tiny-GGUF", "Q8_0", MACHINE).await.unwrap();
    hub.wait_for(&started.id, Duration::from_secs(10)).await.unwrap();
    assert!(!w.seen.lock().unwrap().authorized.contains_key("/test/tiny-GGUF/resolve/abc123/tiny-Q8_0.gguf"));
}
