//! The tools that read and change files, inside one folder and nowhere else.
//!
//! A session has a folder. Every path a model names is taken as a path inside it; one that leads out, by
//! `..`, by an absolute path or through a link, is refused before anything is opened. That check is in
//! code, here, and does not depend on what the model was told.
//!
//! What a tool gives back is bounded, and says so when it has left something out: a model's room is small,
//! and a file that fills it helps nobody.

use std::fs;
use std::path::{Component, Path, PathBuf};

/// A file is given this many lines at a time, and no more than this many bytes.
const MOST_LINES: usize = 300;
const MOST_BYTES: usize = 20_000;
/// A folder is listed up to this many entries; a search gives up to this many lines.
const MOST_ENTRIES: usize = 200;
const MOST_MATCHES: usize = 50;
/// A search does not open files larger than this, or go deeper than this.
const LARGEST_SEARCHED: u64 = 1_000_000;
const DEEPEST: usize = 12;
/// A file a tool writes is no larger than this.
const LARGEST_WRITTEN: usize = 1_000_000;
/// Folders a search does not go into: what programs make, not what people write.
const NOT_SEARCHED: [&str; 8] = [".git", "node_modules", "target", ".venv", "venv", "__pycache__", "dist", ".cache"];

/// A change a tool made to a file, with what was there before, so that it can be undone.
#[derive(Clone, Debug, PartialEq)]
pub struct Changed {
    /// The file, as a path inside the folder.
    pub path: String,
    /// What the file held before. `None`: it did not exist.
    pub before: Option<Vec<u8>>,
}

/// What a refusal for leaving the folder says. A use that ends this way is not followed by another try.
pub const OUTSIDE: &str = "is outside this session's folder";

/// A path a model named, as a real path inside the folder. `Err` when it leads outside, in words.
pub fn inside(folder: &Path, named: &str) -> Result<PathBuf, String> {
    let named = named.trim().trim_matches(['"', '\'', '`']);
    let outside = || format!("\"{named}\" {OUTSIDE}. Only files inside the folder can be used");
    let folder = folder.canonicalize().map_err(|e| format!("the session's folder cannot be opened: {e}"))?;
    let given = Path::new(named);
    // An absolute path is fine when it is inside the folder: models repeat paths as they were shown them.
    let relative = if given.is_absolute() { given.strip_prefix(&folder).map_err(|_| outside())?.to_path_buf() } else { given.to_path_buf() };
    let mut path = folder.clone();
    for part in relative.components() {
        match part {
            Component::Normal(name) => path.push(name),
            Component::CurDir => {}
            Component::ParentDir => {
                if path == folder {
                    return Err(outside());
                }
                path.pop();
            }
            Component::RootDir | Component::Prefix(_) => return Err(outside()),
        }
    }
    // A link inside the folder may lead out of it. What exists of the path is followed, and must still be inside.
    let mut exists = path.clone();
    while !exists.exists() {
        if !exists.pop() {
            break;
        }
    }
    let real = exists.canonicalize().map_err(|e| format!("\"{named}\" cannot be opened: {e}"))?;
    if !real.starts_with(&folder) {
        return Err(outside());
    }
    Ok(path)
}

/// A path as the model is shown it: from the folder, with forward slashes.
fn shown(folder: &Path, path: &Path) -> String {
    let folder = folder.canonicalize().unwrap_or_else(|_| folder.to_path_buf());
    let within = path.strip_prefix(&folder).unwrap_or(path);
    let text = within.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/");
    if text.is_empty() { ".".into() } else { text }
}

fn sized(bytes: u64) -> String {
    match bytes {
        0..=999 => format!("{bytes} bytes"),
        1_000..=999_999 => format!("{:.1} kB", bytes as f64 / 1e3),
        _ => format!("{:.1} MB", bytes as f64 / 1e6),
    }
}

/// Text, or `Err` for a file that is not text. A file is text when it is valid UTF-8 without a zero byte.
fn text_of(path: &Path, named: &str) -> Result<String, String> {
    let meta = fs::metadata(path).map_err(|_| format!("there is no file \"{named}\""))?;
    if meta.is_dir() {
        return Err(format!("\"{named}\" is a folder, not a file. list_files shows what is in it"));
    }
    let bytes = fs::read(path).map_err(|e| format!("\"{named}\" cannot be read: {e}"))?;
    if bytes.contains(&0) {
        return Err(format!("\"{named}\" is not a text file ({})", sized(meta.len())));
    }
    String::from_utf8(bytes).map_err(|_| format!("\"{named}\" is not a text file ({})", sized(meta.len())))
}

