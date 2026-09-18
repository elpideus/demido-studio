//! The library, against a real temporary directory with real files.
//!
//! Borrowing, part classification and the directory-as-registry rule are only
//! interesting against real paths and real Windows path shapes (#36), so every
//! test here writes GGUF bytes into a folder the OS made and reads them back
//! through the same calls the window makes.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

mod support;

use std::path::PathBuf;

use demido_models::{Damage, Error, Fact, Folders, Library, Local};
use support::{snapshot, truncate, write, Spec};

/// Demido's own folder and one borrowed one, both real and both empty.
struct Rig {
    _temp: tempfile::TempDir,
    mine: PathBuf,
    theirs: PathBuf,
}

impl Rig {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("a temporary directory");
        let mine = temp.path().join("profile").join("models");
        let theirs = temp.path().join("lmstudio").join("models");
        std::fs::create_dir_all(&mine).expect("made mine");
        std::fs::create_dir_all(&theirs).expect("made theirs");
        Rig {
            _temp: temp,
            mine,
            theirs,
        }
    }

    fn folders(&self) -> Folders {
        Folders {
            download: self.mine.clone(),
            scan: vec![self.theirs.clone()],
        }
    }

    fn library(&self) -> Library {
        Library::open(&self.folders())
    }
}

fn labels(models: &[Local]) -> Vec<&str> {
    models.iter().map(|model| model.label.as_str()).collect()
}

fn named<'a>(models: &'a [Local], label: &str) -> &'a Local {
    models
        .iter()
        .find(|model| model.label == label)
        .unwrap_or_else(|| panic!("no {label} in {:?}", labels(models)))
}

#[test]
fn the_download_folder_and_the_scan_folders_are_the_library_and_nothing_records_it() {
    let rig = Rig::new();
    write(
        &rig.mine
            .join("unsloth/Qwen3.5-4B-GGUF/Qwen3.5-4B-Q4_K_M.gguf"),
        &Spec::model("qwen35"),
    );
    write(
        &rig.theirs
            .join("lmstudio-community/gemma-4-GGUF/gemma-4-E4B-it-Q8_0.gguf"),
        &Spec::model("gemma4"),
    );
    let mine = snapshot(&rig.mine);
    let theirs = snapshot(&rig.theirs);

    let scan = rig.library().scan();

    assert_eq!(
        labels(&scan.models),
        ["gemma-4-E4B-it-Q8_0", "Qwen3.5-4B-Q4_K_M"]
    );
    let owned = named(&scan.models, "Qwen3.5-4B-Q4_K_M");
    assert!(!owned.borrowed);
    assert_eq!(owned.repo.as_deref(), Some("unsloth/Qwen3.5-4B-GGUF"));
    assert_eq!(snapshot(&rig.mine), mine, "no bookkeeping file in mine");
    assert_eq!(snapshot(&rig.theirs), theirs, "nor in theirs");
}

#[test]
fn a_model_deleted_by_another_tool_is_gone_on_the_next_scan() {
    let rig = Rig::new();
    let model = rig.theirs.join("a/b/Model-Q8_0.gguf");
    write(&model, &Spec::model("llama"));
    let library = rig.library();
    assert_eq!(library.scan().models.len(), 1);

    std::fs::remove_file(&model).expect("the other tool deleted it");

    assert!(library.scan().models.is_empty(), "nothing lingers");
}

#[test]
fn a_model_copied_in_by_hand_is_there_on_the_next_scan() {
    let rig = Rig::new();
    let library = rig.library();
    assert!(library.scan().models.is_empty());

    write(&rig.mine.join("my-own-merge.gguf"), &Spec::model("llama"));

    let scan = library.scan();
    assert_eq!(labels(&scan.models), ["my-own-merge"]);
    assert_eq!(
        scan.models[0].repo, None,
        "a file with no repository is still a model"
    );
}

#[test]
fn removal_refuses_every_path_outside_demidos_own_root() {
    let rig = Rig::new();
    let borrowed = rig.theirs.join("p/m/Theirs-Q8_0.gguf");
    write(&borrowed, &Spec::model("llama"));
    // A sibling whose name starts with the root's: a prefix test on strings
    // would take it for a path inside.
    let sibling = rig.mine.with_file_name("models-elsewhere").join("x.gguf");
    write(&sibling, &Spec::model("llama"));
    // Inside the root by its spelling and outside it by where it resolves.
    let climbing = rig.mine.join("..").join("models-elsewhere").join("x.gguf");
    let library = rig.library();

    for path in [&borrowed, &sibling, &climbing] {
        let refused = library.remove(path);
        assert!(
            matches!(refused, Err(Error::Outside { .. })),
            "{} was not refused: {refused:?}",
            path.display()
        );
    }
    assert!(borrowed.is_file());
    assert!(sibling.is_file());
}

