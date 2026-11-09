//! Finding MetaTrader 5 terminals and the accounts they logged into, from their files only: nothing
//! is started. On Windows, in Program Files and in the folders the terminals keep for each user. On
//! Linux, in Wine prefixes: `~/.mt5` (where MetaQuotes' installer script puts it), `~/.wine`,
//! `~/.local/share/wineprefixes/*`, `$WINEPREFIX`, and any prefix Wine's menu entries run in.
//!
//! A terminal keeps its accounts and history in its data folder: the install folder when it runs
//! portable, otherwise `AppData\Roaming\MetaQuotes\Terminal\<id>`, whose `origin.txt` names the
//! install folder (a `portable.txt` beside it says the data is in the install folder instead).
//! Each account it logged into has `Bases\<server>\trades\<login>`; `Config\common.ini` names the
//! one it was last on and its "Max bars in chart".

use std::path::{Path, PathBuf};

use serde::Serialize;

const EXE: &str = "terminal64.exe";

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Terminal {
    /// terminal64.exe on this computer: what the settings name it by.
    pub path: PathBuf,
    /// The same file as Windows programs see it (under Wine, from inside its prefix).
    pub windows_path: String,
    /// The install folder's name: "MetaTrader 5", or a broker's own.
    pub name: String,
    /// Linux: the Wine prefix it is installed in.
    pub prefix: Option<PathBuf>,
    /// Where it keeps its accounts and history.
    pub data_dir: PathBuf,
    /// Its data is in its install folder, so Demido starts it with `/portable`.
    pub portable: bool,
    /// The accounts it has logged into, the one it was last on first.
    pub accounts: Vec<Account>,
    /// Its "Max bars in chart": how much 1-minute history it serves.
    pub max_bars: Option<u64>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub login: u64,
    pub server: String,
    /// The terminal was last logged into this one.
    pub last: bool,
}

/// Every terminal found, each once.
pub fn terminals() -> Vec<Terminal> {
    let mut found: Vec<Terminal> = Vec::new();
    for root in roots() {
        for exe in root.installs() {
            if let Some(t) = root.terminal(&exe)
                && !found.iter().any(|f| same_file(&f.path, &t.path))
            {
                found.push(t);
            }
        }
    }
    found
}

/// The terminal at `path` (a terminal64.exe the settings name), if it is one.
pub fn terminal_at(path: &Path) -> Option<Terminal> {
    let name = path.file_name()?.to_str()?;
    if !name.eq_ignore_ascii_case(EXE) || !path.is_file() {
        return None;
    }
    root_of(path)?.terminal(path)
}

fn same_file(a: &Path, b: &Path) -> bool {
    a == b || matches!((a.canonicalize(), b.canonicalize()), (Ok(a), Ok(b)) if a == b)
}

// ---------------------------------------------------------------------------------------------
// Where to look

/// A Windows file system: this computer's, or a Wine prefix's.
struct Root {
    /// The Wine prefix, under Wine.
    prefix: Option<PathBuf>,
    drives: Drives,
    program_files: Vec<PathBuf>,
    /// Each user's `AppData\Roaming`.
    roaming: Vec<PathBuf>,
}

#[cfg(windows)]
fn roots() -> Vec<Root> {
    vec![windows_root()]
}

#[cfg(windows)]
fn windows_root() -> Root {
    let mut program_files: Vec<PathBuf> = Vec::new();
    for var in ["ProgramW6432", "ProgramFiles"] {
        if let Some(dir) = std::env::var_os(var).map(PathBuf::from)
            && !program_files.contains(&dir)
        {
            program_files.push(dir);
        }
    }
    Root {
        prefix: None,
        drives: Drives::Native,
        program_files,
        roaming: std::env::var_os("APPDATA").map(PathBuf::from).into_iter().collect(),
    }
}

#[cfg(windows)]
fn root_of(_path: &Path) -> Option<Root> {
    Some(windows_root())
}