/// Every file under the folder, as (path from the folder, size), near ones before deep ones. Folders that
/// programs fill are named and not gone into. Stops at `most`; the second value is how many more there are.
fn files_under(folder: &Path, most: usize) -> (Vec<(String, u64)>, usize, Vec<String>) {
    let root = folder.canonicalize().unwrap_or_else(|_| folder.to_path_buf());
    let (mut found, mut more, mut skipped) = (Vec::new(), 0usize, Vec::new());
    let mut waiting = std::collections::VecDeque::from([(root.clone(), 0usize)]);
    while let Some((dir, depth)) = waiting.pop_front() {
        let mut entries: Vec<_> = fs::read_dir(&dir).into_iter().flatten().filter_map(|e| e.ok()).collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let (path, name) = (entry.path(), entry.file_name().to_string_lossy().into_owned());
            // A link is not followed: it may lead out of the folder, or round in a circle.
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_dir() {
                if NOT_SEARCHED.contains(&name.as_str()) {
                    skipped.push(format!("{}/", shown(&root, &path)));
                } else if depth < DEEPEST {
                    waiting.push_back((path, depth + 1));
                }
            } else if kind.is_file() {
                if found.len() < most {
                    // A name is text from outside too: nothing in it may break out of its line.
                    let name: String = shown(&root, &path).chars().filter(|c| !c.is_control()).collect();
                    found.push((name, entry.metadata().map_or(0, |m| m.len())));
                } else {
                    more += 1;
                }
            }
        }
    }
    (found, more, skipped)
}

/// The folder at a glance, for a model that has not looked yet: the paths of its files, near ones first.
/// A model that is not shown this does not connect "my todo list" with a file called todo.txt, and makes
/// a list up. Tried on two real models; both did. Files only, as full paths: shown the name of a folder,
/// the same models passed it to whatever tool they picked.
pub fn glance(folder: &Path) -> String {
    let (files, more, _) = files_under(folder, 30);
    let names: Vec<&str> = files.iter().map(|(name, _)| name.as_str()).collect();
    match (names.is_empty(), more) {
        (true, _) => "The folder is empty.".into(),
        (false, 0) => format!("Files in the folder: {}", names.join(", ")),
        (false, more) => format!("Files in the folder: {}, and {more} more (list_files shows all).", names.join(", ")),
    }
}

/// Every file in the folder and the folders inside it, with its size.
pub fn list(folder: &Path) -> Result<String, String> {
    if !folder.is_dir() {
        return Err("the session's folder is not there any more".into());
    }
    let (files, more, skipped) = files_under(folder, MOST_ENTRIES);
    if files.is_empty() && skipped.is_empty() {
        return Ok("The folder is empty.".into());
    }
    let mut out: Vec<String> = files.iter().map(|(name, size)| format!("{name}  ({})", sized(*size))).collect();
    out.extend(skipped.iter().map(|name| format!("{name}  (made by programs; not listed)")));
    if more > 0 {
        out.push(format!("({more} more files are not shown.)"));
    }
    Ok(out.join("\n"))
}

/// The text of a file, from a line on. Long files come a piece at a time, and say where the next piece starts.
pub fn read(folder: &Path, named: &str, from_line: Option<i64>) -> Result<String, String> {
    let text = text_of(&inside(folder, named)?, named)?;
    if text.is_empty() {
        return Ok(format!("The file {named} is empty."));
    }
    let lines: Vec<&str> = text.lines().collect();
    // A line past the end is not a mistake worth a turn: the file is given from its start, and says so.
    // (A real model asked for line 1000000000000000 of a file of three.)
    let asked_from = from_line.unwrap_or(1).max(1) as usize;
    let from = if asked_from > lines.len() { 1 } else { asked_from };
    let (mut out, mut last) = (String::new(), from - 1);
    for line in lines.iter().skip(from - 1).take(MOST_LINES) {
        if !out.is_empty() && out.len() + line.len() > MOST_BYTES {
            break;
        }
        // One line longer than everything allowed is cut, and said to be.
        if line.len() > MOST_BYTES {
            let cut: String = line.chars().take(MOST_BYTES / 4).collect();
            out.push_str(&format!("{cut}… (this line is {} characters long; the rest is not shown)\n", line.chars().count()));
        } else {
            out.push_str(line);
            out.push('\n');
        }
        last += 1;
    }
    if asked_from != from {
        out.push_str(&format!("(The file has {} lines, so there is no line {asked_from}. This is the file from its start.)\n", lines.len()));
    }
    if from > 1 || last < lines.len() {
        out.push_str(&format!("(Lines {from} to {last} of {}.", lines.len()));
        if last < lines.len() {
            out.push_str(&format!(" For more, ask again with from_line: {}.", last + 1));
        }
        out.push(')');
    }
    Ok(out.trim_end().to_string())
}