#[test]
fn removal_refuses_a_borrowed_folder_even_inside_demidos_own() {
    let rig = Rig::new();
    let nested = rig.mine.join("shared-with-my-brother");
    let model = nested.join("Shared-Q8_0.gguf");
    write(&model, &Spec::model("llama"));
    let library = Library::open(&Folders {
        download: rig.mine.clone(),
        scan: vec![nested.clone()],
    });

    let scan = library.scan();
    assert_eq!(scan.models.len(), 1, "listed once, not once per folder");
    assert!(
        scan.models[0].borrowed,
        "the borrowed folder is the closer one"
    );
    assert!(matches!(library.remove(&model), Err(Error::Outside { .. })));
    assert!(model.is_file());
}

#[test]
fn removal_inside_the_root_takes_every_piece_and_nothing_else() {
    let rig = Rig::new();
    let folder = rig.mine.join("r/m");
    for piece in 1..=2 {
        write(
            &folder.join(format!("Big-Q4_K_M-0000{piece}-of-00002.gguf")),
            &Spec::model("llama"),
        );
    }
    write(&folder.join("Other-Q8_0.gguf"), &Spec::model("llama"));
    let library = rig.library();

    let removed = library
        .remove(&folder.join("Big-Q4_K_M-00001-of-00002.gguf"))
        .expect("it is Demido's");

    assert_eq!(removed.len(), 2);
    assert_eq!(labels(&library.scan().models), ["Other-Q8_0"]);
}

#[test]
fn nothing_is_written_into_a_borrowed_folder() {
    let rig = Rig::new();
    let model = rig.theirs.join("p/m/Theirs-Q8_0.gguf");
    write(&model, &Spec::model("llama"));
    write(
        &rig.theirs.join("p/m/mmproj-F16.gguf"),
        &Spec {
            sees: Some(true),
            ..Spec::model("clip")
        },
    );
    let before = snapshot(&rig.theirs);
    let library = rig.library();

    let _ = library.scan();
    let _ = library.remove(&model);
    let _ = library.remove(&rig.theirs.join("p/m/mmproj-F16.gguf"));
    let landing = library.destination("p/m", "Theirs-Q8_0.gguf");

    assert_eq!(snapshot(&rig.theirs), before);
    assert!(
        landing.starts_with(&rig.mine),
        "a download of the same file lands in Demido's own folder: {}",
        landing.display()
    );
}

#[test]
fn a_download_never_lands_outside_the_download_folder() {
    let rig = Rig::new();
    let library = rig.library();
    for (repo, file) in [
        ("../../etc", "passwd.gguf"),
        ("a/b", "../../../x.gguf"),
        ("C:/Windows", "x.gguf"),
        ("p/m", "C:\\x.gguf"),
    ] {
        let landing = library.destination(repo, file);
        // Lexically under the folder, and with nothing below it that climbs
        // back out: `starts_with` alone is satisfied by `mine/../..`.
        let below = landing
            .strip_prefix(&rig.mine)
            .unwrap_or_else(|_| panic!("{repo} {file} lands at {}", landing.display()));
        assert!(
            below
                .components()
                .all(|part| matches!(part, std::path::Component::Normal(_))),
            "{repo} {file} climbs out at {}",
            landing.display()
        );
    }
}

#[test]
fn a_borrowed_row_names_the_library_it_came_from() {
    let rig = Rig::new();
    write(
        &rig.theirs.join("p/m/Theirs-Q8_0.gguf"),
        &Spec::model("llama"),
    );
    write(&rig.mine.join("p/m/Mine-Q8_0.gguf"), &Spec::model("llama"));

    let scan = rig.library().scan();

    let theirs = named(&scan.models, "Theirs-Q8_0");
    assert!(theirs.borrowed);
    assert_eq!(theirs.library, rig.theirs.display().to_string());
    assert!(!named(&scan.models, "Mine-Q8_0").borrowed);
}

#[test]
fn changing_the_download_folder_moves_nothing() {
    let rig = Rig::new();
    let model = rig.mine.join("p/m/Mine-Q8_0.gguf");
    write(&model, &Spec::model("llama"));
    let elsewhere = rig.mine.parent().unwrap().join("bigger-drive");
    let before = snapshot(&rig.mine);

    let folders = rig.folders().with_download(elsewhere.clone());
    let scan = Library::open(&folders).scan();

    assert_eq!(snapshot(&rig.mine), before, "the old folder is untouched");
    assert!(!elsewhere.exists(), "and the new one is not even made yet");
    assert!(
        folders.scan.contains(&rig.mine),
        "the old folder is still read"
    );
    let still = named(&scan.models, "Mine-Q8_0");
    assert_eq!(still.path, model);
    assert!(still.borrowed, "and it is no longer Demido's to delete");
}