#[cfg(not(windows))]
fn roots() -> Vec<Root> {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return Vec::new();
    };
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local").join("share"));
    let env_prefix = std::env::var_os("WINEPREFIX").map(PathBuf::from);
    wine_prefixes(&home, &data_home, env_prefix)
        .iter()
        .map(|p| wine_root(p))
        .collect()
}

#[cfg(not(windows))]
fn root_of(path: &Path) -> Option<Root> {
    let drive_c = path
        .ancestors()
        .find(|a| a.file_name().is_some_and(|n| n == "drive_c"))?;
    Some(wine_root(drive_c.parent()?))
}

#[cfg(not(windows))]
fn wine_root(prefix: &Path) -> Root {
    let prefix = prefix.canonicalize().unwrap_or_else(|_| prefix.to_path_buf());
    let drive_c = prefix.join("drive_c");
    let program_files = find(&drive_c, "Program Files").into_iter().collect();
    let mut roaming = Vec::new();
    if let Some(users) = find(&drive_c, "users")
        && let Ok(entries) = std::fs::read_dir(users)
    {
        for user in entries.flatten().map(|e| e.path()) {
            // Wine 6 and later; older prefixes kept the Windows XP name.
            if let Some(dir) = find_path(&user, &["AppData", "Roaming"]).or_else(|| find(&user, "Application Data")) {
                roaming.push(dir);
            }
        }
    }
    Root {
        drives: Drives::wine(&prefix),
        prefix: Some(prefix),
        program_files,
        roaming,
    }
}

/// The Wine prefixes that exist (they hold `drive_c`), each once.
#[cfg(not(windows))]
fn wine_prefixes(home: &Path, data_home: &Path, env_prefix: Option<PathBuf>) -> Vec<PathBuf> {
    let mut candidates: Vec<PathBuf> = env_prefix.into_iter().collect();
    candidates.push(home.join(".mt5"));
    candidates.push(home.join(".wine"));
    if let Ok(entries) = std::fs::read_dir(data_home.join("wineprefixes")) {
        candidates.extend(entries.flatten().map(|e| e.path()));
    }
    let applications = data_home.join("applications");
    let entries = walkdir::WalkDir::new(applications.join("wine"))
        .max_depth(5)
        .into_iter()
        .chain(walkdir::WalkDir::new(&applications).max_depth(1))
        .flatten()
        .filter(|e| e.file_type().is_file() && e.path().extension().is_some_and(|x| x == "desktop"));
    for entry in entries {
        let Ok(text) = std::fs::read_to_string(entry.path()) else {
            continue;
        };
        if !text.contains("wine") {
            continue;
        }
        candidates.extend(desktop_prefixes(&text));
    }
    let mut prefixes: Vec<PathBuf> = Vec::new();
    for candidate in candidates {
        if !candidate.join("drive_c").is_dir() {
            continue;
        }
        let real = candidate.canonicalize().unwrap_or(candidate);
        if !prefixes.contains(&real) {
            prefixes.push(real);
        }
    }
    prefixes
}

/// The prefixes a desktop entry names: its command's `WINEPREFIX=` (Wine quotes it either as
/// `"WINEPREFIX=/x"` or `WINEPREFIX="/x"`), and the prefix its working folder is in.
#[cfg(not(windows))]
fn desktop_prefixes(text: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for line in text.lines() {
        if let Some(exec) = line.strip_prefix("Exec=")
            && let Some(at) = exec.find("WINEPREFIX=")
        {
            let rest = &exec[at + "WINEPREFIX=".len()..];
            let value = if exec[..at].ends_with('"') {
                rest.split('"').next()
            } else if let Some(quoted) = rest.strip_prefix('"') {
                quoted.split('"').next()
            } else {
                rest.split_whitespace().next()
            };
            if let Some(value) = value.filter(|v| !v.is_empty()) {
                out.push(PathBuf::from(value));
            }
        } else if let Some(dir) = line.strip_prefix("Path=")
            && let Some(at) = dir.find("/drive_c")
        {
            out.push(PathBuf::from(&dir[..at]));
        }
    }
    out
}

