//! The sandbox on Windows (DECISIONS T26, T27; research note 29).
//!
//! A program a model runs is started as a user of Demido's own, `DemidoSandbox`, made once at setup with an
//! administrator's rights, with firewall rules that shut its network. Windows keeps the person's profile
//! (their documents, keys, browser, mail) from it. That alone does not keep it from changing things elsewhere
//! (Windows lets every user change a folder made at `C:\`), so the command is started with a
//! *write-restricted* token, which carries a SID made up for the folder: whenever something is to be
//! changed, Windows asks for that SID too, and only the session's folder and the command's own temporary
//! folder grant it.
//!
//! Three hidden commands of `demido` do it, each as the one who should:
//!
//! - `demido sandbox-setup`, once, as an administrator (Windows asks): the user, its firewall rules, and a
//!   folder only the administrators and the person may change, `%ProgramData%\Demido\sandbox`, which holds
//!   a copy of `demido` the user can run and the commands' temporary folders.
//! - `demido sandbox-launch`, as the person, started by the core the way any command is (in its job, its
//!   output on pipes): shares the folder, starts the runner as the user with its password, which only the
//!   person can read back (DPAPI), and holds it in a job that ends with it.
//! - `demido sandbox-runner`, as the user: makes the write-restricted token from its own and starts
//!   `cmd /D /S /C` with it.
//!
//! What it does not keep a program from: reading what Windows lets every user read (folders at `C:\` and
//! on other disks; not the person's profile), another session's folder in use at the same moment (to read
//! only), servers on this computer's loopback, and names looked up through Windows's DNS service.

use std::ffi::{OsStr, c_void};
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::Digest;
use windows_sys::Win32::Foundation::{CloseHandle, ERROR_SUCCESS, GetLastError, HANDLE, HANDLE_FLAG_INHERIT, HLOCAL, LocalFree, SetHandleInformation};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSidToSidW, DENY_ACCESS, EXPLICIT_ACCESS_W, GRANT_ACCESS, GetNamedSecurityInfoW, NO_MULTIPLE_TRUSTEE, REVOKE_ACCESS, SE_FILE_OBJECT, SET_ACCESS,
    SetEntriesInAclW, SetNamedSecurityInfoW, TRUSTEE_IS_SID, TRUSTEE_IS_UNKNOWN, TRUSTEE_W,
};
use windows_sys::Win32::Security::Cryptography::{CRYPT_INTEGER_BLOB, CRYPTPROTECT_LOCAL_MACHINE, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData};
use windows_sys::Win32::Security::{
    ACL, CONTAINER_INHERIT_ACE, CreateRestrictedToken, DACL_SECURITY_INFORMATION, DISABLE_MAX_PRIVILEGE, EqualSid, GetTokenInformation, LUA_TOKEN, NO_INHERITANCE, OBJECT_INHERIT_ACE,
    OWNER_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, SID_AND_ATTRIBUTES, SetTokenInformation, TOKEN_ADJUST_DEFAULT, TOKEN_ASSIGN_PRIMARY,
    TOKEN_DEFAULT_DACL, TOKEN_DUPLICATE, TOKEN_GROUPS, TOKEN_QUERY, TOKEN_USER, TokenDefaultDacl, TokenGroups, TokenUser, WRITE_RESTRICTED,
};
use windows_sys::Win32::System::Console::{GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation, SetInformationJobObject,
};
use windows_sys::Win32::System::Threading::{
    CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, CreateProcessAsUserW, CreateProcessWithLogonW, GetCurrentProcess, GetExitCodeProcess, INFINITE, LOGON_WITH_PROFILE, OpenProcessToken,
    PROCESS_INFORMATION, ResumeThread, STARTF_USESHOWWINDOW, STARTF_USESTDHANDLES, STARTUPINFOW, TerminateProcess, WaitForSingleObject,
};

/// The user of Demido's own that programs run as.
pub const USER: &str = "DemidoSandbox";
/// What may be done with a file or folder shared with the sandbox: read, change, make, delete; not change
/// who may use it (the "Modify" of Windows's own dialog).
const MODIFY: u32 = 0x0013_01BF;
/// Read and run, and nothing more.
const READ_AND_RUN: u32 = 0x0012_00A9;
const FULL: u32 = 0x001F_01FF;
const GENERIC_ALL: u32 = 0x1000_0000;
const EVERYONE: &str = "S-1-1-0";
const ADMINISTRATORS: &str = "S-1-5-32-544";
const SYSTEM: &str = "S-1-5-18";
/// The mark Windows puts on a token's logon SID among its groups.
const SE_GROUP_LOGON_ID: u32 = 0xC000_0000;

fn wide(s: impl AsRef<OsStr>) -> Vec<u16> {
    s.as_ref().encode_wide().chain(Some(0)).collect()
}

fn last_error(what: &str) -> String {
    // SAFETY: reads this thread's last error.
    format!("{what} failed (Windows error {})", unsafe { GetLastError() })
}

// ---------- what setup left ----------

/// What setup keeps, in the person's Demido folder: who the user is, and its password, which only the person
/// can read back.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Kept {
    pub user: String,
    pub sid: String,
    /// Made up at setup: what the folders' SIDs are made from, with each folder's path.
    pub salt: String,
    /// The password, protected with DPAPI, in base64.
    key: String,
    /// Whose DPAPI protects it: `machine` just after setup (an administrator wrote it), then `person`.
    scope: String,
}

fn kept_path(home: &Path) -> PathBuf {
    home.join("sandbox").join("user.json")
}

/// What setup left, with the password taken into the person's own scope if setup could not do that itself.
pub fn kept(home: &Path) -> Result<Kept, String> {
    let path = kept_path(home);
    let text = std::fs::read_to_string(&path).map_err(|_| "The sandbox has not been set up on this computer yet.".to_string())?;
    let mut kept: Kept = serde_json::from_str(&text).map_err(|e| format!("what the sandbox's setup left cannot be read: {e}"))?;
    if kept.scope != "person" {
        let password = unprotect(&decode(&kept.key)?, true)?;
        kept.key = base64::engine::general_purpose::STANDARD.encode(protect(&password, false)?);
        kept.scope = "person".into();
        let text = serde_json::to_string_pretty(&kept).map_err(|e| e.to_string())?;
        std::fs::write(&path, text).map_err(|e| format!("the sandbox's password could not be kept: {e}"))?;
    }
    Ok(kept)
}

fn decode(key: &str) -> Result<Vec<u8>, String> {
    base64::engine::general_purpose::STANDARD.decode(key).map_err(|e| format!("the sandbox's password is damaged: {e}"))
}