/// The lines that contain a text, in every text file in the folder. Capital letters do not matter.
pub fn search(folder: &Path, wanted: &str) -> Result<String, String> {
    let needle = wanted.to_lowercase();
    let root = folder.canonicalize().map_err(|_| "the session's folder is not there any more".to_string())?;
    let (files, _, _) = files_under(folder, 5_000);
    let (mut found, mut more, mut looked_at) = (Vec::new(), 0usize, 0usize);
    for (name, size) in files {
        if size > LARGEST_SEARCHED {
            continue;
        }
        let Ok(bytes) = fs::read(root.join(&name)) else { continue };
        let Ok(text) = String::from_utf8(bytes) else { continue };
        looked_at += 1;
        for (n, line) in text.lines().enumerate() {
            if line.to_lowercase().contains(&needle) {
                if found.len() < MOST_MATCHES {
                    let line: String = line.trim().chars().take(200).collect();
                    found.push(format!("{name}:{}: {line}", n + 1));
                } else {
                    more += 1;
                }
            }
        }
    }
    if found.is_empty() {
        return Ok(format!("\"{wanted}\" is in none of the {looked_at} text files."));
    }
    let mut out = found.join("\n");
    if more > 0 {
        out.push_str(&format!("\n({more} more lines are not shown. Search for something narrower.)"));
    }
    Ok(out)
}

/// Whether two names are the same file in the folder.
pub fn same_file(folder: &Path, one: &str, other: &str) -> bool {
    matches!((inside(folder, one), inside(folder, other)), (Ok(a), Ok(b)) if a == b)
}

/// Creates a file, or replaces all of it. Folders on the way are made.
///
/// A file that is there is replaced only if the model has read it (`seen`: the files it read for this
/// request). Asked to "add a line to todo.txt", two real models wrote a new file holding only that line
/// and reported success: the list was gone.
pub fn write(folder: &Path, named: &str, content: &str, seen: &[String]) -> Result<(String, Changed), String> {
    if content.len() > LARGEST_WRITTEN {
        return Err(format!("that is {} of text. A file written here can be {} at most", sized(content.len() as u64), sized(LARGEST_WRITTEN as u64)));
    }
    let path = inside(folder, named)?;
    if path.is_dir() {
        return Err(format!("\"{named}\" is a folder. Give the file a name inside it"));
    }
    let before = fs::read(&path).ok();
    if before.as_ref().is_some_and(|old| !old.is_empty()) && !seen.iter().any(|read| same_file(folder, read, named)) {
        return Err(format!(
            "{named} already has text in it, and write_file would replace all of it. To add to its end, use add_to_file. To change part of it, use edit_file. To replace all of it, read it first"
        ));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("the folder for \"{named}\" cannot be made: {e}"))?;
    }
    fs::write(&path, content).map_err(|e| format!("\"{named}\" cannot be written: {e}"))?;
    let name = shown(folder, &path);
    let said = match &before {
        Some(old) => format!("Replaced all of {name}. It was {}, and is now {}.", sized(old.len() as u64), sized(content.len() as u64)),
        None => format!("Created {name} ({}).", sized(content.len() as u64)),
    };
    Ok((said, Changed { path: name, before }))
}

