//! Packages: made and signed here as a publisher would, with the component of `tools/word-count` (a tool of
//! Demido's own, built from its source in the repository).

use super::*;

/// The sample tool's component, as `tools/word-count/build.sh` made it.
const WASM: &[u8] = include_bytes!("../../../../../tools/word-count/package/tool.wasm");

pub(crate) struct Publisher {
    pair: minisign::KeyPair,
}

impl Publisher {
    pub(crate) fn new() -> Self {
        Self { pair: minisign::KeyPair::generate_unencrypted_keypair().unwrap() }
    }
    fn key(&self) -> String {
        self.pair.pk.to_base64()
    }
    /// A package, and its signature.
    pub(crate) fn package(&self, version: &str, asks: Asks, key: Option<String>) -> (Vec<u8>, String) {
        let manifest = Manifest {
            name: "word_count".into(),
            version: version.into(),
            publisher: "A test".into(),
            publisher_key: key.unwrap_or_else(|| self.key()),
            about: "Counts the words of a file.".into(),
            does: "The words, lines and characters of a file in the folder.".into(),
            args: vec![ManifestArg { name: "path".into(), kind: "text".into(), optional: false }],
            asks,
            updates: None,
        };
        let mut tar = tar::Builder::new(Vec::new());
        for (name, bytes) in [("tool.json", serde_json::to_vec(&manifest).unwrap()), ("guidance.md", b"Count a file.".to_vec()), ("tool.wasm", WASM.to_vec())] {
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            tar.append_data(&mut header, name, bytes.as_slice()).unwrap();
        }
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        std::io::Write::write_all(&mut gz, &tar.into_inner().unwrap()).unwrap();
        let archive = gz.finish().unwrap();
        let sig = minisign::sign(Some(&self.pair.pk), &self.pair.sk, std::io::Cursor::new(&archive), None, None).unwrap().to_string();
        (archive, sig)
    }
}

pub(crate) fn reading() -> Asks {
    Asks { folder: "read".into(), network: vec![] }
}

fn packages(home: &Path) -> Packages {
    Packages::new(home, Arc::new(EventLog::new(Arc::new(crate::db::Db::memory().unwrap()))))
}

#[test]
fn a_package_is_installed_only_when_it_is_signed_by_the_key_it_names_and_the_person_said_yes() {
    let home = crate::testing::TempDir::new("demido-packages");
    let p = packages(&home);
    let publisher = Publisher::new();
    let (archive, sig) = publisher.package("0.1.0", reading(), None);

    let checked = p.check(&archive, &sig).unwrap();
    assert_eq!((checked.manifest.name.as_str(), checked.replaces.as_deref(), checked.key_id.len()), ("word_count", None, 16));
    assert_eq!(checked.asks_said, "It asks for: read files in the session's folder; nothing from the internet.");
    assert!(p.install(&archive, &sig, false).unwrap_err().to_string().contains("needs your yes"));
    assert!(p.all().is_empty());

    // A changed byte, or another's signature: refused.
    let mut changed = archive.clone();
    *changed.last_mut().unwrap() ^= 1;
    assert!(p.check(&changed, &sig).unwrap_err().to_string().contains("signature does not match"));
    let impostor = Publisher::new();
    let (_, other_sig) = impostor.package("0.1.0", reading(), Some(publisher.key()));
    assert!(p.check(&archive, &other_sig).is_err());

    let installed = p.install(&archive, &sig, true).unwrap();
    assert_eq!((installed.manifest.version.as_str(), installed.key_id.as_str()), ("0.1.0", checked.key_id.as_str()));
    let tool = p.tool("word_count").unwrap();
    assert_eq!((tool.reach, tool.args[0].name), (crate::tools::Reach::Reads, "path"));

    // An update from another publisher is refused; one that asks for more says so, and asks again.
    let (theirs, theirs_sig) = impostor.package("0.2.0", reading(), None);
    assert!(p.check(&theirs, &theirs_sig).unwrap_err().to_string().contains("signed with another key"));
    let (more, more_sig) = publisher.package("0.2.0", Asks { folder: "write".into(), network: vec!["api.example.org".into()] }, None);
    let checked = p.check(&more, &more_sig).unwrap();
    assert_eq!(checked.replaces.as_deref(), Some("0.1.0"));
    assert_eq!(checked.asks_more, ["the folder (write)", "reading from api.example.org"]);

    // The person's own guidance survives an update.
    p.edit("word_count", Some("My own words."), Some("Counts a file's words.")).unwrap();
    p.install(&more, &more_sig, true).unwrap();
    let after = p.get("word_count").unwrap();
    assert_eq!((after.manifest.version.as_str(), after.guidance.as_str(), after.does.as_str(), after.edited), ("0.2.0", "My own words.", "Counts a file's words.", true));
    assert_eq!(p.tool("word_count").unwrap().does, "Counts a file's words.");
    // Kept on disk: a new start finds it.
    assert_eq!(packages(&home).all().len(), 1);
    p.remove("word_count").unwrap();
    assert!(p.all().is_empty() && p.tool("word_count").is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_tool_runs_shut_in_and_reaches_only_what_its_manifest_asks_for() {
    let home = crate::testing::TempDir::new("demido-packages-run");
    let folder = home.join("work");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("notes.md"), "Grey herons nest\nin the reeds.\n").unwrap();
    let p = Arc::new(packages(&home));
    let publisher = Publisher::new();
    let (archive, sig) = publisher.package("0.1.0", reading(), None);
    p.install(&archive, &sig, true).unwrap();
    let wasm = p.wasm("word_count").unwrap();
    let ground = |asks: Asks, folder: Option<PathBuf>| run::Ground { asks, folder, web: Arc::new(crate::web::Web::new(None)), handle: tokio::runtime::Handle::current() };

    let (p2, w2, g) = (p.clone(), wasm.clone(), ground(reading(), Some(folder.clone())));
    let ran = tokio::task::spawn_blocking(move || run::run(&p2.runtime, &w2, r#"{"path": "notes.md"}"#, g)).await.unwrap();
    assert_eq!(ran.gave, Ok("notes.md: 6 words, 2 lines, 31 characters.".to_string()));
    assert_eq!(ran.log, ["counted notes.md"]);

    // Out of the folder: refused by the host, whatever the tool asks.
    let (p2, w2, g) = (p.clone(), wasm.clone(), ground(reading(), Some(folder.clone())));
    let ran = tokio::task::spawn_blocking(move || run::run(&p2.runtime, &w2, r#"{"path": "../../etc/passwd"}"#, g)).await.unwrap();
    assert!(ran.gave.as_ref().unwrap_err().contains("outside") || ran.gave.as_ref().unwrap_err().contains("leads out"), "{:?}", ran.gave);
    // A tool that did not ask for the folder is not given it.
    let (p2, w2, g) = (p.clone(), wasm.clone(), ground(Asks::default(), Some(folder.clone())));
    let ran = tokio::task::spawn_blocking(move || run::run(&p2.runtime, &w2, r#"{"path": "notes.md"}"#, g)).await.unwrap();
    assert_eq!(ran.gave, Err("this tool did not ask for the folder".to_string()));
    // Arguments it cannot use: its own words.
    let (p2, w2, g) = (p.clone(), wasm, ground(reading(), Some(folder)));
    let ran = tokio::task::spawn_blocking(move || run::run(&p2.runtime, &w2, "{}", g)).await.unwrap();
    assert_eq!(ran.gave, Err("word_count needs \"path\", the file to count".to_string()));
    // Something that is not a component is not installed.
    assert!(p.runtime.compile(b"\0asm not really").is_err());
}