impl Kept {
    fn password(&self) -> Result<String, String> {
        let bytes = unprotect(&decode(&self.key)?, self.scope != "person")?;
        String::from_utf8(bytes).map_err(|_| "the sandbox's password is damaged".to_string())
    }
}

// ---------- DPAPI ----------

fn protect(data: &[u8], machine: bool) -> Result<Vec<u8>, String> {
    crypt(data, machine, true)
}

fn unprotect(data: &[u8], machine: bool) -> Result<Vec<u8>, String> {
    crypt(data, machine, false)
}

fn crypt(data: &[u8], machine: bool, protecting: bool) -> Result<Vec<u8>, String> {
    let input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
    let mut output = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
    let flags = CRYPTPROTECT_UI_FORBIDDEN | if machine { CRYPTPROTECT_LOCAL_MACHINE } else { 0 };
    // SAFETY: the input outlives the call; the output is Windows's, copied, then freed with LocalFree.
    unsafe {
        let ok = if protecting {
            CryptProtectData(&input, std::ptr::null(), std::ptr::null(), std::ptr::null(), std::ptr::null(), flags, &mut output)
        } else {
            CryptUnprotectData(&input, std::ptr::null_mut(), std::ptr::null(), std::ptr::null(), std::ptr::null(), flags, &mut output)
        };
        if ok == 0 {
            return Err(last_error(if protecting { "Keeping the sandbox's password" } else { "Reading the sandbox's password back" }));
        }
        let bytes = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        LocalFree(output.pbData as HLOCAL);
        Ok(bytes)
    }
}

// ---------- SIDs ----------

/// A SID, as Windows holds it, freed when dropped.
struct Sid(PSID);

impl Sid {
    fn parse(text: &str) -> Result<Sid, String> {
        let mut sid: PSID = std::ptr::null_mut();
        // SAFETY: a valid wide string in; the SID out is freed by Drop.
        if unsafe { ConvertStringSidToSidW(wide(text).as_ptr(), &mut sid) } == 0 {
            return Err(format!("{text} is not a SID"));
        }
        Ok(Sid(sid))
    }
}

impl Drop for Sid {
    fn drop(&mut self) {
        // SAFETY: allocated by ConvertStringSidToSidW, freed once.
        unsafe {
            LocalFree(self.0 as HLOCAL);
        }
    }
}

/// A SID Windows gave, as text.
fn sid_text(sid: PSID) -> Result<String, String> {
    let mut text: *mut u16 = std::ptr::null_mut();
    // SAFETY: a valid SID in; the text out is Windows's, copied, then freed.
    unsafe {
        if ConvertSidToStringSidW(sid, &mut text) == 0 {
            return Err(last_error("Reading a SID"));
        }
        let len = (0..).take_while(|&i| *text.add(i) != 0).count();
        let out = String::from_utf16_lossy(std::slice::from_raw_parts(text, len));
        LocalFree(text as HLOCAL);
        Ok(out)
    }
}

/// The SID of whoever runs this program: the person, in Demido's own processes.
pub fn own_sid() -> Result<String, String> {
    let token = own_token(TOKEN_QUERY)?;
    let info = token_info(token.0, TokenUser)?;
    // SAFETY: the buffer holds a TOKEN_USER, as asked for.
    let user = unsafe { &*(info.as_ptr() as *const TOKEN_USER) };
    sid_text(user.User.Sid)
}

/// A path as people write it: without the `\\?\` that Windows's own full paths start with.
pub fn plain(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    match text.strip_prefix(r"\\?\") {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => PathBuf::from(rest),
        _ => path.to_path_buf(),
    }
}

/// The SID made up for a folder: from the salt setup made and the folder's path, so that each folder has
/// its own and keeps it. Not any account's: no account is ever given it, only folders.
pub fn folder_sid(salt: &str, folder: &Path) -> String {
    let mut hash = sha2::Sha256::new();
    hash.update(salt.as_bytes());
    let path = plain(&folder.canonicalize().unwrap_or_else(|_| folder.to_path_buf())).to_string_lossy().to_lowercase();
    hash.update(path.trim_end_matches('\\').as_bytes());
    let digest = hash.finalize();
    let part = |i: usize| u32::from_le_bytes([digest[i], digest[i + 1], digest[i + 2], digest[i + 3]]) & 0x7FFF_FFFF;
    format!("S-1-5-21-{}-{}-{}-{}", part(0).max(1), part(4).max(1), part(8).max(1), part(12).max(1000))
}

struct Handle(HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: a handle this process opened, closed once.
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}

fn own_token(access: u32) -> Result<Handle, String> {
    let mut token: HANDLE = std::ptr::null_mut();
    // SAFETY: the current process's pseudo-handle; the token out is closed by Handle.
    if unsafe { OpenProcessToken(GetCurrentProcess(), access, &mut token) } == 0 {
        return Err(last_error("Opening this program's token"));
    }
    Ok(Handle(token))
}

/// A part of a token, as Windows lays it out, in a buffer aligned for it.
fn token_info(token: HANDLE, class: i32) -> Result<Vec<u64>, String> {
    let mut need = 0u32;
    // SAFETY: the first call only asks how much room is needed.
    unsafe { GetTokenInformation(token, class, std::ptr::null_mut(), 0, &mut need) };
    let mut buffer = vec![0u64; (need as usize).div_ceil(8)];
    // SAFETY: the buffer is as large as Windows said.
    if unsafe { GetTokenInformation(token, class, buffer.as_mut_ptr() as *mut c_void, need, &mut need) } == 0 {
        return Err(last_error("Reading this program's token"));
    }
    Ok(buffer)
}

// ---------- who may use a file ----------

fn access(sid: &Sid, permissions: u32, mode: i32, inherit: bool) -> EXPLICIT_ACCESS_W {
    EXPLICIT_ACCESS_W {
        grfAccessPermissions: permissions,
        grfAccessMode: mode,
        grfInheritance: if inherit { OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE } else { NO_INHERITANCE },
        Trustee: TRUSTEE_W {
            pMultipleTrustee: std::ptr::null_mut(),
            MultipleTrusteeOperation: NO_MULTIPLE_TRUSTEE,
            TrusteeForm: TRUSTEE_IS_SID,
            TrusteeType: TRUSTEE_IS_UNKNOWN,
            ptstrName: sid.0 as *mut u16,
        },
    }
}

