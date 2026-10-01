//! Files the person attaches to a message.
//!
//! A file lives in two places in turn. While it waits in the composer it is *staged*: copied to
//! `staging/<id>/<name>`, read once (its kind, the text a model can read, that text's passages
//! for search, and what a model is given of an image or a sound) and stored in the database with
//! no chat. Sending the message moves it into the chat's workspace, `uploads/<name>`, where the
//! assistant's tools can use it too, and links it to the message. What the model reads of it is
//! decided when the prompt is built ([`context`]); [`search`] finds its passages by their words,
//! and [`meaning`] by what they mean.

pub mod context;
pub mod meaning;
pub mod search;

use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};

use demido_extract::Kind;
use tokio::sync::Semaphore;

use crate::agent::prompt::estimate_tokens;
use crate::bail_msg;
use crate::db::{Attachment, AttachmentContent, Db, new_id, now_ms};
use crate::error::{AppError, CmdResult};
use crate::paths::AppPaths;
use crate::state::AppState;

/// Largest file that can be attached.
pub const MAX_FILE_BYTES: u64 = 100 * 1024 * 1024;
/// Most files one message can carry.
pub const MAX_PER_MESSAGE: usize = 20;
/// Long side of the copy of an image a model reads: enough for text in a screenshot, without
/// spending thousands of tokens per picture.
pub(crate) const IMAGE_EDGE: u32 = 1568;
/// Largest sound a model is given: providers refuse requests much above 20 MB.
const MAX_AUDIO_BYTES: u64 = 10 * 1024 * 1024;
/// Size of a passage, about 650 tokens, and how much of the previous one it repeats. Small
/// enough that a model with a small context window is still shown several of them.
const PASSAGE_CHARS: usize = 2000;
const PASSAGE_OVERLAP: usize = 200;
/// The workspace folder sent files go to.
pub const UPLOADS: &str = "uploads";

/// Files read at the same time. A dozen dropped PDFs or photos read at once would hold them all
/// in memory together, next to a model that may already fill it.
static READING: LazyLock<Semaphore> = LazyLock::new(|| Semaphore::new(2));

/// Stages a file from disk (the file dialog, drag and drop).
pub async fn stage_file(state: &Arc<AppState>, source: PathBuf) -> CmdResult<Attachment> {
    let name = source
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".into());
    let meta = tokio::fs::metadata(&source)
        .await
        .map_err(|_| AppError::msg(format!("{name} could not be opened.")))?;
    if meta.is_dir() {
        bail_msg!("{name} is a folder. Add the files inside it instead.");
    }
    if meta.len() > MAX_FILE_BYTES {
        bail_msg!("{name} is larger than 100 MB, the most a file can be.");
    }
    let _turn = READING.acquire().await.map_err(|e| AppError::msg(e.to_string()))?;
    let state = state.clone();
    tokio::task::spawn_blocking(move || stage(&state, &name, |dest| std::fs::copy(&source, dest).map(|_| ())))
        .await
        .map_err(|e| AppError::msg(e.to_string()))?
}

/// Stages bytes from the clipboard. Like a download, the file is marked as coming from the
/// internet (Windows' Mark of the Web), since nothing says where the bytes came from: Office then
/// opens it in Protected View.
pub async fn stage_bytes(state: &Arc<AppState>, name: String, bytes: Vec<u8>) -> CmdResult<Attachment> {
    if bytes.len() as u64 > MAX_FILE_BYTES {
        bail_msg!("{name} is larger than 100 MB, the most a file can be.");
    }
    let _turn = READING.acquire().await.map_err(|e| AppError::msg(e.to_string()))?;
    let state = state.clone();
    tokio::task::spawn_blocking(move || {
        stage(&state, &name, |dest| {
            std::fs::write(dest, &bytes)?;
            mark_of_the_web(dest);
            Ok(())
        })
    })
    .await
    .map_err(|e| AppError::msg(e.to_string()))?
}