/// How Windows paths map to this computer's.
enum Drives {
    #[cfg_attr(not(windows), allow(dead_code))]
    Native,
    /// A Wine prefix: drive letters and the folders they stand for (`dosdevices`).
    Wine(Vec<(char, PathBuf)>),
}

impl Drives {
    fn wine(prefix: &Path) -> Self {
        let dev = prefix.join("dosdevices");
        let mut letters = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&dev) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_ascii_uppercase();
                let mut chars = name.chars();
                // "c:", not "c::" (the raw device) or "com1".
                let (Some(letter), Some(':'), None) = (chars.next(), chars.next(), chars.next()) else {
                    continue;
                };
                if !letter.is_ascii_alphabetic() {
                    continue;
                }
                if let Ok(target) = std::fs::read_link(entry.path())
                    && let Ok(target) = dev.join(target).canonicalize()
                {
                    letters.push((letter, target));
                }
            }
        }
        if letters.is_empty() {
            // What a new prefix has.
            if let Ok(c) = prefix.join("drive_c").canonicalize() {
                letters.push(('C', c));
            }
            letters.push(('Z', PathBuf::from("/")));
        }
        letters.sort();
        Drives::Wine(letters)
    }

    /// `host` as a Windows path; None when no drive reaches it (or it does not exist, under Wine).
    fn windows(&self, host: &Path) -> Option<String> {
        let Drives::Wine(letters) = self else {
            return Some(host.to_string_lossy().into_owned());
        };
        let host = host.canonicalize().ok()?;
        let (letter, base) = letters
            .iter()
            .filter(|(_, base)| host.starts_with(base))
            .max_by_key(|(_, base)| base.components().count())?;
        let rest = host.strip_prefix(base).ok()?;
        let mut out = format!("{letter}:");
        for part in rest.components() {
            out.push('\\');
            out.push_str(&part.as_os_str().to_string_lossy());
        }
        if rest.as_os_str().is_empty() {
            out.push('\\');
        }
        Some(out)
    }

    /// A Windows path on this computer, matching names without regard to case as Windows does.
    fn host(&self, windows: &str) -> Option<PathBuf> {
        let windows = windows
            .trim()
            .trim_start_matches('\u{feff}')
            .trim_end_matches(['\\', '/']);
        let Drives::Wine(letters) = self else {
            return Some(PathBuf::from(windows));
        };
        let (drive, rest) = windows.split_once(':')?;
        let mut chars = drive.chars();
        let (Some(letter), None) = (chars.next(), chars.next()) else {
            return None;
        };
        let letter = letter.to_ascii_uppercase();
        let mut path = letters.iter().find(|(l, _)| *l == letter)?.1.clone();
        for part in rest.split(['\\', '/']).filter(|p| !p.is_empty()) {
            path = find(&path, part).unwrap_or_else(|| path.join(part));
        }
        Some(path)
    }
}

/// The Windows path `host` has inside the Wine prefix `prefix`: how a Windows program started there
/// is told about a file of Demido's.
pub fn wine_path(prefix: &Path, host: &Path) -> Option<String> {
    Drives::wine(prefix).windows(host)
}

// ---------------------------------------------------------------------------------------------
// One terminal

impl Root {
    /// Every terminal64.exe in Program Files, and in the install folders the data folders name.
    fn installs(&self) -> Vec<PathBuf> {
        let mut out: Vec<PathBuf> = Vec::new();
        for dir in &self.program_files {
            let Ok(entries) = std::fs::read_dir(dir) else {
                continue;
            };
            out.extend(entries.flatten().filter_map(|e| find(&e.path(), EXE)));
        }
        for (_, origin) in self.data_dirs() {
            if let Some(exe) = self.drives.host(&origin).and_then(|dir| find(&dir, EXE))
                && !out.iter().any(|o| same_file(o, &exe))
            {
                out.push(exe);
            }
        }
        out
    }

