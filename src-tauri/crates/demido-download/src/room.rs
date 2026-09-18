//! Free space on the volume a download lands on.
//!
//! Asked before a transfer starts, against what is still to come, so a
//! seventeen gigabyte model on a drive with ten is refused while it costs
//! nothing rather than at 98 per cent.

use std::path::Path;

/// Bytes free to this user on the volume holding `path`, or `None` where the
/// platform would not say. `path` need not exist yet: a download folder is
/// made by the first download into it, so the nearest folder that does exist
/// is the one asked about.
pub fn free(path: &Path) -> Option<u64> {
    let existing = path.ancestors().find(|folder| folder.is_dir())?;
    free_at(existing)
}

#[cfg(windows)]
fn free_at(folder: &Path) -> Option<u64> {
    use windows::core::HSTRING;
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    let name = HSTRING::from(folder.as_os_str());
    let mut available = 0u64;
    // SAFETY: the name is a NUL-terminated wide string that outlives the call,
    // and the one out-pointer is a `u64` this frame owns.
    unsafe { GetDiskFreeSpaceExW(&name, Some(&mut available), None, None) }.ok()?;
    Some(available)
}

/// Windows first, per the brief. Elsewhere nothing is refused up front, and a
/// full disk is still named when the write fails.
#[cfg(not(windows))]
fn free_at(_folder: &Path) -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn a_folder_that_does_not_exist_yet_is_asked_about_through_its_parent() {
        let here = std::env::temp_dir();
        let later = here.join("demido-room").join("not-yet").join("models");
        assert!(free(&here).is_some_and(|bytes| bytes > 0));
        assert_eq!(free(&later).is_some(), free(&here).is_some());
    }
}