fn stage(state: &AppState, name: &str, write: impl FnOnce(&Path) -> std::io::Result<()>) -> CmdResult<Attachment> {
    let id = new_id();
    let name = safe_name(name);
    let dir = state.paths.staging_dir.join(&id);
    std::fs::create_dir_all(&dir)?;
    let dest = dir.join(&name);
    let result = write(&dest)
        .map_err(|e| AppError::msg(format!("{name} could not be copied: {e}")))
        .and_then(|()| read_into_db(state, &id, &name, &dest));
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&dir);
    }
    result
}

/// Reads a staged file and stores what was read.
fn read_into_db(state: &AppState, id: &str, name: &str, file: &Path) -> CmdResult<Attachment> {
    let size = std::fs::metadata(file)?.len();
    let read = demido_extract::extract(file).map_err(AppError::msg)?;
    let mut note = read.note.clone();
    let (width, height) = read.dimensions.unzip();
    let mime = match read.kind {
        Kind::Audio => audio_mime(&read.mime).to_string(),
        _ => read.mime.clone(),
    };
    // What a model is given of a picture or a sound, kept with the attachment so it never
    // changes, whatever later happens to the file in the workspace.
    let media: Option<(Vec<u8>, String)> = match read.kind {
        Kind::Image => match demido_extract::model_image(file, IMAGE_EDGE) {
            Ok(image) => Some((image.bytes, image.mime.to_string())),
            Err(e) => {
                note = note.or(Some(e));
                None
            }
        },
        Kind::Audio if size <= MAX_AUDIO_BYTES => Some((std::fs::read(file)?, mime.clone())),
        Kind::Audio => {
            note = note.or(Some(
                "This recording is too long to give to a model (over 10 MB). It stays in the workspace.".into(),
            ));
            None
        }
        _ => None,
    };
    let text = read.text.filter(|t| !t.trim().is_empty());
    let passages = text
        .as_deref()
        .map(|t| demido_extract::chunks(t, &read.page_starts, PASSAGE_CHARS, PASSAGE_OVERLAP))
        .unwrap_or_default();
    let attachment = Attachment {
        id: id.to_string(),
        chat_id: None,
        message_id: None,
        name: name.to_string(),
        stored: format!("{id}/{name}"),
        file: None,
        path: String::new(),
        mime,
        kind: read.kind,
        size,
        pages: read.pages,
        tokens: text.as_deref().map(|t| estimate_tokens(t) as u64),
        width,
        height,
        note,
        created_at: now_ms(),
    };
    state.db.insert_attachment(
        &attachment,
        AttachmentContent {
            text: text.as_deref(),
            media: media.as_ref().map(|(bytes, mime)| (bytes.as_slice(), mime.as_str())),
            chunks: &passages,
        },
    )?;
    if !passages.is_empty() {
        state.embedder.wake();
    }
    Ok(resolved(&state.paths, attachment))
}

/// One name for each sound format, whatever the detector or the browser called it.
pub(crate) fn audio_mime(mime: &str) -> &str {
    match mime {
        "audio/x-wav" | "audio/wave" | "audio/vnd.wave" => "audio/wav",
        "audio/mp3" | "audio/x-mp3" => "audio/mpeg",
        "audio/x-flac" => "audio/flac",
        "audio/x-aiff" => "audio/aiff",
        other => other,
    }
}

/// Removes a file from the composer before it was sent.
pub fn discard(state: &AppState, id: &str) -> CmdResult<()> {
    if state.db.delete_staged_attachment(id)? && is_id(id) {
        let _ = std::fs::remove_dir_all(state.paths.staging_dir.join(id));
    }
    Ok(())
}