    /// `MetaQuotes\Terminal\<id>` folders and the install folder each names (a Windows path).
    fn data_dirs(&self) -> Vec<(PathBuf, String)> {
        let mut out = Vec::new();
        for roaming in &self.roaming {
            let Some(terminals) = find_path(roaming, &["MetaQuotes", "Terminal"]) else {
                continue;
            };
            let Ok(entries) = std::fs::read_dir(terminals) else {
                continue;
            };
            for dir in entries.flatten().map(|e| e.path()) {
                if let Some(origin) = find(&dir, "origin.txt").and_then(|f| std::fs::read(f).ok()) {
                    let origin = decode(&origin);
                    out.push((dir, origin.trim().trim_start_matches('\u{feff}').to_string()));
                }
            }
        }
        out
    }

    fn terminal(&self, exe: &Path) -> Option<Terminal> {
        let install = exe.parent()?;
        let windows_path = self.drives.windows(exe)?;
        let install_windows = windows_path.rsplit_once('\\').map_or("", |(dir, _)| dir).to_string();
        let own = self
            .data_dirs()
            .into_iter()
            .find(|(_, origin)| same_windows_path(origin, &install_windows))
            .map(|(dir, _)| dir);
        let (data_dir, portable) = match own {
            Some(dir) if find(&dir, "portable.txt").is_none() => (dir, false),
            _ if find(install, "Config").is_some() || find(install, "Bases").is_some() => (install.to_path_buf(), true),
            // It never ran: where its data goes is up to it.
            _ => (install.to_path_buf(), false),
        };
        let common = find_path(&data_dir, &["Config", "common.ini"])
            .and_then(|f| std::fs::read(f).ok())
            .map(|raw| decode(&raw))
            .unwrap_or_default();
        Some(Terminal {
            path: exe.to_path_buf(),
            windows_path,
            name: install.file_name()?.to_string_lossy().into_owned(),
            prefix: self.prefix.clone(),
            accounts: accounts(&data_dir, &common),
            max_bars: ini_value(&common, "Charts", "MaxBars").and_then(|v| v.parse().ok()),
            data_dir,
            portable,
        })
    }
}

fn same_windows_path(a: &str, b: &str) -> bool {
    let norm = |p: &str| p.trim().trim_end_matches(['\\', '/']).replace('/', "\\").to_lowercase();
    norm(a) == norm(b)
}

/// `Bases\<server>\trades\<login>` for every account, plus the one `common.ini` names last, first.
fn accounts(data_dir: &Path, common: &str) -> Vec<Account> {
    let last_login = ini_value(common, "Common", "Login").and_then(|v| v.parse::<u64>().ok());
    let last_server = ini_value(common, "Common", "Server");
    let mut out: Vec<Account> = Vec::new();
    if let Some(bases) = find(data_dir, "Bases")
        && let Ok(servers) = std::fs::read_dir(bases)
    {
        for server in servers.flatten().filter(|e| e.path().is_dir()) {
            let name = server.file_name().to_string_lossy().into_owned();
            // The terminal's own folders, not servers.
            if ["Default", "Custom", "signals"]
                .iter()
                .any(|n| n.eq_ignore_ascii_case(&name))
            {
                continue;
            }
            let Some(Ok(logins)) = find(&server.path(), "trades").map(std::fs::read_dir) else {
                continue;
            };
            for login in logins.flatten().filter(|e| e.path().is_dir()) {
                if let Ok(login) = login.file_name().to_string_lossy().parse::<u64>()
                    && login > 0
                {
                    out.push(Account {
                        login,
                        server: name.clone(),
                        last: false,
                    });
                }
            }
        }
    }
    if let (Some(login), Some(server)) = (last_login.filter(|l| *l > 0), last_server.filter(|s| !s.is_empty())) {
        match out
            .iter_mut()
            .find(|a| a.login == login && a.server.eq_ignore_ascii_case(&server))
        {
            Some(account) => account.last = true,
            None => out.push(Account {
                login,
                server,
                last: true,
            }),
        }
    }
    out.sort_by(|a, b| {
        b.last
            .cmp(&a.last)
            .then_with(|| a.server.cmp(&b.server))
            .then(a.login.cmp(&b.login))
    });
    out
}