#[test]
fn borrowed_bytes_are_not_disk_demido_spent() {
    let rig = Rig::new();
    let mine = write(&rig.mine.join("p/m/Mine-Q8_0.gguf"), &Spec::model("llama"));
    let partial = rig.mine.join("p/m/Next-Q8_0.gguf.part");
    std::fs::write(&partial, vec![0u8; 100]).expect("a download in flight");
    write(
        &rig.theirs.join("p/m/Theirs-Q8_0.gguf"),
        &Spec {
            weights: 4096,
            ..Spec::model("llama")
        },
    );

    let scan = rig.library().scan();

    assert_eq!(scan.spent, mine + 100, "Demido's files, partial included");
}

#[test]
fn what_a_model_is_capable_of_is_read_out_of_the_file() {
    let rig = Rig::new();
    let folder = rig.mine.join("p/vision");
    write(
        &folder.join("Seer-Q8_0.gguf"),
        &Spec {
            architecture: Some("gemma4"),
            context: Some(131_072),
            template: Some(
                "{% if tools %}<tools>{{ tools }}</tools>{% endif %}{% if enable_thinking %}<think>{% endif %}",
            ),
            ..Spec::model("gemma4")
        },
    );
    write(
        &folder.join("mmproj-F16.gguf"),
        &Spec {
            sees: Some(true),
            hears: Some(true),
            ..Spec::model("clip")
        },
    );
    write(
        &rig.mine.join("p/plain/Plain-Q8_0.gguf"),
        &Spec {
            template: Some("{{ messages }}"),
            ..Spec::model("llama")
        },
    );
    write(
        &rig.mine.join("p/bare/Bare-Q8_0.gguf"),
        &Spec::model("llama"),
    );

    let scan = rig.library().scan();

    let seer = named(&scan.models, "Seer-Q8_0");
    assert_eq!(seer.architecture.as_deref(), Some("gemma4"));
    assert_eq!(seer.context, Some(131_072));
    assert_eq!(
        seer.capabilities.vision,
        Fact::Yes,
        "a projector is beside it"
    );
    assert_eq!(seer.capabilities.audio, Fact::Yes, "and it says it hears");
    assert_eq!(seer.capabilities.tools, Fact::Yes);
    assert_eq!(seer.capabilities.reasoning, Fact::Yes);

    let plain = named(&scan.models, "Plain-Q8_0");
    assert_eq!(plain.capabilities.vision, Fact::No, "no projector, no eyes");
    assert_eq!(plain.capabilities.audio, Fact::No);
    assert_eq!(
        plain.capabilities.tools,
        Fact::No,
        "a template that never names tools"
    );
    assert_eq!(plain.capabilities.reasoning, Fact::No);

    let bare = named(&scan.models, "Bare-Q8_0");
    assert_eq!(
        bare.capabilities.tools,
        Fact::Unknown,
        "no template, no fact"
    );
    assert_eq!(bare.capabilities.reasoning, Fact::Unknown);
}

#[test]
fn a_projector_and_a_draft_travel_with_their_weights_and_are_not_offered() {
    let rig = Rig::new();
    let folder = rig.theirs.join("unsloth/gemma-4-E4B-it-GGUF");
    write(
        &folder.join("gemma-4-E4B-it-Q8_0.gguf"),
        &Spec::model("gemma4"),
    );
    write(
        &folder.join("mmproj-BF16.gguf"),
        &Spec {
            sees: Some(true),
            ..Spec::model("clip")
        },
    );
    write(
        &folder.join("mtp-gemma-4-E4B-it-Q8_0.gguf"),
        &Spec::model("gemma4"),
    );

    let scan = rig.library().scan();

    assert_eq!(labels(&scan.models), ["gemma-4-E4B-it-Q8_0"]);
    assert_eq!(scan.models[0].companions.len(), 2);
}

#[test]
fn a_folder_holding_only_a_projector_offers_nothing() {
    let rig = Rig::new();
    write(
        &rig.mine.join("p/m/mmproj-F32.gguf"),
        &Spec {
            sees: Some(true),
            ..Spec::model("clip")
        },
    );
    assert!(rig.library().scan().models.is_empty());
}

#[test]
fn a_split_model_is_one_row_whose_size_is_every_piece() {
    let rig = Rig::new();
    let mut total = 0;
    for piece in 1..=3 {
        total += write(
            &rig.mine
                .join(format!("r/m/DeepSeek-Q4_K_M-0000{piece}-of-00003.gguf")),
            &Spec::model("deepseek2"),
        );
    }

    let scan = rig.library().scan();

    assert_eq!(labels(&scan.models), ["DeepSeek-Q4_K_M"]);
    let model = &scan.models[0];
    assert_eq!(model.shards, Some(3));
    assert_eq!(model.bytes, total);
    assert!(model.path.ends_with("DeepSeek-Q4_K_M-00001-of-00003.gguf"));
}

