//! Checks that an installer was signed with Demido Studio's update key. The app checks the
//! installer it downloads before running it; setup checks the copy of itself it leaves behind as
//! the uninstaller when it runs elevated from a folder the user can write to.
//!
//! Releases are signed with `tauri signer sign --app-version <version>`: the `.sig` file is the
//! base64 of a minisign signature, and [`PUBLIC_KEY`] is the base64 of a minisign public key
//! file. The signature's trusted comment, which the signature covers, names the file and the
//! version. Requiring both ties a signature to one release, so an older installer, validly
//! signed, cannot be passed off as a newer version.

use std::path::Path;

use anyhow::{Context, bail};
use base64::Engine;
use minisign_verify::{PublicKey, Signature};

/// The public half of the key that signs releases. Its private half lives only in the release
/// workflow's secrets.
pub const PUBLIC_KEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEE4MTY2MENERkZEMDQwQTUKUldTbFFORC96V0FXcUNIaDRkbzAvQWVHWUppQ2RzRVNzbDFJZmYwekllb2drRTlPRzlwOUkwR2QK";

/// Verifies `data` against `signature_b64` (the `.sig` text) made by the key `public_key_b64`,
/// for exactly `file_name` at `version`.
pub fn verify(
    data: &[u8],
    signature_b64: &str,
    public_key_b64: &str,
    file_name: &str,
    version: &str,
) -> anyhow::Result<()> {
    let key = decode_text(public_key_b64)
        .and_then(|text| PublicKey::decode(&text).map_err(anyhow::Error::from))
        .context("the update key is not valid")?;
    let signature = decode_text(signature_b64)
        .and_then(|text| Signature::decode(&text).map_err(anyhow::Error::from))
        .context("the signature file is not valid")?;
    // Legacy (not pre-hashed) signatures are accepted too, as tauri-plugin-updater does.
    key.verify(data, &signature, true)
        .context("the signature does not match the file")?;

    let comment = signature.trusted_comment();
    let fields: Vec<&str> = comment.split('\t').collect();
    if !fields.contains(&format!("file:{file_name}").as_str()) {
        bail!("the signature is for another file ({comment})");
    }
    if !fields.contains(&format!("version:{version}").as_str()) {
        bail!("the signature is for another version ({comment})");
    }
    Ok(())
}

/// [`verify`] for a file on disk.
pub fn verify_file(
    path: &Path,
    signature_b64: &str,
    public_key_b64: &str,
    file_name: &str,
    version: &str,
) -> anyhow::Result<()> {
    let data = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    verify(&data, signature_b64, public_key_b64, file_name, version)
}

/// Both the key and the signature are base64 of a minisign text file.
fn decode_text(b64: &str) -> anyhow::Result<String> {
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64.trim())?;
    Ok(String::from_utf8(bytes)?)
}

/// A throwaway key and a file it signed, for tests on both sides of the contract. It signs nothing
/// real: the release key is [`PUBLIC_KEY`].
#[doc(hidden)]
pub mod fixtures {
    // A throwaway key made for these tests only; it signs nothing real.
    pub const TEST_PUBLIC_KEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEExNkE3NkYxRUM3ODcyQwpSV1FzaDhjZWI2Y1dDdFZBK1NBSzRCbmxFRzFpWENzUFBMQnZ3SDBlU1JGS1JPSWtlTXcxSUdFWAo=";
    pub const TEST_SIGNATURE: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVRc2g4Y2ViNmNXQ3ZDMTI3ZUdDVlFXTDJYblVOUWl1amJJOWtjbmxYNVRHcXhpTlcrL1hQSDlqcmprVlhtUWwrNHlkYTk5M0ErQk1XOWpoSEU2RnlTcnhSdEV5eVc5U2dnPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzkwNTIwODY0CWZpbGU6RGVtaWRvLVN0dWRpby1TZXR1cC05LjkuOS5leGUJdmVyc2lvbjo5LjkuOQpySEVZaVRjTXdSQ3Z5Tk5Vc3o0RUtidFVQbmh1cDdwZ3hIakJKN3B1U1R6V205TUJGRnhyZDBXY2pkMnVsMWtqWkFNUGI2YVdGMm5Za2tWd1dvc25Bdz09Cg==";
    pub const TEST_DATA: &[u8] = b"demido update test payload";
    pub const TEST_FILE: &str = "Demido-Studio-Setup-9.9.9.exe";
    pub const TEST_VERSION: &str = "9.9.9";
}

#[cfg(test)]
mod tests {
    use super::fixtures::*;
    use super::*;

    #[test]
    fn a_valid_signature_passes() {
        verify(TEST_DATA, TEST_SIGNATURE, TEST_PUBLIC_KEY, TEST_FILE, TEST_VERSION).unwrap();
    }

    #[test]
    fn one_changed_byte_fails() {
        let mut data = TEST_DATA.to_vec();
        data[3] ^= 0x01;
        assert!(verify(&data, TEST_SIGNATURE, TEST_PUBLIC_KEY, TEST_FILE, TEST_VERSION).is_err());
    }

    #[test]
    fn the_release_key_rejects_a_signature_from_another_key() {
        assert!(verify(TEST_DATA, TEST_SIGNATURE, PUBLIC_KEY, TEST_FILE, TEST_VERSION).is_err());
    }

    #[test]
    fn the_release_key_is_a_valid_minisign_key() {
        let text = decode_text(PUBLIC_KEY).unwrap();
        PublicKey::decode(&text).unwrap();
    }

    #[test]
    fn a_signature_only_vouches_for_its_own_file_and_version() {
        let err = verify(
            TEST_DATA,
            TEST_SIGNATURE,
            TEST_PUBLIC_KEY,
            "Demido-Studio-Setup-9.9.10.exe",
            TEST_VERSION,
        )
        .unwrap_err();
        assert!(err.to_string().contains("another file"), "{err}");
        let err = verify(TEST_DATA, TEST_SIGNATURE, TEST_PUBLIC_KEY, TEST_FILE, "9.9.10").unwrap_err();
        assert!(err.to_string().contains("another version"), "{err}");
        // A prefix of the real version is not the version.
        assert!(verify(TEST_DATA, TEST_SIGNATURE, TEST_PUBLIC_KEY, TEST_FILE, "9.9").is_err());
    }

    #[test]
    fn garbage_is_an_error_not_a_panic() {
        for signature in ["", "not base64 at all!", "aGVsbG8gd29ybGQ=", "////", "\u{0}\u{1}"] {
            assert!(verify(TEST_DATA, signature, TEST_PUBLIC_KEY, TEST_FILE, TEST_VERSION).is_err());
        }
        assert!(verify(TEST_DATA, TEST_SIGNATURE, "garbage", TEST_FILE, TEST_VERSION).is_err());
    }

    #[test]
    fn surrounding_whitespace_in_the_sig_file_is_ignored() {
        let padded = format!("{TEST_SIGNATURE}\r\n");
        verify(TEST_DATA, &padded, TEST_PUBLIC_KEY, TEST_FILE, TEST_VERSION).unwrap();
    }

    #[test]
    fn files_verify_from_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(TEST_FILE);
        std::fs::write(&path, TEST_DATA).unwrap();
        verify_file(&path, TEST_SIGNATURE, TEST_PUBLIC_KEY, TEST_FILE, TEST_VERSION).unwrap();
        assert!(
            verify_file(
                &dir.path().join("missing.exe"),
                TEST_SIGNATURE,
                TEST_PUBLIC_KEY,
                TEST_FILE,
                TEST_VERSION
            )
            .is_err()
        );
    }
}