// ---------------------------------------------------------------------------------------------
// Files

/// `dir/name`, matching the name without regard to case: under Wine a file keeps whatever case it
/// was created with, and Windows programs ask for it in any.
fn find(dir: &Path, name: &str) -> Option<PathBuf> {
    let exact = dir.join(name);
    if exact.exists() {
        return Some(exact);
    }
    std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).find(|p| {
        p.file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.eq_ignore_ascii_case(name))
    })
}

fn find_path(dir: &Path, names: &[&str]) -> Option<PathBuf> {
    names.iter().try_fold(dir.to_path_buf(), |at, name| find(&at, name))
}

/// Text MetaTrader wrote: UTF-16LE with a byte order mark (its .ini files and origin.txt), or UTF-8.
fn decode(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        let units: Vec<u16> = rest.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
        return String::from_utf16_lossy(&units);
    }
    String::from_utf8_lossy(bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes)).into_owned()
}

fn ini_value(text: &str, section: &str, key: &str) -> Option<String> {
    let mut inside = false;
    for line in text.lines().map(str::trim) {
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            inside = name.eq_ignore_ascii_case(section);
        } else if inside
            && let Some((k, v)) = line.split_once('=')
            && k.trim().eq_ignore_ascii_case(key)
        {
            return Some(v.trim().to_string());
        }
    }
    None
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn utf16(text: &str) -> Vec<u8> {
        let mut out = vec![0xFF, 0xFE];
        out.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
        out
    }

    fn write(path: &Path, bytes: &[u8]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    /// A prefix like the one MetaQuotes' script makes: C: and Z: in `dosdevices`.
    fn prefix(dir: &Path) -> PathBuf {
        let prefix = dir.join("mt5 prefix");
        std::fs::create_dir_all(prefix.join("dosdevices")).unwrap();
        std::fs::create_dir_all(prefix.join("drive_c/users/me/AppData/Roaming")).unwrap();
        std::os::unix::fs::symlink("../drive_c", prefix.join("dosdevices/c:")).unwrap();
        std::os::unix::fs::symlink("/", prefix.join("dosdevices/z:")).unwrap();
        prefix
    }

    #[test]
    fn a_portable_terminal_under_wine_with_its_accounts() {
        let dir = tempfile::tempdir().unwrap();
        let prefix = prefix(dir.path());
        let install = prefix.join("drive_c/Program Files/MetaTrader 5");
        write(&install.join("terminal64.exe"), b"MZ");
        write(
            &install.join("Config/common.ini"),
            &utf16("[Common]\r\nLogin=113998409\r\nServer=MetaQuotes-Demo\r\n[Charts]\r\nMaxBars=500000\r\n"),
        );
        for account in [
            "MetaQuotes-Demo/trades/113998409",
            "MetaQuotes-Demo/trades/5000",
            "ICMarketsSC-Demo/trades/77",
        ] {
            std::fs::create_dir_all(install.join("Bases").join(account)).unwrap();
        }
        for skipped in [
            "Default/trades/0",
            "Custom/history",
            "signals",
            "MetaQuotes-Demo/trades/0",
        ] {
            std::fs::create_dir_all(install.join("Bases").join(skipped)).unwrap();
        }
        // The terminal's own folder, marked portable: its data is in the install folder.
        let own = prefix.join("drive_c/users/me/AppData/Roaming/MetaQuotes/Terminal/D0E8209F77C8CF37AD8BF550E51FF075");
        write(&own.join("origin.txt"), &utf16("C:\\Program Files\\MetaTrader 5"));
        write(&own.join("portable.txt"), b"");

        let root = wine_root(&prefix);
        let exes = root.installs();
        assert_eq!(exes, vec![install.join("terminal64.exe")], "found once, by both ways");
        let t = root.terminal(&exes[0]).unwrap();
        assert_eq!(t.windows_path, "C:\\Program Files\\MetaTrader 5\\terminal64.exe");
        assert_eq!(t.name, "MetaTrader 5");
        assert_eq!(t.prefix.as_deref(), Some(prefix.as_path()));
        assert_eq!(t.data_dir, install);
        assert!(t.portable);
        assert_eq!(t.max_bars, Some(500_000));
        let accounts: Vec<(u64, &str, bool)> = t
            .accounts
            .iter()
            .map(|a| (a.login, a.server.as_str(), a.last))
            .collect();
        assert_eq!(
            accounts,
            vec![
                (113998409, "MetaQuotes-Demo", true),
                (77, "ICMarketsSC-Demo", false),
                (5000, "MetaQuotes-Demo", false),
            ]
        );
        assert_eq!(terminal_at(&install.join("terminal64.exe")), Some(t));
    }

    #[test]
    fn a_terminal_elsewhere_with_its_data_in_appdata() {
        let dir = tempfile::tempdir().unwrap();
        let prefix = prefix(dir.path());
        // Installed outside Program Files, under another case than its data folder writes.
        let install = prefix.join("drive_c/Brokers/FTMO MetaTrader 5");
        write(&install.join("Terminal64.exe"), b"MZ");
        let own = prefix.join("drive_c/users/me/AppData/Roaming/MetaQuotes/Terminal/ABC");
        write(&own.join("origin.txt"), &utf16("C:\\brokers\\FTMO MetaTrader 5\r\n"));
        write(
            &own.join("config/common.ini"),
            &utf16("[Common]\r\nLogin=42\r\nServer=FTMO-Demo\r\n"),
        );
        // Logged into before, but not the last one.
        std::fs::create_dir_all(own.join("bases/FTMO-Server/trades/7")).unwrap();

        let root = wine_root(&prefix);
        let exes = root.installs();
        assert_eq!(exes, vec![install.join("Terminal64.exe")]);
        let t = root.terminal(&exes[0]).unwrap();
        assert_eq!(t.windows_path, "C:\\Brokers\\FTMO MetaTrader 5\\Terminal64.exe");
        assert_eq!(t.data_dir, own);
        assert!(!t.portable);
        assert_eq!(t.max_bars, None);
        assert_eq!(
            t.accounts,
            vec![
                Account {
                    login: 42,
                    server: "FTMO-Demo".into(),
                    last: true
                },
                Account {
                    login: 7,
                    server: "FTMO-Server".into(),
                    last: false
                },
            ]
        );
    }

    #[test]
    fn a_terminal_that_never_ran_has_no_accounts_and_nothing_else_is_one() {
        let dir = tempfile::tempdir().unwrap();
        let prefix = prefix(dir.path());
        let install = prefix.join("drive_c/Program Files/MetaTrader 5");
        write(&install.join("terminal64.exe"), b"MZ");
        write(&prefix.join("drive_c/Program Files/Other/other.exe"), b"MZ");
        let t = wine_root(&prefix).terminal(&install.join("terminal64.exe")).unwrap();
        assert!(t.accounts.is_empty());
        assert!(!t.portable);
        assert_eq!(wine_root(&prefix).installs().len(), 1);

        assert_eq!(terminal_at(&prefix.join("drive_c/Program Files/Other/other.exe")), None);
        assert_eq!(terminal_at(&install.join("missing/terminal64.exe")), None);
    }

    #[test]
    fn prefixes_come_from_the_usual_places_and_wine_menu_entries() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let data = home.join(".local/share");
        for p in [
            ".mt5",
            ".wine",
            ".local/share/wineprefixes/trading",
            "Games/broker",
            "Other place",
        ] {
            std::fs::create_dir_all(home.join(p).join("drive_c")).unwrap();
        }
        // Not prefixes: no drive_c.
        std::fs::create_dir_all(data.join("wineprefixes/empty")).unwrap();
        let entry = |exec: &str| format!("[Desktop Entry]\nName=MetaTrader 5\nExec={exec}\nType=Application\n");
        write(
            &data.join("applications/wine/Programs/MetaTrader 5/MetaTrader 5.desktop"),
            entry(&format!(
                "env \"WINEPREFIX={}\" wine \"C:\\\\\\\\users\\\\\\\\me\\\\\\\\MetaTrader 5.lnk\"",
                home.join("Games/broker").display()
            ))
            .as_bytes(),
        );
        write(
            &data.join("applications/wine-extension-ini.desktop"),
            entry(&format!(
                "env WINEPREFIX=\"{}\" wine start /ProgIDOpen inifile %f",
                home.join("Other place").display()
            ))
            .as_bytes(),
        );
        write(
            &data.join("applications/firefox.desktop"),
            entry("firefox %u").as_bytes(),
        );

        let found = wine_prefixes(&home, &data, Some(home.join(".mt5")));
        let names: Vec<String> = found
            .iter()
            .map(|p| {
                p.strip_prefix(home.canonicalize().unwrap())
                    .unwrap()
                    .display()
                    .to_string()
            })
            .collect();
        assert_eq!(
            names,
            vec![
                ".mt5",
                ".wine",
                ".local/share/wineprefixes/trading",
                "Games/broker",
                "Other place"
            ]
        );
    }

    #[test]
    fn desktop_entries_name_their_prefix_either_way() {
        assert_eq!(
            desktop_prefixes(
                "Exec=env \"WINEPREFIX=/home/a/.mt5\" wine \"C:\\\\x.lnk\"\nPath=/home/a/.mt5/drive_c/Program Files/MetaTrader 5\n"
            ),
            vec![PathBuf::from("/home/a/.mt5"), PathBuf::from("/home/a/.mt5")]
        );
        assert_eq!(
            desktop_prefixes("Exec=env WINEPREFIX=\"/home/a/my prefix\" wine x"),
            vec![PathBuf::from("/home/a/my prefix")]
        );
        assert_eq!(
            desktop_prefixes("Exec=env WINEPREFIX=/home/a/p wine x"),
            vec![PathBuf::from("/home/a/p")]
        );
        assert!(desktop_prefixes("Exec=wine x\nPath=/home/a\n").is_empty());
    }

    #[test]
    fn paths_cross_between_the_prefix_and_this_computer() {
        let dir = tempfile::tempdir().unwrap();
        let prefix = prefix(dir.path());
        let tools = dir.path().join("Demido data/tools");
        write(&tools.join("bridge.py"), b"");
        let drives = Drives::wine(&prefix);
        let real = tools.canonicalize().unwrap();
        assert_eq!(
            wine_path(&prefix, &tools.join("bridge.py")).unwrap(),
            format!("Z:{}\\bridge.py", real.display().to_string().replace('/', "\\"))
        );
        assert_eq!(drives.windows(&prefix.join("drive_c")).unwrap(), "C:\\");
        assert_eq!(wine_path(&prefix, &tools.join("missing.py")), None);

        std::fs::create_dir_all(prefix.join("drive_c/Program Files/MetaTrader 5")).unwrap();
        assert_eq!(
            drives.host("c:\\program files\\MetaTrader 5\\").unwrap(),
            prefix
                .join("drive_c/Program Files/MetaTrader 5")
                .canonicalize()
                .unwrap()
        );
        assert_eq!(drives.host("Q:\\nowhere"), None);
        assert_eq!(drives.host("not a path"), None);

        // A prefix without dosdevices still has C: and Z:.
        let bare = dir.path().join("bare");
        std::fs::create_dir_all(bare.join("drive_c")).unwrap();
        assert_eq!(Drives::wine(&bare).windows(&bare.join("drive_c")).unwrap(), "C:\\");
    }

    #[test]
    fn metatrader_text_is_utf16_or_utf8() {
        assert_eq!(decode(&utf16("[Common]\r\nLogin=1\r\n")), "[Common]\r\nLogin=1\r\n");
        assert_eq!(decode(b"\xEF\xBB\xBFplain"), "plain");
        let ini = "[Common]\nLogin=1\n[Charts]\nmaxbars = 100\n";
        assert_eq!(ini_value(ini, "charts", "MaxBars").as_deref(), Some("100"));
        assert_eq!(ini_value(ini, "Common", "MaxBars"), None);
    }
}