/// Adds text at the end of a file, on a line of its own. A file that is not there is made.
pub fn add(folder: &Path, named: &str, text: &str) -> Result<(String, Changed), String> {
    let path = inside(folder, named)?;
    if path.is_dir() {
        return Err(format!("\"{named}\" is a folder. Give the file a name inside it"));
    }
    let before = match fs::read(&path) {
        Ok(bytes) => Some(String::from_utf8(bytes).map_err(|_| format!("\"{named}\" is not a text file"))?),
        Err(_) => None,
    };
    let mut after = before.clone().unwrap_or_default();
    if !after.is_empty() && !after.ends_with('\n') {
        after.push('\n');
    }
    after.push_str(text);
    if !after.ends_with('\n') {
        after.push('\n');
    }
    if after.len() > LARGEST_WRITTEN {
        return Err(format!("the file would be {}. A file written here can be {} at most", sized(after.len() as u64), sized(LARGEST_WRITTEN as u64)));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("the folder for \"{named}\" cannot be made: {e}"))?;
    }
    fs::write(&path, &after).map_err(|e| format!("\"{named}\" cannot be written: {e}"))?;
    let name = shown(folder, &path);
    let lines = text.lines().count().max(1);
    let said = match &before {
        Some(_) => format!("Added {lines} {} at the end of {name}. Nothing that was there was changed.", if lines == 1 { "line" } else { "lines" }),
        None => format!("Created {name} with {lines} {}.", if lines == 1 { "line" } else { "lines" }),
    };
    Ok((said, Changed { path: name, before: before.map(String::into_bytes) }))
}

/// Changes one part of a file: the exact text to find, which must be there once, and what goes in its place.
pub fn edit(folder: &Path, named: &str, find: &str, replace: &str) -> Result<(String, Changed), String> {
    let path = inside(folder, named)?;
    let text = text_of(&path, named)?;
    let times = text.matches(find).count();
    if times == 0 {
        // Often only the spaces at the ends differ. Say what would have matched.
        let hint = if text.contains(find.trim()) && find.trim() != find { " The same text without the spaces around it is there" } else { "" };
        return Err(format!("the text to find is not in {named}. It must match exactly, with its spaces and line breaks.{hint}"));
    }
    if times > 1 {
        return Err(format!("the text to find is in {named} {times} times. Give more of what is around it, so that it is there once"));
    }
    let after = text.replacen(find, replace, 1);
    if after.len() > LARGEST_WRITTEN {
        return Err(format!("the file would be {}. A file written here can be {} at most", sized(after.len() as u64), sized(LARGEST_WRITTEN as u64)));
    }
    fs::write(&path, &after).map_err(|e| format!("\"{named}\" cannot be written: {e}"))?;
    let name = shown(folder, &path);
    Ok((format!("Changed {name} in one place."), Changed { path: name, before: Some(text.into_bytes()) }))
}

