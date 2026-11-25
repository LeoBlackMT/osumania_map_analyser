//! Release check: verify the exact files to be published with the updater's public key.
use mma_shell::osu::offsets::{
    verify_table_signature, RemoteManifest, DEFAULT_LAZER_TABLE_JSON, DEFAULT_STABLE_TABLE_JSON,
    ED25519_MASTER_PUBLIC_KEY,
};
use std::{env, fs, path::PathBuf, process::ExitCode};

fn main() -> Result<ExitCode, Box<dyn std::error::Error>> {
    let root = env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("offsets"));
    let manifest: RemoteManifest = serde_json::from_slice(&fs::read(root.join("manifest.json"))?)?;
    if manifest.schema_version != 2 {
        return Err("unsupported manifest schema".into());
    }
    let mut ok = true;
    let mut has_default = [false; 2];
    for entry in manifest.tables {
        let path = root.join(&entry.client).join(&entry.filename);
        let bytes = fs::read(&path)?;
        match entry.client.as_str() {
            "stable" => has_default[0] |= bytes == DEFAULT_STABLE_TABLE_JSON.as_bytes(),
            "lazer" => has_default[1] |= bytes == DEFAULT_LAZER_TABLE_JSON.as_bytes(),
            _ => {}
        }
        match verify_table_signature(
            &bytes,
            &entry.sha256,
            &entry.signature,
            &ED25519_MASTER_PUBLIC_KEY,
        ) {
            Ok(()) => println!("OK   {}/{}", entry.client, entry.filename),
            Err(reason) => {
                ok = false;
                eprintln!("FAIL {}/{}: {reason}", entry.client, entry.filename);
            }
        }
    }
    for (found, client) in has_default.into_iter().zip(["stable", "lazer"]) {
        if !found {
            ok = false;
            eprintln!("FAIL embedded {client} table missing from manifest");
        }
    }
    Ok(if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}