/// Forgets whatever was left in the composer when the app last closed.
pub fn clear_staging(paths: &AppPaths, db: &Db) {
    if let Err(e) = db.delete_staged_attachments() {
        tracing::warn!("could not forget staged attachments: {e}");
    }
    if let Ok(entries) = std::fs::read_dir(&paths.staging_dir) {
        for entry in entries.flatten() {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

/// Checks that every id is a file waiting in the composer, before anything is created or moved.
pub fn check_staged(db: &Db, ids: &[String]) -> CmdResult<()> {
    if ids.len() > MAX_PER_MESSAGE {
        bail_msg!("A message can carry at most {MAX_PER_MESSAGE} files.");
    }
    for id in ids {
        match db.get_attachment(id)? {
            Some(a) if a.chat_id.is_none() => {}
            _ => bail_msg!("A file was removed before the message was sent. Add it again."),
        }
    }
    Ok(())
}

/// Moves staged files into the chat's workspace and links them to `message_id`, in the order
/// given. When one cannot be moved, the ones already moved go back to the composer, so the
/// person can send again.
pub fn take_for_message(
    state: &AppState,
    chat_id: &str,
    message_id: &str,
    ids: &[String],
) -> CmdResult<Vec<Attachment>> {
    check_staged(&state.db, ids)?;
    let uploads = state.paths.workspace(chat_id).join(UPLOADS);
    if !ids.is_empty() {
        std::fs::create_dir_all(&uploads)?;
    }
    let mut moved: Vec<(Attachment, PathBuf)> = Vec::with_capacity(ids.len());
    let result = (|| -> CmdResult<()> {
        for (position, id) in ids.iter().enumerate() {
            let Some(a) = state.db.get_attachment(id)? else {
                continue;
            };
            let from = state.paths.staging_dir.join(&a.stored);
            // A name the chat used before is not reused, even when that file was deleted since:
            // the workspace path identifies an attachment.
            let name = unique_name(&a.name, |candidate| {
                uploads.join(candidate).exists()
                    || state
                        .db
                        .attachment_at(chat_id, &format!("{UPLOADS}/{candidate}"))
                        .ok()
                        .flatten()
                        .is_some()
            });
            let to = uploads.join(&name);
            move_file(&from, &to).map_err(|e| AppError::msg(format!("{} could not be moved: {e}", a.name)))?;
            moved.push((a.clone(), to));
            state
                .db
                .link_attachment(&a.id, chat_id, message_id, &format!("{UPLOADS}/{name}"), position)?;
        }
        Ok(())
    })();
    if let Err(e) = result {
        for (a, to) in moved {
            let back = state.paths.staging_dir.join(&a.stored);
            if let Some(dir) = back.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = move_file(&to, &back);
            let _ = state.db.unlink_attachment(&a.id, &a.stored);
        }
        return Err(e);
    }
    let mut out = Vec::with_capacity(moved.len());
    for (a, _) in moved {
        if is_id(&a.id) {
            let _ = std::fs::remove_dir_all(state.paths.staging_dir.join(&a.id));
        }
        if let Some(sent) = state.db.get_attachment(&a.id)? {
            out.push(resolved(&state.paths, sent));
        }
    }
    Ok(out)
}

/// Renames a file, or copies it and removes the original where a rename cannot go.
fn move_file(from: &Path, to: &Path) -> std::io::Result<()> {
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    std::fs::copy(from, to)?;
    let _ = std::fs::remove_file(from);
    Ok(())
}

/// Fills in the absolute path the UI opens and previews.
pub fn resolved(paths: &AppPaths, mut a: Attachment) -> Attachment {
    let path = match &a.chat_id {
        Some(chat) => paths.workspace(chat).join(&a.stored),
        None => paths.staging_dir.join(&a.stored),
    };
    a.path = path.to_string_lossy().replace('/', std::path::MAIN_SEPARATOR_STR);
    a
}

/// Fills in the paths of every message's files.
pub fn resolve_messages(paths: &AppPaths, messages: &mut [crate::db::Message]) {
    for m in messages {
        for a in std::mem::take(&mut m.attachments) {
            m.attachments.push(resolved(paths, a));
        }
    }
}

/// Ids are generated here, but a staging folder is only ever removed for something shaped
/// like one, so a bad id can never name another folder.
fn is_id(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

/// Marks a file as downloaded from the internet (zone 3), as browsers do.
fn mark_of_the_web(path: &Path) {
    #[cfg(windows)]
    {
        let mut stream = path.as_os_str().to_owned();
        stream.push(":Zone.Identifier");
        if let Err(e) = std::fs::write(PathBuf::from(stream), "[ZoneTransfer]\r\nZoneId=3\r\n") {
            tracing::debug!("could not mark {} as coming from the internet: {e}", path.display());
        }
    }
    #[cfg(not(windows))]
    let _ = path;
}

/// A file name that is safe on every system: no folders, no characters Windows refuses, no
/// reserved device names, not too long.
pub fn safe_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or_default();
    let cleaned: String = base
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let cleaned = cleaned
        .trim()
        .trim_end_matches(['.', ' '])
        .trim_start_matches('.')
        .to_string();
    let cleaned = if cleaned.is_empty() {
        "file".to_string()
    } else {
        cleaned
    };
    let (stem, ext) = split_ext(&cleaned);
    const RESERVED: &[&str] = &[
        "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8", "com9", "lpt1",
        "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
    ];
    let stem = if RESERVED.contains(&stem.to_ascii_lowercase().as_str()) {
        format!("_{stem}")
    } else {
        stem.to_string()
    };
    let stem: String = stem.chars().take(120).collect();
    let ext: String = ext.chars().take(16).collect();
    if ext.is_empty() { stem } else { format!("{stem}.{ext}") }
}

fn split_ext(name: &str) -> (&str, &str) {
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && !ext.contains(' ') => (stem, ext),
        _ => (name, ""),
    }
}

/// `name`, or `name (2)`, `name (3)`... when `taken` says it is in use.
fn unique_name(name: &str, taken: impl Fn(&str) -> bool) -> String {
    if !taken(name) {
        return name.to_string();
    }
    let (stem, ext) = split_ext(name);
    (2..)
        .map(|n| {
            if ext.is_empty() {
                format!("{stem} ({n})")
            } else {
                format!("{stem} ({n}).{ext}")
            }
        })
        .find(|candidate| !taken(candidate))
        .unwrap_or_else(|| name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_safe_everywhere() {
        assert_eq!(safe_name("report.pdf"), "report.pdf");
        assert_eq!(safe_name("C:\\Users\\x\\a:b?.txt"), "a_b_.txt");
        assert_eq!(safe_name("../../etc/passwd"), "passwd");
        assert_eq!(safe_name("CON.txt"), "_CON.txt");
        assert_eq!(safe_name("  .hidden  "), "hidden");
        assert_eq!(safe_name("..."), "file");
        assert_eq!(safe_name("notes."), "notes");
        assert!(safe_name(&"a".repeat(300)).chars().count() <= 120);
    }

    #[test]
    fn names_do_not_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let on_disk = |n: &str| dir.path().join(n).exists();
        assert_eq!(unique_name("a.pdf", on_disk), "a.pdf");
        std::fs::write(dir.path().join("a.pdf"), b"x").unwrap();
        assert_eq!(unique_name("a.pdf", on_disk), "a (2).pdf");
        std::fs::write(dir.path().join("a (2).pdf"), b"x").unwrap();
        assert_eq!(unique_name("a.pdf", on_disk), "a (3).pdf");
        std::fs::write(dir.path().join("Makefile"), b"x").unwrap();
        assert_eq!(unique_name("Makefile", on_disk), "Makefile (2)");
        // A name recorded for the chat counts too, file or no file.
        assert_eq!(unique_name("gone.pdf", |n| n == "gone.pdf"), "gone (2).pdf");
    }

    #[test]
    fn only_ids_name_staging_folders() {
        assert!(is_id("0199a1b2-7c3d-7e4f-8a9b-0c1d2e3f4a5b"));
        assert!(!is_id(".."));
        assert!(!is_id(""));
        assert!(!is_id("a/b"));
    }

    #[test]
    fn sound_formats_have_one_name() {
        assert_eq!(audio_mime("audio/x-wav"), "audio/wav");
        assert_eq!(audio_mime("audio/mp3"), "audio/mpeg");
        assert_eq!(audio_mime("audio/ogg"), "audio/ogg");
    }
}