/// Changes who may use `path`: `entries` added to (or taken out of) what it has, or, with `protected`, in
/// place of it. Windows carries the change to everything inside a folder.
fn set_entries(path: &Path, entries: &[EXPLICIT_ACCESS_W], protected: bool) -> Result<(), String> {
    let name = wide(path);
    // SAFETY: Windows's own descriptor and lists, each freed with LocalFree; the entries outlive the calls.
    unsafe {
        let mut old: *mut ACL = std::ptr::null_mut();
        let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
        if !protected {
            let r = GetNamedSecurityInfoW(name.as_ptr(), SE_FILE_OBJECT, DACL_SECURITY_INFORMATION, std::ptr::null_mut(), std::ptr::null_mut(), &mut old, std::ptr::null_mut(), &mut descriptor);
            if r != ERROR_SUCCESS {
                return Err(format!("who may use {} could not be read (Windows error {r})", path.display()));
            }
        }
        let mut new: *mut ACL = std::ptr::null_mut();
        let r = SetEntriesInAclW(entries.len() as u32, entries.as_ptr(), old, &mut new);
        if !descriptor.is_null() {
            LocalFree(descriptor as HLOCAL);
        }
        if r != ERROR_SUCCESS {
            return Err(format!("who may use {} could not be changed (Windows error {r})", path.display()));
        }
        let info = DACL_SECURITY_INFORMATION | if protected { PROTECTED_DACL_SECURITY_INFORMATION } else { 0 };
        let r = SetNamedSecurityInfoW(name.as_ptr() as *mut u16, SE_FILE_OBJECT, info, std::ptr::null_mut(), std::ptr::null_mut(), new, std::ptr::null());
        LocalFree(new as HLOCAL);
        if r != ERROR_SUCCESS {
            return Err(format!("who may use {} could not be changed (Windows error {r})", path.display()));
        }
    }
    Ok(())
}

/// Whether `path` itself grants `sid` something (an entry of its own, not one it inherits).
fn grants(path: &Path, sid: &Sid) -> bool {
    has_entry(path, sid, 0)
}

/// Whether `path` itself refuses `sid` (an entry of its own).
fn refuses(path: &Path, sid: &Sid) -> bool {
    has_entry(path, sid, 1)
}

/// Whether `path` has an entry of its own of `kind` (0 allows, 1 refuses) for `sid`.
fn has_entry(path: &Path, sid: &Sid, wanted: u8) -> bool {
    let name = wide(path);
    // SAFETY: Windows's own descriptor, read and freed here; every ACE is read within the ACL's size.
    unsafe {
        let mut acl: *mut ACL = std::ptr::null_mut();
        let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
        if GetNamedSecurityInfoW(name.as_ptr(), SE_FILE_OBJECT, DACL_SECURITY_INFORMATION, std::ptr::null_mut(), std::ptr::null_mut(), &mut acl, std::ptr::null_mut(), &mut descriptor) != ERROR_SUCCESS
            || acl.is_null()
        {
            return false;
        }
        let mut found = false;
        let count = (*acl).AceCount as usize;
        let mut at = (acl as *const u8).add(std::mem::size_of::<ACL>());
        for _ in 0..count {
            // ACE_HEADER: type (u8), flags (u8), size (u16); an ACCESS_ALLOWED_ACE then has its mask (u32) and the SID.
            let (kind, flags, size) = (*at, *at.add(1), u16::from_le_bytes([*at.add(2), *at.add(3)]) as usize);
            const INHERITED_ACE: u8 = 0x10;
            if kind == wanted && flags & INHERITED_ACE == 0 && EqualSid(at.add(8) as PSID, sid.0) != 0 {
                found = true;
                break;
            }
            at = at.add(size);
        }
        LocalFree(descriptor as HLOCAL);
        found
    }
}

/// The owner of `path`, as text.
pub fn owner_of(path: &Path) -> Option<String> {
    let name = wide(path);
    // SAFETY: Windows's own descriptor, read and freed here.
    unsafe {
        let mut owner: PSID = std::ptr::null_mut();
        let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
        if GetNamedSecurityInfoW(name.as_ptr(), SE_FILE_OBJECT, OWNER_SECURITY_INFORMATION, &mut owner, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), &mut descriptor)
            != ERROR_SUCCESS
        {
            return None;
        }
        let text = sid_text(owner).ok();
        LocalFree(descriptor as HLOCAL);
        text
    }
}

fn set_owner(path: &Path, sid: &Sid) -> Result<(), String> {
    let name = wide(path);
    // SAFETY: a valid SID; nothing allocated.
    let r = unsafe { SetNamedSecurityInfoW(name.as_ptr() as *mut u16, SE_FILE_OBJECT, OWNER_SECURITY_INFORMATION, sid.0, std::ptr::null_mut(), std::ptr::null(), std::ptr::null()) };
    if r != ERROR_SUCCESS {
        return Err(format!("{} could not be given back to you (Windows error {r})", path.display()));
    }
    Ok(())
}

/// Shares a session's folder with the sandbox: its user may read and change what is in it, and its own SID
/// is granted too, which the write-restricted token asks for. Done once: a folder already shared is left.
pub fn share(kept: &Kept, folder: &Path) -> Result<String, String> {
    let made_up = folder_sid(&kept.salt, folder);
    let (user, sid) = (Sid::parse(&kept.sid)?, Sid::parse(&made_up)?);
    // A folder kept from the sandbox (IDEAS 147) is let in while a session has it: an entry that refuses
    // would weigh more than the one that allows.
    if refuses(folder, &user) {
        drop_refusal(folder, &user)?;
    }
    if !(grants(folder, &user) && grants(folder, &sid)) {
        set_entries(folder, &[access(&user, MODIFY, GRANT_ACCESS, true), access(&sid, MODIFY, GRANT_ACCESS, true)], false)?;
    }
    Ok(made_up)
}

/// Takes a folder back from the sandbox: both entries come off, and what the sandbox's user made there is
/// given to the person, because an owner can always give itself access again.
pub fn give_back(home: &Path, folder: &Path) -> Result<(), String> {
    let kept = kept(home)?;
    let folder = &plain(&folder.canonicalize().map_err(|e| format!("{} cannot be opened: {e}", folder.display()))?);
    let (user, sid) = (Sid::parse(&kept.sid)?, Sid::parse(&folder_sid(&kept.salt, folder))?);
    if grants(folder, &user) || grants(folder, &sid) {
        set_entries(folder, &[access(&user, 0, REVOKE_ACCESS, false), access(&sid, 0, REVOKE_ACCESS, false)], false)?;
    }
    // Kept from the sandbox again, if the person asked for it (IDEAS 147).
    if kept_out(home).iter().any(|k| k == folder) {
        set_entries(folder, &[access(&user, FULL, DENY_ACCESS, true)], false)?;
    }
    let me = Sid::parse(&own_sid()?)?;
    let mut problems = Vec::new();
    reown(folder, &kept.sid, &me, &mut problems);
    match problems.first() {
        None => Ok(()),
        Some(first) => Err(format!("{} of the files the sandbox made could not be given back to you; the first: {first}", problems.len())),
    }
}