/// Puts a file back as it was before a change: its old contents, or gone if it did not exist.
pub fn put_back(folder: &Path, changed: &Changed) -> Result<(), String> {
    let path = inside(folder, &changed.path)?;
    match &changed.before {
        Some(before) => fs::write(&path, before).map_err(|e| format!("\"{}\" cannot be written: {e}", changed.path)),
        None => match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("\"{}\" cannot be removed: {e}", changed.path)),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("demido-files-{name}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(dir.join("notes/old")).unwrap();
        fs::write(dir.join("todo.txt"), "Buy stamps.\nCall Mira about Friday.\nRenew the domain by 3 December.\n").unwrap();
        fs::write(dir.join("notes/plan.md"), "# Plan\n\nShip on Friday.\n").unwrap();
        fs::write(dir.join("notes/old/2025.md"), "Nothing shipped on a friday.\n").unwrap();
        fs::write(dir.join("picture.bin"), [0u8, 159, 146, 150]).unwrap();
        dir
    }

    #[test]
    fn a_path_stays_inside_the_folder_however_it_is_written() {
        let dir = folder("inside");
        let real = dir.canonicalize().unwrap();
        assert_eq!(inside(&dir, "todo.txt").unwrap(), real.join("todo.txt"));
        assert_eq!(inside(&dir, " ./notes/../todo.txt ").unwrap(), real.join("todo.txt"));
        assert_eq!(inside(&dir, "\"notes/plan.md\"").unwrap(), real.join("notes/plan.md"));
        assert_eq!(inside(&dir, "new/deep/file.txt").unwrap(), real.join("new/deep/file.txt"), "a file that is not there yet");
        // The same path written from the root is the same file.
        assert_eq!(inside(&dir, real.join("todo.txt").to_str().unwrap()).unwrap(), real.join("todo.txt"));
        for out in ["../secret.txt", "notes/../../secret.txt", "/etc/passwd", "..", "notes/../.."] {
            assert!(inside(&dir, out).unwrap_err().contains("outside this session's folder"), "{out}");
        }
        fs::remove_dir_all(dir).ok();
    }

    #[cfg(unix)]
    #[test]
    fn a_link_that_leads_out_of_the_folder_is_not_followed() {
        let dir = folder("link");
        let outside = std::env::temp_dir().join(format!("demido-files-outside-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("secret.txt"), "the key").unwrap();
        std::os::unix::fs::symlink(&outside, dir.join("way-out")).unwrap();
        std::os::unix::fs::symlink(outside.join("secret.txt"), dir.join("looks-harmless.txt")).unwrap();
        assert!(read(&dir, "way-out/secret.txt", None).unwrap_err().contains("outside"));
        assert!(read(&dir, "looks-harmless.txt", None).unwrap_err().contains("outside"));
        assert!(write(&dir, "way-out/new.txt", "x", &[]).unwrap_err().contains("outside"));
        assert!(!outside.join("new.txt").exists());
        // A search does not follow it either.
        assert!(search(&dir, "the key").unwrap().contains("in none of"));
        fs::remove_dir_all(dir).ok();
        fs::remove_dir_all(outside).ok();
    }

    #[test]
    fn a_folder_is_listed_and_a_file_is_read_a_piece_at_a_time() {
        let dir = folder("read");
        // Every file, near ones before deep ones, with nothing to pass and so nothing to get wrong.
        assert_eq!(list(&dir).unwrap(), "picture.bin  (4 bytes)\ntodo.txt  (68 bytes)\nnotes/plan.md  (24 bytes)\nnotes/old/2025.md  (29 bytes)");
        assert_eq!(glance(&dir), "Files in the folder: picture.bin, todo.txt, notes/plan.md, notes/old/2025.md");
        fs::create_dir_all(dir.join("node_modules/x")).unwrap();
        fs::write(dir.join("node_modules/x/index.js"), "x").unwrap();
        assert!(list(&dir).unwrap().ends_with("node_modules/  (made by programs; not listed)"));
        assert!(!glance(&dir).contains("node_modules"));
        let empty = dir.join("notes/empty");
        fs::create_dir(&empty).unwrap();
        assert_eq!((list(&empty).unwrap().as_str(), glance(&empty).as_str()), ("The folder is empty.", "The folder is empty."));
        for i in 0..40 {
            fs::write(empty.join(format!("f{i:02}.txt")), "").unwrap();
        }
        assert!(glance(&empty).ends_with("f29.txt, and 10 more (list_files shows all)."));

        assert_eq!(read(&dir, "todo.txt", None).unwrap(), "Buy stamps.\nCall Mira about Friday.\nRenew the domain by 3 December.");
        assert_eq!(read(&dir, "todo.txt", Some(2)).unwrap(), "Call Mira about Friday.\nRenew the domain by 3 December.\n(Lines 2 to 3 of 3.)");
        assert_eq!(
            read(&dir, "todo.txt", Some(1_000_000_000_000_000)).unwrap(),
            "Buy stamps.\nCall Mira about Friday.\nRenew the domain by 3 December.\n(The file has 3 lines, so there is no line 1000000000000000. This is the file from its start.)"
        );
        assert!(read(&dir, "nope.txt", None).unwrap_err().contains("no file"));
        assert!(read(&dir, "notes", None).unwrap_err().contains("is a folder"));
        assert!(read(&dir, "picture.bin", None).unwrap_err().contains("not a text file"));
        // A long file comes in pieces, and says where the next one starts.
        let long: String = (1..=1000).map(|i| format!("line {i}\n")).collect();
        fs::write(dir.join("long.txt"), long).unwrap();
        let first = read(&dir, "long.txt", None).unwrap();
        assert!(first.starts_with("line 1\n") && first.ends_with("(Lines 1 to 300 of 1000. For more, ask again with from_line: 301.)"), "{}", &first[first.len() - 80..]);
        assert!(read(&dir, "long.txt", Some(901)).unwrap().ends_with("line 1000\n(Lines 901 to 1000 of 1000.)"));
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_search_finds_lines_and_says_where() {
        let dir = folder("search");
        assert_eq!(search(&dir, "FRIDAY").unwrap(), "todo.txt:2: Call Mira about Friday.\nnotes/plan.md:3: Ship on Friday.\nnotes/old/2025.md:1: Nothing shipped on a friday.");
        assert_eq!(search(&dir, "invoice").unwrap(), "\"invoice\" is in none of the 3 text files.");
        // What programs make is not searched.
        fs::create_dir_all(dir.join("node_modules/x")).unwrap();
        fs::write(dir.join("node_modules/x/index.js"), "friday()").unwrap();
        assert!(!search(&dir, "friday").unwrap().contains("node_modules"));
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_file_is_written_and_changed_and_what_was_there_is_kept() {
        let dir = folder("write");
        let (said, made) = write(&dir, "notes/new/idea.md", "A shop for stamps.\n", &[]).unwrap();
        assert_eq!((said.as_str(), &made), ("Created notes/new/idea.md (19 bytes).", &Changed { path: "notes/new/idea.md".into(), before: None }));
        // A file with text in it is not replaced whole by a model that has not read it.
        let unread = write(&dir, "todo.txt", "Nothing to do.\n", &["notes/plan.md".into()]).unwrap_err();
        assert!(unread.starts_with("todo.txt already has text in it") && unread.contains("add_to_file") && unread.ends_with("read it first"), "{unread}");
        assert!(fs::read_to_string(dir.join("todo.txt")).unwrap().starts_with("Buy stamps."));
        let (said, replaced) = write(&dir, "todo.txt", "Nothing to do.\n", &["./todo.txt".into()]).unwrap();
        assert_eq!(said, "Replaced all of todo.txt. It was 68 bytes, and is now 15 bytes.");
        assert!(replaced.before.as_deref().unwrap().starts_with(b"Buy stamps."));
        assert!(write(&dir, "notes", "x", &[]).unwrap_err().contains("is a folder"));
        fs::write(dir.join("blank.txt"), "").unwrap();
        assert!(write(&dir, "blank.txt", "x", &[]).is_ok(), "an empty file has nothing to lose");

        let (said, changed) = edit(&dir, "notes/plan.md", "Friday", "Monday").unwrap();
        assert_eq!(said, "Changed notes/plan.md in one place.");
        assert_eq!(fs::read_to_string(dir.join("notes/plan.md")).unwrap(), "# Plan\n\nShip on Monday.\n");
        assert!(edit(&dir, "notes/plan.md", "Tuesday", "x").unwrap_err().contains("is not in notes/plan.md"));
        assert!(edit(&dir, "notes/plan.md", " Monday. ", "x").unwrap_err().ends_with("The same text without the spaces around it is there"));
        fs::write(dir.join("twice.txt"), "one two one").unwrap();
        assert!(edit(&dir, "twice.txt", "one", "three").unwrap_err().contains("2 times"));
        assert_eq!(fs::read_to_string(dir.join("twice.txt")).unwrap(), "one two one", "nothing is changed when the change is refused");

        // Adding to a file leaves what was there, and starts on a line of its own.
        fs::write(dir.join("list.txt"), "one\ntwo").unwrap();
        let (said, added) = add(&dir, "list.txt", "three").unwrap();
        assert_eq!(said, "Added 1 line at the end of list.txt. Nothing that was there was changed.");
        assert_eq!(fs::read_to_string(dir.join("list.txt")).unwrap(), "one\ntwo\nthree\n");
        assert_eq!(added.before.as_deref(), Some(b"one\ntwo".as_slice()));
        assert_eq!(add(&dir, "fresh.txt", "a\nb\n").unwrap().0, "Created fresh.txt with 2 lines.");
        assert_eq!(fs::read_to_string(dir.join("fresh.txt")).unwrap(), "a\nb\n");
        assert!(add(&dir, "picture.bin", "x").unwrap_err().contains("not a text file"));

        // Each change can be put back: the old contents, or gone if the file was new.
        put_back(&dir, &changed).unwrap();
        assert_eq!(fs::read_to_string(dir.join("notes/plan.md")).unwrap(), "# Plan\n\nShip on Friday.\n");
        put_back(&dir, &replaced).unwrap();
        assert!(fs::read_to_string(dir.join("todo.txt")).unwrap().starts_with("Buy stamps."));
        put_back(&dir, &made).unwrap();
        assert!(!dir.join("notes/new/idea.md").exists());
        put_back(&dir, &made).unwrap();
        fs::remove_dir_all(dir).ok();
    }
}