#[test]
fn a_truncated_model_is_refused_before_it_is_offered() {
    let rig = Rig::new();
    let model = rig.mine.join("p/m/Cut-Q8_0.gguf");
    let whole = write(&model, &Spec::model("llama"));
    truncate(&model, 40);

    let scan = rig.library().scan();

    assert!(scan.models.is_empty(), "a backend never gets to find out");
    assert_eq!(scan.damaged.len(), 1);
    assert_eq!(scan.damaged[0].path, model);
    assert_eq!(
        scan.damaged[0].damage,
        Damage::Truncated {
            needs: Some(whole),
            has: whole - 40
        }
    );
}

#[test]
fn a_file_cut_off_inside_its_header_is_refused_too() {
    let rig = Rig::new();
    let model = rig.mine.join("p/m/Stub-Q8_0.gguf");
    write(&model, &Spec::model("llama"));
    std::fs::OpenOptions::new()
        .write(true)
        .open(&model)
        .unwrap()
        .set_len(20)
        .unwrap();

    let scan = rig.library().scan();

    assert!(matches!(
        scan.damaged[0].damage,
        Damage::Truncated {
            needs: None,
            has: 20
        }
    ));
}

#[test]
fn a_login_page_saved_as_a_gguf_is_not_a_model() {
    let rig = Rig::new();
    let page = rig.mine.join("p/gated/Gated-Q4_K_M.gguf");
    std::fs::create_dir_all(page.parent().unwrap()).unwrap();
    std::fs::write(&page, b"<!doctype html><html>Sign in</html>").unwrap();

    let scan = rig.library().scan();

    assert!(scan.models.is_empty());
    assert_eq!(scan.damaged[0].damage, Damage::NotGguf);
}

#[test]
fn a_split_model_with_a_piece_missing_is_refused() {
    let rig = Rig::new();
    for piece in [1, 3] {
        write(
            &rig.mine.join(format!("r/m/Big-0000{piece}-of-00003.gguf")),
            &Spec::model("llama"),
        );
    }

    let scan = rig.library().scan();

    assert!(scan.models.is_empty());
    assert_eq!(
        scan.damaged[0].damage,
        Damage::MissingPiece { index: 2, total: 3 }
    );
}

#[test]
fn a_folder_that_is_not_there_is_an_empty_library_rather_than_a_failure() {
    let rig = Rig::new();
    let library = Library::open(&Folders {
        download: rig.mine.join("never-made"),
        scan: vec![PathBuf::from("Z:/not/plugged/in")],
    });
    let scan = library.scan();
    assert!(scan.models.is_empty());
    assert_eq!(scan.spent, 0);
}

#[test]
fn a_folder_named_twice_in_two_spellings_is_listed_once() {
    let rig = Rig::new();
    write(
        &rig.theirs.join("p/m/Theirs-Q8_0.gguf"),
        &Spec::model("llama"),
    );
    let shouting = PathBuf::from(rig.theirs.display().to_string().to_uppercase() + "\\");
    let library = Library::open(&Folders {
        download: rig.mine.clone(),
        scan: vec![rig.theirs.clone(), shouting],
    });
    assert_eq!(library.scan().models.len(), 1);
}

/// Found on #79, in LM Studio's own folder for Gemma 4 E4B: a 94 MiB draft head
/// beside the 7.8 GiB model. As the smallest file there it was what the runtime
/// was verified against, and `llama.cpp` refuses a context from one alone.
#[test]
fn the_smallest_model_is_never_a_projector_or_a_draft_head() {
    let rig = Rig::new();
    let folder = rig.theirs.join("unsloth/gemma-4-E4B-it-GGUF");
    write(
        &folder.join("gemma-4-E4B-it-Q8_0.gguf"),
        &Spec {
            weights: 4096,
            ..Spec::model("gemma4")
        },
    );
    write(
        &folder.join("mtp-gemma-4-E4B-it-Q8_0.gguf"),
        &Spec::model("gemma4"),
    );
    write(
        &folder.join("mmproj-F16.gguf"),
        &Spec {
            sees: Some(true),
            ..Spec::model("clip")
        },
    );
    write(
        &rig.mine.join("p/m/Tiny-Q4_0.gguf"),
        &Spec {
            weights: 1024,
            ..Spec::model("llama")
        },
    );

    let scan = rig.library().scan();

    assert_eq!(
        scan.smallest().map(|model| model.label.as_str()),
        Some("Tiny-Q4_0")
    );
    assert_eq!(
        named(&scan.models, "gemma-4-E4B-it-Q8_0").folder,
        rig.theirs,
        "a row says which folder it was read out of"
    );
}