// ---------- folders kept from the sandbox (IDEAS 147) ----------

/// Takes an entry refusing `sid` off `path`. Windows's REVOKE_ACCESS takes off only the entries that allow (found
/// on the checks' Windows machine: the refusal stayed); GRANT_ACCESS cancels the rights an entry refuses. So the
/// rights are granted, which cancels the refusal, and then revoked, which leaves neither.
fn drop_refusal(path: &Path, sid: &Sid) -> Result<(), String> {
    set_entries(path, &[access(sid, FULL, GRANT_ACCESS, true)], false)?;
    set_entries(path, &[access(sid, 0, REVOKE_ACCESS, false)], false)
}

fn kept_out_path(home: &Path) -> PathBuf {
    home.join("sandbox").join("kept-out.json")
}

/// The folders the person keeps the sandbox out of.
pub fn kept_out(home: &Path) -> Vec<PathBuf> {
    std::fs::read(kept_out_path(home)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn keep_list(home: &Path, list: &[PathBuf]) -> Result<(), String> {
    std::fs::write(kept_out_path(home), serde_json::to_vec_pretty(list).map_err(|e| e.to_string())?).map_err(|e| format!("the list of folders kept from the sandbox could not be written: {e}"))
}

/// Keeps the sandbox's user out of one of the person's folders: an entry that refuses it everything, which
/// Windows weighs before any entry that allows, so a program inside cannot read it even where Windows lets
/// every user read (a folder at `C:\` or on another disk). Changes the person's own folder, so only when they
/// ask (`demido sandbox keep-out`). A session that uses the folder lets the sandbox in while it has it.
pub fn keep_out(home: &Path, folder: &Path) -> Result<PathBuf, String> {
    let kept = kept(home)?;
    let folder = plain(&folder.canonicalize().map_err(|e| format!("{} cannot be opened: {e}", folder.display()))?);
    let user = Sid::parse(&kept.sid)?;
    if !refuses(&folder, &user) && !grants(&folder, &user) {
        set_entries(&folder, &[access(&user, FULL, DENY_ACCESS, true)], false)?;
    }
    let mut list = kept_out(home);
    if !list.contains(&folder) {
        list.push(folder.clone());
        keep_list(home, &list)?;
    }
    Ok(folder)
}

/// Undoes `keep_out`: the refusing entry comes off, and the folder is as Windows had it.
pub fn let_back_in(home: &Path, folder: &Path) -> Result<PathBuf, String> {
    let kept = kept(home)?;
    let folder = plain(&folder.canonicalize().map_err(|e| format!("{} cannot be opened: {e}", folder.display()))?);
    let user = Sid::parse(&kept.sid)?;
    if refuses(&folder, &user) && !grants(&folder, &user) {
        drop_refusal(&folder, &user)?;
    }
    let list: Vec<PathBuf> = kept_out(home).into_iter().filter(|k| k != &folder).collect();
    keep_list(home, &list)?;
    Ok(folder)
}

fn reown(path: &Path, from: &str, to: &Sid, problems: &mut Vec<String>) {
    if owner_of(path).is_some_and(|o| o.eq_ignore_ascii_case(from))
        && let Err(e) = set_owner(path, to)
    {
        problems.push(e);
    }
    // Into folders, never through a link: what a link leads to is not the folder's.
    if let Ok(meta) = std::fs::symlink_metadata(path)
        && meta.is_dir()
        && !meta.file_type().is_symlink()
        && let Ok(entries) = std::fs::read_dir(path)
    {
        for entry in entries.flatten() {
            reown(&entry.path(), from, to, problems);
        }
    }
}

/// A folder no one but `entries` may use: nothing inherited from around it.
fn private_folder(path: &Path, entries: &[(&Sid, u32)]) -> Result<(), String> {
    std::fs::create_dir_all(path).map_err(|e| format!("{} could not be made: {e}", path.display()))?;
    let list: Vec<EXPLICIT_ACCESS_W> = entries.iter().map(|(sid, permissions)| access(sid, *permissions, SET_ACCESS, true)).collect();
    set_entries(path, &list, true)
}

// ---------- the sandbox's own place ----------

/// `%ProgramData%\Demido\sandbox`: what setup made, which only the administrators and the person may change.
pub fn place() -> PathBuf {
    std::env::var_os("ProgramData").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\ProgramData")).join("Demido").join("sandbox")
}

/// A copy of this program the sandbox's user can run, made again whenever this program changed.
fn runner() -> Result<PathBuf, String> {
    let me = std::env::current_exe().map_err(|e| format!("this program cannot be found: {e}"))?;
    let meta = std::fs::metadata(&me).map_err(|e| format!("this program cannot be read: {e}"))?;
    let mark = format!("{} {:?}", meta.len(), meta.modified().ok());
    let (copy, noted) = (place().join("demido-runner.exe"), place().join("runner.txt"));
    if copy.is_file() && std::fs::read_to_string(&noted).is_ok_and(|n| n == mark) {
        return Ok(copy);
    }
    let fresh = place().join(format!("demido-runner-{}.exe", uuid::Uuid::new_v4().simple()));
    std::fs::copy(&me, &fresh).map_err(|e| format!("the sandbox's copy of Demido could not be made in {}: {e}", place().display()))?;
    let _ = std::fs::remove_file(&copy);
    std::fs::rename(&fresh, &copy).map_err(|e| format!("the sandbox's copy of Demido could not be put in place: {e}"))?;
    std::fs::write(&noted, mark).map_err(|e| format!("the sandbox's copy of Demido could not be noted: {e}"))?;
    Ok(copy)
}

/// Whether this computer's sandbox is set up and can be used: what setup left, the password, the place.
pub fn ready(home: &Path) -> Result<Kept, String> {
    let kept = kept(home)?;
    kept.password()?;
    if !place().is_dir() {
        return Err(format!("{} is missing: set the sandbox up again.", place().display()));
    }
    Ok(kept)
}

// ---------- demido sandbox-launch, as the person ----------

/// Runs `script` in `folder` as the sandbox's user, with what it prints on this program's own output, and
/// gives back its exit code. Everything it starts ends when this program ends.
pub fn launch(home: &Path, folder: &Path, script: &str) -> Result<u32, String> {
    let kept = ready(home)?;
    let password = kept.password()?;
    let made_up = share(&kept, folder)?;
    let runner = runner()?;
    // Its own temporary folder and home, for it alone and for the person: gone when it ends.
    let (user, sid, me) = (Sid::parse(&kept.sid)?, Sid::parse(&made_up)?, Sid::parse(&own_sid()?)?);
    let (system, admins) = (Sid::parse(SYSTEM)?, Sid::parse(ADMINISTRATORS)?);
    let scratch = place().join("scratch").join(uuid::Uuid::new_v4().simple().to_string());
    private_folder(&scratch, &[(&me, FULL), (&system, FULL), (&admins, FULL), (&user, MODIFY), (&sid, MODIFY)])?;
    let _gone = Gone(scratch.clone());
    for inside in ["temp", r"home\AppData\Roaming", r"home\AppData\Local"] {
        std::fs::create_dir_all(scratch.join(inside)).map_err(|e| format!("the command's temporary folder could not be made: {e}"))?;
    }
    let environment = environment(&scratch);
    let line = format!("\"{}\" sandbox-runner --sid {made_up} --script {}", runner.display(), hex(script.as_bytes()));
    let mut line: Vec<u16> = wide(&line);
    let (user_w, domain, password_w, runner_w, folder_w) = (wide(&kept.user), wide("."), wide(&password), wide(&runner), wide(folder));
    // SAFETY: plain Win32 calls; every buffer outlives the call it is given to; handles are closed by Handle.
    unsafe {
        let mut info: STARTUPINFOW = std::mem::zeroed();
        info.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
        info.dwFlags = STARTF_USESTDHANDLES | STARTF_USESHOWWINDOW;
        info.wShowWindow = 0; // SW_HIDE: a console of its own, never seen
        info.hStdInput = inheritable(GetStdHandle(STD_INPUT_HANDLE));
        info.hStdOutput = inheritable(GetStdHandle(STD_OUTPUT_HANDLE));
        info.hStdError = inheritable(GetStdHandle(STD_ERROR_HANDLE));
        let mut started: PROCESS_INFORMATION = std::mem::zeroed();
        let ok = CreateProcessWithLogonW(
            user_w.as_ptr(),
            domain.as_ptr(),
            password_w.as_ptr(),
            LOGON_WITH_PROFILE,
            runner_w.as_ptr(),
            line.as_mut_ptr(),
            CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT,
            environment.as_ptr() as *const c_void,
            folder_w.as_ptr(),
            &info,
            &mut started,
        );
        if ok == 0 {
            return Err(last_error("Starting the command as the sandbox's user"));
        }
        let (process, thread) = (Handle(started.hProcess), Handle(started.hThread));
        // A job of its own, ended when this program ends however it ends, so nothing it starts is left.
        let job = Handle(CreateJobObjectW(std::ptr::null(), std::ptr::null()));
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let held = !job.0.is_null()
            && SetInformationJobObject(job.0, JobObjectExtendedLimitInformation, &limits as *const _ as *const c_void, std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32) != 0
            && AssignProcessToJobObject(job.0, process.0) != 0;
        if !held {
            let why = last_error("Holding the command in a job");
            TerminateProcess(process.0, 1);
            return Err(why);
        }
        ResumeThread(thread.0);
        WaitForSingleObject(process.0, INFINITE);
        let mut code = 1u32;
        GetExitCodeProcess(process.0, &mut code);
        Ok(code)
    }
}

/// A folder removed with everything in it when this is dropped.
struct Gone(PathBuf);

impl Drop for Gone {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The handle, made one a program started from here may be given.
fn inheritable(handle: HANDLE) -> HANDLE {
    // SAFETY: a handle of this process's own; only its flag changes.
    unsafe {
        SetHandleInformation(handle, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT);
    }
    handle
}

/// A clean environment, as on Linux: nothing of the person's own, where keys live; Windows's own folders;
/// of the person's `PATH`, what is outside their profile (what is in it, the sandbox's user cannot read);
/// and a home and temporary folder of the command's own.
fn environment(scratch: &Path) -> Vec<u16> {
    let mut vars: Vec<(String, String)> = Vec::new();
    for name in [
        "SystemRoot",
        "windir",
        "SystemDrive",
        "ComSpec",
        "PATHEXT",
        "OS",
        "PROCESSOR_ARCHITECTURE",
        "PROCESSOR_IDENTIFIER",
        "PROCESSOR_LEVEL",
        "PROCESSOR_REVISION",
        "NUMBER_OF_PROCESSORS",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "ProgramW6432",
        "CommonProgramFiles",
        "CommonProgramFiles(x86)",
        "CommonProgramW6432",
        "ProgramData",
        "ALLUSERSPROFILE",
    ] {
        if let Ok(value) = std::env::var(name) {
            vars.push((name.into(), value));
        }
    }
    let profile = std::env::var_os("USERPROFILE").map(PathBuf::from);
    let path: Vec<PathBuf> = std::env::var_os("PATH").iter().flat_map(std::env::split_paths).filter(|dir| profile.as_ref().is_none_or(|p| !dir.starts_with(p))).collect();
    if let Ok(path) = std::env::join_paths(path) {
        vars.push(("PATH".into(), path.to_string_lossy().into_owned()));
    }
    let home = scratch.join("home");
    for (name, value) in [
        ("TEMP", scratch.join("temp")),
        ("TMP", scratch.join("temp")),
        ("USERPROFILE", home.clone()),
        ("HOME", home.clone()),
        ("APPDATA", home.join(r"AppData\Roaming")),
        ("LOCALAPPDATA", home.join(r"AppData\Local")),
    ] {
        vars.push((name.into(), value.to_string_lossy().into_owned()));
    }
    // Windows wants them sorted, without regard to case.
    vars.sort_by_key(|(name, _)| name.to_uppercase());
    let mut block: Vec<u16> = Vec::new();
    for (name, value) in vars {
        block.extend(OsStr::new(&format!("{name}={value}")).encode_wide());
        block.push(0);
    }
    block.push(0);
    block
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn unhex(text: &str) -> Result<Vec<u8>, String> {
    if !text.len().is_multiple_of(2) {
        return Err("the script is damaged".into());
    }
    (0..text.len()).step_by(2).map(|i| u8::from_str_radix(&text[i..i + 2], 16).map_err(|_| "the script is damaged".to_string())).collect()
}

// ---------- demido sandbox-runner, as the sandbox's user ----------

/// Starts `cmd /D /S /C script` with a write-restricted copy of this program's token, which carries the
/// folder's SID (and its logon SID, and Everyone, as Codex does: without the logon SID a program cannot even
/// start), and waits for it.
pub fn run_restricted(folder_sid: &str, script: &str) -> Result<u32, String> {
    let token = own_token(TOKEN_DUPLICATE | TOKEN_QUERY | TOKEN_ASSIGN_PRIMARY | TOKEN_ADJUST_DEFAULT)?;
    let groups = token_info(token.0, TokenGroups)?;
    // SAFETY: the buffer holds a TOKEN_GROUPS, as asked for; its groups are read within its count.
    let logon: PSID = unsafe {
        let list = &*(groups.as_ptr() as *const TOKEN_GROUPS);
        let all = std::slice::from_raw_parts(list.Groups.as_ptr(), list.GroupCount as usize);
        all.iter().find(|g| g.Attributes & SE_GROUP_LOGON_ID == SE_GROUP_LOGON_ID).map(|g| g.Sid).ok_or("this program has no logon SID")?
    };
    let (folder, everyone) = (Sid::parse(folder_sid)?, Sid::parse(EVERYONE)?);
    let restricting = [SID_AND_ATTRIBUTES { Sid: folder.0, Attributes: 0 }, SID_AND_ATTRIBUTES { Sid: logon, Attributes: 0 }, SID_AND_ATTRIBUTES { Sid: everyone.0, Attributes: 0 }];
    // SAFETY: plain Win32 calls; every buffer outlives the call it is given to; handles are closed by Handle.
    unsafe {
        let mut restricted: HANDLE = std::ptr::null_mut();
        if CreateRestrictedToken(
            token.0,
            DISABLE_MAX_PRIVILEGE | LUA_TOKEN | WRITE_RESTRICTED,
            0,
            std::ptr::null(),
            0,
            std::ptr::null(),
            restricting.len() as u32,
            restricting.as_ptr(),
            &mut restricted,
        ) == 0
        {
            return Err(last_error("Making the command's token"));
        }
        let restricted = Handle(restricted);
        // What the command makes for itself is granted to its logon SID, which its token carries.
        let logon_sid = Sid::parse(&sid_text(logon)?)?;
        let entry = access(&logon_sid, GENERIC_ALL, GRANT_ACCESS, false);
        let mut dacl: *mut ACL = std::ptr::null_mut();
        if SetEntriesInAclW(1, &entry, std::ptr::null(), &mut dacl) != ERROR_SUCCESS {
            return Err(last_error("Making the command's own access list"));
        }
        let default = TOKEN_DEFAULT_DACL { DefaultDacl: dacl };
        let set = SetTokenInformation(restricted.0, TokenDefaultDacl, &default as *const _ as *const c_void, std::mem::size_of::<TOKEN_DEFAULT_DACL>() as u32);
        LocalFree(dacl as HLOCAL);
        if set == 0 {
            return Err(last_error("Giving the command its own access list"));
        }
        let cmd = std::env::var_os("ComSpec").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows\System32\cmd.exe"));
        // As without a sandbox (`sandbox::command`): the script as it is, the person's AutoRun commands not run.
        let mut line = wide(format!("\"{}\" /D /S /C \"{script}\"", cmd.display()));
        let cmd = wide(&cmd);
        let mut info: STARTUPINFOW = std::mem::zeroed();
        info.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
        info.dwFlags = STARTF_USESTDHANDLES;
        info.hStdInput = inheritable(GetStdHandle(STD_INPUT_HANDLE));
        info.hStdOutput = inheritable(GetStdHandle(STD_OUTPUT_HANDLE));
        info.hStdError = inheritable(GetStdHandle(STD_ERROR_HANDLE));
        let mut started: PROCESS_INFORMATION = std::mem::zeroed();
        if CreateProcessAsUserW(restricted.0, cmd.as_ptr(), line.as_mut_ptr(), std::ptr::null(), std::ptr::null(), 1, 0, std::ptr::null(), std::ptr::null(), &info, &mut started) == 0 {
            return Err(last_error("Starting the command"));
        }
        let (process, _thread) = (Handle(started.hProcess), Handle(started.hThread));
        WaitForSingleObject(process.0, INFINITE);
        let mut code = 1u32;
        GetExitCodeProcess(process.0, &mut code);
        Ok(code)
    }
}

// ---------- the Windows Filtering Platform: loopback too (IDEAS 148) ----------

/// Demido's own sublayer and filters, known by these keys so that a second setup replaces them.
const WFP_SUBLAYER: windows_sys::core::GUID = windows_sys::core::GUID::from_u128(0x4c7e_2f41_9b3a_4d6e_8a0f_d3e1_6b9c_2a57);
const WFP_FILTERS: [windows_sys::core::GUID; 4] = [
    windows_sys::core::GUID::from_u128(0x4c7e_2f41_9b3a_4d6e_8a0f_d3e1_6b9c_2a58),
    windows_sys::core::GUID::from_u128(0x4c7e_2f41_9b3a_4d6e_8a0f_d3e1_6b9c_2a59),
    windows_sys::core::GUID::from_u128(0x4c7e_2f41_9b3a_4d6e_8a0f_d3e1_6b9c_2a5a),
    windows_sys::core::GUID::from_u128(0x4c7e_2f41_9b3a_4d6e_8a0f_d3e1_6b9c_2a5b),
];

/// Every TCP or UDP connection the sandbox's user makes or accepts is blocked by the Windows Filtering Platform itself, which
/// the firewall is built on: Windows Firewall's rules leave loopback alone (research note 29: a block rule did not
/// stop the user reaching a server on 127.0.0.1), a filter does not. Four filters (connecting and accepting, IPv4
/// and IPv6), matching that user (`FWPM_CONDITION_ALE_USER_ID`, as the firewall rule names it), in a sublayer of
/// Demido's own of the highest weight, so that no permit of another sublayer decides first; kept across restarts.
/// Set at setup, as an administrator; a second setup replaces them.
pub fn shut_every_connection(user_sid: &str) -> Result<(), String> {
    use windows_sys::Win32::NetworkManagement::WindowsFilteringPlatform::*;
    use windows_sys::Win32::Security::Authorization::{ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1};

    let sddl = wide(format!("O:LSD:(A;;CC;;;{user_sid})"));
    let (mut sd, mut sd_size): (PSECURITY_DESCRIPTOR, u32) = (std::ptr::null_mut(), 0);
    // SAFETY: a valid wide string in; the descriptor out is freed below.
    if unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl.as_ptr(), SDDL_REVISION_1, &mut sd, &mut sd_size) } == 0 {
        return Err(last_error("Describing the sandbox's user for the filter"));
    }
    struct Freed(PSECURITY_DESCRIPTOR);
    impl Drop for Freed {
        fn drop(&mut self) {
            // SAFETY: allocated by ConvertStringSecurityDescriptorToSecurityDescriptorW, freed once.
            unsafe { LocalFree(self.0 as HLOCAL) };
        }
    }
    let _freed = Freed(sd);
    let mut blob = FWP_BYTE_BLOB { size: sd_size, data: sd as *mut u8 };

    let mut engine: HANDLE = std::ptr::null_mut();
    // SAFETY: the local engine, Windows's own authentication; the handle is closed below.
    let opened = unsafe { FwpmEngineOpen0(std::ptr::null(), windows_sys::Win32::System::Rpc::RPC_C_AUTHN_WINNT, std::ptr::null(), std::ptr::null(), &mut engine) };
    if opened != ERROR_SUCCESS {
        return Err(format!("The Windows Filtering Platform could not be opened (error {opened:#x}): is this an administrator?"));
    }
    struct Engine(HANDLE);
    impl Drop for Engine {
        fn drop(&mut self) {
            // SAFETY: opened by FwpmEngineOpen0, closed once.
            unsafe { FwpmEngineClose0(self.0) };
        }
    }
    let engine = Engine(engine);
    let failed = |what: &str, code: u32| format!("The Windows Filtering Platform refused {what} (error {code:#x}).");
    // SAFETY: an open engine.
    let began = unsafe { FwpmTransactionBegin0(engine.0, 0) };
    if began != ERROR_SUCCESS {
        return Err(failed("to begin", began));
    }
    let mut all = || -> Result<(), String> {
        // What an earlier setup left, if anything.
        for key in &WFP_FILTERS {
            // SAFETY: an open engine and a key; not found is fine.
            unsafe { FwpmFilterDeleteByKey0(engine.0, key) };
        }
        // SAFETY: as above.
        unsafe { FwpmSubLayerDeleteByKey0(engine.0, &WFP_SUBLAYER) };

        let (mut name, mut about) = (wide("Demido Studio sandbox"), wide("The programs a model runs: no TCP or UDP connection at all, this computer's own included."));
        // SAFETY: plain data; every pointer set below outlives the call that reads it.
        let mut sublayer: FWPM_SUBLAYER0 = unsafe { std::mem::zeroed() };
        sublayer.subLayerKey = WFP_SUBLAYER;
        sublayer.displayData = FWPM_DISPLAY_DATA0 { name: name.as_mut_ptr(), description: about.as_mut_ptr() };
        sublayer.flags = FWPM_SUBLAYER_FLAG_PERSISTENT;
        sublayer.weight = u16::MAX;
        // SAFETY: a filled sublayer, and no descriptor of its own (the default).
        let added = unsafe { FwpmSubLayerAdd0(engine.0, &sublayer, std::ptr::null_mut()) };
        if added != ERROR_SUCCESS {
            return Err(failed("Demido's sublayer", added));
        }
        let layers = [FWPM_LAYER_ALE_AUTH_CONNECT_V4, FWPM_LAYER_ALE_AUTH_CONNECT_V6, FWPM_LAYER_ALE_AUTH_RECV_ACCEPT_V4, FWPM_LAYER_ALE_AUTH_RECV_ACCEPT_V6];
        for (key, layer) in WFP_FILTERS.iter().zip(layers) {
            // That user, and TCP or UDP (conditions on one field are either-or, on different fields both). The
            // firewall's rules already shut every protocol beyond this computer; on loopback what matters is
            // reaching a server, and ICMP (ping) carries nothing to one.
            // SAFETY: plain data, filled below.
            let mut conditions: [FWPM_FILTER_CONDITION0; 3] = unsafe { std::mem::zeroed() };
            conditions[0].fieldKey = FWPM_CONDITION_ALE_USER_ID;
            conditions[0].matchType = FWP_MATCH_EQUAL;
            conditions[0].conditionValue.r#type = FWP_SECURITY_DESCRIPTOR_TYPE;
            conditions[0].conditionValue.Anonymous.sd = &mut blob;
            for (c, protocol) in conditions[1..].iter_mut().zip([6u8, 17u8]) {
                c.fieldKey = FWPM_CONDITION_IP_PROTOCOL;
                c.matchType = FWP_MATCH_EQUAL;
                c.conditionValue.r#type = FWP_UINT8;
                c.conditionValue.Anonymous.uint8 = protocol;
            }
            // SAFETY: plain data, filled below.
            let mut filter: FWPM_FILTER0 = unsafe { std::mem::zeroed() };
            filter.filterKey = *key;
            filter.displayData = FWPM_DISPLAY_DATA0 { name: name.as_mut_ptr(), description: about.as_mut_ptr() };
            filter.flags = FWPM_FILTER_FLAG_PERSISTENT;
            filter.layerKey = layer;
            filter.subLayerKey = WFP_SUBLAYER;
            filter.weight.r#type = FWP_UINT8;
            filter.weight.Anonymous.uint8 = 15;
            filter.numFilterConditions = conditions.len() as u32;
            filter.filterCondition = conditions.as_mut_ptr();
            filter.action.r#type = FWP_ACTION_BLOCK;
            // SAFETY: a filled filter whose pointers (condition, blob, names) live until the call returns.
            let added = unsafe { FwpmFilterAdd0(engine.0, &filter, std::ptr::null_mut(), std::ptr::null_mut()) };
            if added != ERROR_SUCCESS {
                return Err(failed("a filter", added));
            }
        }
        Ok(())
    };
    match all() {
        Ok(()) => {
            // SAFETY: an open engine with a transaction under way.
            let committed = unsafe { FwpmTransactionCommit0(engine.0) };
            if committed != ERROR_SUCCESS { Err(failed("to keep the filters", committed)) } else { Ok(()) }
        }
        Err(e) => {
            // SAFETY: as above.
            unsafe { FwpmTransactionAbort0(engine.0) };
            Err(e)
        }
    }
}

// ---------- demido sandbox-setup, once, as an administrator ----------

/// What setup asks PowerShell to do, as an administrator: the user (made, or its password changed), off the
/// sign-in screen, and the firewall's rules that shut its network, both ways. The password comes on its
/// standard input, never on a command line. Refused when the firewall is off or a policy makes rules made on
/// this computer count for nothing: then the network would not be shut, and the sandbox would say it was.
const SETUP: &str = r#"
$ErrorActionPreference = 'Stop'
$name = 'DemidoSandbox'
$secure = ConvertTo-SecureString ([Console]::In.ReadLine()) -AsPlainText -Force
$off = @(Get-NetFirewallProfile | Where-Object { -not $_.Enabled } | ForEach-Object { $_.Name })
if ($off.Count -gt 0) { [Console]::Error.WriteLine("Windows Firewall is off for the $($off -join ', ') network profile, so the sandbox's network could not be shut. Turn it on, or ask whoever looks after this computer."); exit 3 }
if ((New-Object -ComObject HNetCfg.FwPolicy2).LocalPolicyModifyState -ne 0) { [Console]::Error.WriteLine("A policy on this computer makes firewall rules made here count for nothing, so the sandbox's network could not be shut."); exit 3 }
if (Get-LocalUser -Name $name -ErrorAction SilentlyContinue) {
  Set-LocalUser -Name $name -Password $secure -AccountNeverExpires -PasswordNeverExpires $true -UserMayChangePassword $false
} else {
  New-LocalUser -Name $name -Password $secure -AccountNeverExpires -PasswordNeverExpires -UserMayNotChangePassword -Description 'Demido Studio: the programs a model runs' | Out-Null
}
try { Add-LocalGroupMember -SID 'S-1-5-32-545' -Member $name } catch { }
$list = 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon\SpecialAccounts\UserList'
New-Item $list -Force | Out-Null
New-ItemProperty $list -Name $name -Value 0 -PropertyType DWord -Force | Out-Null
$sid = (Get-LocalUser $name).SID.Value
foreach ($rule in 'DemidoSandboxOut', 'DemidoSandboxIn') { Remove-NetFirewallRule -Name $rule -ErrorAction SilentlyContinue }
$who = "O:LSD:(A;;CC;;;$sid)"
New-NetFirewallRule -Name 'DemidoSandboxOut' -DisplayName 'Demido Studio sandbox: no network out' -Direction Outbound -Action Block -LocalUser $who -Profile Any | Out-Null
New-NetFirewallRule -Name 'DemidoSandboxIn' -DisplayName 'Demido Studio sandbox: no network in' -Direction Inbound -Action Block -LocalUser $who -Profile Any | Out-Null
"SID=$sid"
"#;

/// Sets the sandbox up, for the person whose SID is `person`, keeping what it made in their Demido folder.
/// Runs as an administrator, which may be someone other than the person.
pub fn setup(home: &Path, person: &str) -> Result<(), String> {
    use std::io::Write;
    let password = format!("{}{}aZ7!", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple());
    let system = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    let powershell = system.join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
    let mut child = std::process::Command::new(&powershell)
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", SETUP])
        // Started from PowerShell 7, the variable names 7's modules, which Windows PowerShell cannot load
        // ("ConvertTo-SecureString was found in the module …, but the module could not be loaded").
        .env_remove("PSModulePath")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("PowerShell could not be started: {e}"))?;
    if let Some(mut input) = child.stdin.take() {
        let _ = writeln!(input, "{password}");
    }
    let out = child.wait_with_output().map_err(|e| format!("PowerShell did not finish: {e}"))?;
    if !out.status.success() {
        let said = String::from_utf8_lossy(&out.stderr);
        return Err(said.trim().lines().find(|l| !l.trim().is_empty()).unwrap_or("PowerShell failed without saying why").trim().to_string());
    }
    let printed = String::from_utf8_lossy(&out.stdout);
    let sid = printed.lines().find_map(|l| l.trim().strip_prefix("SID=")).ok_or("the user was made, and its SID could not be read")?.to_string();
    // Loopback too, which the firewall's rules leave alone (IDEAS 148). Without it, no sandbox.
    shut_every_connection(&sid)?;
    // Its place: only the administrators and the person may change it; the user may read and run.
    let (user, me, system, admins) = (Sid::parse(&sid)?, Sid::parse(person)?, Sid::parse(SYSTEM)?, Sid::parse(ADMINISTRATORS)?);
    private_folder(&place(), &[(&system, FULL), (&admins, FULL), (&me, FULL), (&user, READ_AND_RUN)])?;
    std::fs::create_dir_all(place().join("scratch")).map_err(|e| format!("{} could not be made: {e}", place().display()))?;
    // Kept for the person. When the administrator is the person, in their own DPAPI scope at once; when not,
    // in the machine's, until the person's own Demido takes it into theirs (`kept`).
    let mine = own_sid().is_ok_and(|s| s.eq_ignore_ascii_case(person));
    let key = protect(password.as_bytes(), !mine)?;
    let kept = Kept {
        user: USER.into(),
        sid,
        salt: uuid::Uuid::new_v4().simple().to_string(),
        key: base64::engine::general_purpose::STANDARD.encode(key),
        scope: if mine { "person" } else { "machine" }.into(),
    };
    std::fs::create_dir_all(home.join("sandbox")).map_err(|e| format!("Demido's folder could not be written: {e}"))?;
    std::fs::write(kept_path(home), serde_json::to_string_pretty(&kept).map_err(|e| e.to_string())?).map_err(|e| format!("what setup made could not be kept: {e}"))?;
    Ok(())
}

/// Starts setup as an administrator: Windows asks the person (or someone who may) first. Waits for it.
pub fn start_setup(home: &Path) -> Result<(), String> {
    use windows_sys::Win32::UI::Shell::{SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW};
    let me = std::env::current_exe().map_err(|e| format!("this program cannot be found: {e}"))?;
    let failed = home.join("sandbox").join("setup-failed.txt");
    let _ = std::fs::remove_file(&failed);
    let parameters = format!("sandbox-setup --home \"{}\" --for {}", home.display(), own_sid()?);
    let (verb, file, parameters) = (wide("runas"), wide(&me), wide(parameters));
    // SAFETY: the strings outlive the call; the process handle is closed by Handle.
    unsafe {
        let mut info: SHELLEXECUTEINFOW = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC;
        info.lpVerb = verb.as_ptr();
        info.lpFile = file.as_ptr();
        info.lpParameters = parameters.as_ptr();
        info.nShow = 0; // SW_HIDE
        if ShellExecuteExW(&mut info) == 0 {
            const ERROR_CANCELLED: u32 = 1223;
            return Err(if GetLastError() == ERROR_CANCELLED {
                "Windows asked for an administrator, and was told no: the sandbox was not set up.".into()
            } else {
                last_error("Starting the sandbox's setup")
            });
        }
        let process = Handle(info.hProcess);
        WaitForSingleObject(process.0, INFINITE);
        let mut code = 1u32;
        GetExitCodeProcess(process.0, &mut code);
        if code != 0 {
            return Err(std::fs::read_to_string(&failed).unwrap_or_else(|_| format!("The sandbox's setup failed (exit code {code}).")));
        }
    }
    ready(home).map(|_| ())
}

/// `demido sandbox-setup`'s own end: what went wrong is left where `start_setup` reads it, since nobody sees
/// this program's output.
pub fn setup_and_tell(home: &Path, person: &str) -> i32 {
    match setup(home, person) {
        Ok(()) => 0,
        Err(why) => {
            let _ = std::fs::create_dir_all(home.join("sandbox"));
            let _ = std::fs::write(home.join("sandbox").join("setup-failed.txt"), &why);
            eprintln!("{why}");
            1
        }
    }
}
