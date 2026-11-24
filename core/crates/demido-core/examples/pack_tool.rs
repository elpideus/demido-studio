//! Packs a tool's folder into a package and signs it, as a publisher would (DECISIONS C7, C8):
//!
//!     cargo run --manifest-path core/Cargo.toml --example pack_tool -- <folder> <key file> <out dir>
//!
//! The folder holds `tool.json` (whose `publisher_key` is filled in from the key), `guidance.md` and `tool.wasm`.
//! The key file is a minisign secret key without a password; it is made when it is not there, with its public
//! key beside it (`<key file>.pub`). Keep it out of the repository: whoever holds it can sign as you.
//! Writes `<name>-<version>.demido-tool` and its `.minisig` into the out dir.

use std::io::Write;
use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [folder, key, out] = &args[..] else {
        eprintln!("usage: pack_tool <folder> <key file> <out dir>");
        std::process::exit(2);
    };
    let (folder, key, out) = (PathBuf::from(folder), PathBuf::from(key), PathBuf::from(out));
    let pair = if key.is_file() {
        let sk = minisign::SecretKey::from_file(&key, Some(String::new())).expect("a minisign secret key without a password");
        let pk = minisign::PublicKey::from_file(key.with_extension("pub")).expect("its public key beside it, <key>.pub");
        minisign::KeyPair { pk, sk }
    } else {
        let pair = minisign::KeyPair::generate_unencrypted_keypair().expect("a key can be made");
        if let Some(dir) = key.parent() {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::write(&key, pair.sk.to_box(None).unwrap().to_string()).unwrap();
        std::fs::write(key.with_extension("pub"), pair.pk.to_box().unwrap().to_string()).unwrap();
        eprintln!("Made a key: {} (secret) and its .pub", key.display());
        pair
    };
    let mut manifest: serde_json::Value = serde_json::from_slice(&std::fs::read(folder.join("tool.json")).expect("tool.json")).expect("tool.json is JSON");
    manifest["publisher_key"] = serde_json::json!(pair.pk.to_base64());
    let name = format!("{}-{}", manifest["name"].as_str().unwrap(), manifest["version"].as_str().unwrap());
    let mut tar = tar::Builder::new(Vec::new());
    for (file, bytes) in [
        ("tool.json", serde_json::to_vec_pretty(&manifest).unwrap()),
        ("guidance.md", std::fs::read(folder.join("guidance.md")).unwrap_or_default()),
        ("tool.wasm", std::fs::read(folder.join("tool.wasm")).expect("tool.wasm")),
    ] {
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_mtime(0);
        header.set_cksum();
        tar.append_data(&mut header, file, bytes.as_slice()).unwrap();
    }
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gz.write_all(&tar.into_inner().unwrap()).unwrap();
    let archive = gz.finish().unwrap();
    std::fs::create_dir_all(&out).unwrap();
    let package = out.join(format!("{name}.demido-tool"));
    std::fs::write(&package, &archive).unwrap();
    let sig = minisign::sign(Some(&pair.pk), &pair.sk, std::io::Cursor::new(&archive), Some(&format!("{name}.demido-tool")), None).unwrap();
    std::fs::write(out.join(format!("{name}.demido-tool.minisig")), sig.to_string()).unwrap();
    println!("{}", package.display());
}
