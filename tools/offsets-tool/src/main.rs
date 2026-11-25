//! Offline offsets manifest signer / verifier.
//!
//! Usage:
//!   offsets-tool verify --root <offsets-dir> [--manifest <path>]
//!   offsets-tool sign   --root <offsets-dir> --seed-file <64-hex-file> [--updated-at <RFC3339>]
//!
//! The signing seed must stay outside the repository. This tool never prints it.
//! This crate is deliberately standalone: it must not pull in the Tauri shell.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use ed25519_compact::{KeyPair, PublicKey, Seed, Signature};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const MANIFEST_SCHEMA_VERSION: u32 = 3;

// Must match desktop/src/osu/offsets/remote.rs::ED25519_MASTER_PUBLIC_KEY.
const ED25519_MASTER_PUBLIC_KEY: [u8; 32] = [
    206, 229, 44, 102, 228, 179, 117, 67, 18, 115, 220, 52, 238, 212, 17, 70, 95, 211, 253, 148,
    249, 231, 59, 153, 109, 26, 243, 174, 121, 135, 167, 119,
];

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ManifestTable {
    filename: String,
    client: String,
    game_version: String,
    runtime_version: String,
    arch: String,
    #[serde(default)]
    revision: u64,
    sha256: String,
    signature: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Manifest {
    schema_version: u32,
    updated_at: String,
    tables: Vec<ManifestTable>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    manifest_signature: Option<String>,
}

impl Manifest {
    fn signing_bytes(&self) -> Vec<u8> {
        let mut copy = self.clone();
        copy.manifest_signature = None;
        serde_json::to_vec(&copy).expect("serialize manifest")
    }

    fn verify_manifest_signature(&self) -> Result<(), String> {
        let signature = self
            .manifest_signature
            .as_deref()
            .ok_or_else(|| "manifest_signature_missing".to_string())?;
        let signature_bytes = hex_decode(signature)?;
        let signature = Signature::from_slice(&signature_bytes)
            .map_err(|_| "signature_length_error".to_string())?;
        let public_key = PublicKey::from_slice(&ED25519_MASTER_PUBLIC_KEY)
            .map_err(|_| "invalid_public_key".to_string())?;
        public_key
            .verify(&self.signing_bytes(), &signature)
            .map_err(|_| "manifest_signature_verification_failed".to_string())
    }
}

fn usage() -> ExitCode {
    eprintln!(
        "usage:\n  offsets-tool verify --root <offsets-dir> [--manifest <path>]\n  offsets-tool sign --root <offsets-dir> --seed-file <64-hex-file> [--updated-at <RFC3339>]"
    );
    ExitCode::from(2)
}

fn arg_value(args: &[String], name: &str) -> Option<String> {
    args.windows(2)
        .find(|window| window[0] == name)
        .map(|window| window[1].clone())
}

fn default_root() -> PathBuf {
    // tools/offsets-tool -> repo root -> desktop/offsets
    let tool_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    tool_dir
        .parent()
        .and_then(|path| path.parent())
        .map(|root| root.join("desktop").join("offsets"))
        .unwrap_or_else(|| PathBuf::from("desktop/offsets"))
}

fn manifest_path(root: &Path, args: &[String]) -> PathBuf {
    arg_value(args, "--manifest").map_or_else(|| root.join("manifest.json"), PathBuf::from)
}

fn packaged_table_path(root: &Path, client: &str, filename: &str) -> PathBuf {
    let direct = root.join(client).join(filename);
    if direct.is_file() {
        return direct;
    }
    if client == "ppysb" {
        let shared = root.join("stable").join(filename);
        if shared.is_file() {
            return shared;
        }
    }
    direct
}

fn hex_decode(value: &str) -> Result<Vec<u8>, String> {
    let value = value.trim();
    if value.len() % 2 != 0 {
        return Err("hex length must be even".to_string());
    }
    let mut out = Vec::with_capacity(value.len() / 2);
    for chunk in value.as_bytes().chunks_exact(2) {
        let high = (chunk[0] as char)
            .to_digit(16)
            .ok_or_else(|| "invalid hex".to_string())?;
        let low = (chunk[1] as char)
            .to_digit(16)
            .ok_or_else(|| "invalid hex".to_string())?;
        out.push(((high << 4) | low) as u8);
    }
    Ok(out)
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn verify_table_signature(
    content: &[u8],
    expected_sha256: &str,
    signature_str: &str,
) -> Result<(), String> {
    let actual_sha256 = sha256_hex(content);
    if !actual_sha256.eq_ignore_ascii_case(expected_sha256.trim()) {
        return Err("sha256_mismatch".to_string());
    }
    let signature_bytes = hex_decode(signature_str)?;
    let signature = Signature::from_slice(&signature_bytes)
        .map_err(|_| "signature_length_error".to_string())?;
    let public_key = PublicKey::from_slice(&ED25519_MASTER_PUBLIC_KEY)
        .map_err(|_| "invalid_public_key".to_string())?;
    public_key
        .verify(content, &signature)
        .map_err(|_| "signature_verification_failed".to_string())
}

fn utc_now_rfc3339() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let days = seconds.div_euclid(86_400);
    let rem = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3_600,
        (rem % 3_600) / 60,
        rem % 60
    )
}

// Howard Hinnant's civil_from_days.
fn civil_from_days(days_since_epoch: i64) -> (i64, u32, u32) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    (
        if month <= 2 { year + 1 } else { year },
        month as u32,
        day as u32,
    )
}

fn read_seed(args: &[String]) -> Result<[u8; 32], String> {
    let value = if let Some(path) = arg_value(args, "--seed-file") {
        fs::read_to_string(&path).map_err(|error| format!("read seed file {path}: {error}"))?
    } else {
        env::var("MMA_OFFSETS_SIGNING_SEED")
            .map_err(|_| "missing --seed-file or MMA_OFFSETS_SIGNING_SEED".to_string())?
    };
    let bytes = hex_decode(&value)?;
    let length = bytes.len();
    bytes
        .try_into()
        .map_err(|_| format!("seed must be 32 bytes (64 hex), got {length}"))
}

fn lazer_revision_from_version(version: &str) -> u64 {
    let clean = version.split('-').next().unwrap_or(version).trim();
    let mut parts = clean.split('.');
    let Some(year) = parts.next().and_then(|part| part.parse::<u64>().ok()) else {
        return 0;
    };
    let Some(month_day) = parts.next().and_then(|part| part.parse::<u64>().ok()) else {
        return 0;
    };
    if year == 0 || month_day == 0 {
        return 0;
    }
    year * 10_000 + month_day
}

fn is_supported_client(client: &str) -> bool {
    matches!(client, "stable" | "lazer" | "ppysb")
}

fn is_safe_table_filename(filename: &str) -> bool {
    if filename.is_empty() || filename == "." || filename == ".." {
        return false;
    }
    if filename.contains('/') || filename.contains('\\') {
        return false;
    }
    if !filename
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
    {
        return false;
    }
    let mut components = Path::new(filename).components();
    matches!(components.next(), Some(std::path::Component::Normal(_)))
        && components.next().is_none()
}

fn json_string(value: &serde_json::Value, key: &str) -> Result<String, String> {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("missing string field: {key}"))
}

fn table_entry(
    root: &Path,
    client: &str,
    folder: &str,
    filename: &str,
    keypair: &KeyPair,
) -> Result<ManifestTable, String> {
    let path = root.join(folder).join(filename);
    let bytes = fs::read(&path).map_err(|error| format!("read {}: {error}", path.display()))?;
    if bytes.windows(2).any(|pair| pair == b"\r\n") {
        return Err(format!(
            "{} uses CRLF; offsets tables must be LF",
            path.display()
        ));
    }
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|error| format!("parse {}: {error}", path.display()))?;

    let (game_version, runtime_version, arch, revision) = match client {
        "lazer" => {
            let version = json_string(&value, "lazer_version")?;
            let runtime = json_string(&value, "runtime_version")?;
            let arch = json_string(&value, "arch")?;
            let revision = lazer_revision_from_version(&version);
            (version, runtime, arch, revision)
        }
        "stable" | "ppysb" => {
            let version = json_string(&value, "version")?;
            let arch = json_string(&value, "arch")?;
            let revision = value
                .get("revision")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            (version, "CLRv4".to_string(), arch, revision)
        }
        _ => return Err(format!("unsupported client: {client}")),
    };
    if revision == 0 {
        return Err(format!("{} has revision 0", path.display()));
    }

    let signature = keypair.sk.sign(&bytes, None);
    let signature_hex = hex_encode(signature.as_ref());
    let sha256 = sha256_hex(&bytes);
    verify_table_signature(&bytes, &sha256, &signature_hex)
        .map_err(|error| format!("self-check failed for {}: {error}", path.display()))?;

    Ok(ManifestTable {
        filename: filename.to_string(),
        client: client.to_string(),
        game_version,
        runtime_version,
        arch,
        revision,
        sha256,
        signature: signature_hex,
    })
}

fn canonical_lazer_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    let dir = root.join("lazer");
    let entries = fs::read_dir(&dir).map_err(|error| format!("read {}: {error}", dir.display()))?;
    let mut by_key: BTreeMap<(String, String, String), PathBuf> = BTreeMap::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let bytes = fs::read(&path).map_err(|error| format!("read {}: {error}", path.display()))?;
        let value: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|error| format!("parse {}: {error}", path.display()))?;
        let key = (
            json_string(&value, "lazer_version")?,
            json_string(&value, "runtime_version")?,
            json_string(&value, "arch")?,
        );
        let replace = by_key.get(&key).map_or(true, |current| {
            let new_len = path.file_name().map_or(0, |value| value.len());
            let current_len = current.file_name().map_or(0, |value| value.len());
            new_len > current_len
        });
        if replace {
            by_key.insert(key, path);
        }
    }
    Ok(by_key.into_values().collect())
}

fn build_manifest(root: &Path, keypair: &KeyPair, updated_at: String) -> Result<Manifest, String> {
    let mut tables = Vec::new();
    tables.push(table_entry(
        root,
        "stable",
        "stable",
        "stable__x86.json",
        keypair,
    )?);
    let ppysb_path = if root.join("stable").join("ppysb__x86.json").is_file() {
        Some("stable")
    } else if root.join("ppysb").join("ppysb__x86.json").is_file() {
        Some("ppysb")
    } else {
        None
    };
    if let Some(folder) = ppysb_path {
        tables.push(table_entry(
            root,
            "ppysb",
            folder,
            "ppysb__x86.json",
            keypair,
        )?);
    }
    for path in canonical_lazer_files(root)? {
        let filename = path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| format!("invalid filename: {}", path.display()))?;
        tables.push(table_entry(root, "lazer", "lazer", filename, keypair)?);
    }
    tables.sort_by(|left, right| {
        (left.client.as_str(), left.revision, left.filename.as_str()).cmp(&(
            right.client.as_str(),
            right.revision,
            right.filename.as_str(),
        ))
    });
    Ok(Manifest {
        schema_version: MANIFEST_SCHEMA_VERSION,
        updated_at,
        tables,
        manifest_signature: None,
    })
}

fn cmd_verify(args: &[String]) -> Result<(), String> {
    let root = arg_value(args, "--root").map_or_else(default_root, PathBuf::from);
    let path = manifest_path(&root, args);
    let manifest_bytes =
        fs::read(&path).map_err(|error| format!("read {}: {error}", path.display()))?;
    let manifest: Manifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|error| format!("parse manifest: {error}"))?;
    if manifest.schema_version != MANIFEST_SCHEMA_VERSION {
        return Err(format!(
            "manifest schema_version {}, expected {}",
            manifest.schema_version, MANIFEST_SCHEMA_VERSION
        ));
    }
    if manifest.tables.is_empty() {
        return Err("manifest has no tables".to_string());
    }
    manifest
        .verify_manifest_signature()
        .map_err(|error| format!("manifest signature: {error}"))?;
    let mut seen = std::collections::HashSet::new();
    for table in &manifest.tables {
        if !is_supported_client(&table.client) {
            return Err(format!("unsupported client: {}", table.client));
        }
        if !is_safe_table_filename(&table.filename) {
            return Err(format!("unsafe filename: {}", table.filename));
        }
        if !seen.insert((table.client.clone(), table.filename.clone())) {
            return Err(format!(
                "duplicate table: {}/{}",
                table.client, table.filename
            ));
        }
        let path = packaged_table_path(&root, &table.client, &table.filename);
        let bytes = fs::read(&path).map_err(|error| format!("read {}: {error}", path.display()))?;
        if bytes.windows(2).any(|pair| pair == b"\r\n") {
            return Err(format!("{} uses CRLF", path.display()));
        }
        verify_table_signature(&bytes, &table.sha256, &table.signature)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        let value: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|error| format!("parse {}: {error}", path.display()))?;
        let revision = match table.client.as_str() {
            "lazer" => lazer_revision_from_version(&json_string(&value, "lazer_version")?),
            "stable" | "ppysb" => value
                .get("revision")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0),
            _ => unreachable!(),
        };
        if table.revision != revision {
            return Err(format!(
                "{}: manifest revision {} != table revision {}",
                path.display(),
                table.revision,
                revision
            ));
        }
        println!("OK {} revision={}", table.filename, table.revision);
    }
    println!("OK manifest: {} table(s)", manifest.tables.len());
    Ok(())
}

fn cmd_sign(args: &[String]) -> Result<(), String> {
    let root = arg_value(args, "--root").map_or_else(default_root, PathBuf::from);
    let seed = read_seed(args)?;
    let seed = Seed::from_slice(&seed).map_err(|error| format!("invalid seed: {error}"))?;
    let keypair = KeyPair::from_seed(seed);
    let updated_at = arg_value(args, "--updated-at").unwrap_or_else(utc_now_rfc3339);
    let mut manifest = build_manifest(&root, &keypair, updated_at)?;
    let signature = keypair.sk.sign(&manifest.signing_bytes(), None);
    manifest.manifest_signature = Some(hex_encode(signature.as_ref()));
    let path = manifest_path(&root, args);
    let json = serde_json::to_string_pretty(&manifest)
        .map_err(|error| format!("serialize manifest: {error}"))?;
    let temp = path.with_extension("json.tmp");
    let mut file =
        fs::File::create(&temp).map_err(|error| format!("create {}: {error}", temp.display()))?;
    file.write_all(json.as_bytes())
        .and_then(|_| file.write_all(b"\n"))
        .and_then(|_| file.sync_all())
        .map_err(|error| format!("write {}: {error}", temp.display()))?;
    drop(file);
    fs::rename(&temp, &path).map_err(|error| format!("rename {}: {error}", temp.display()))?;
    println!(
        "signed {} table(s) -> {} (public_key_sha256={})",
        manifest.tables.len(),
        path.display(),
        sha256_hex(keypair.pk.as_ref())
    );
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("verify") => cmd_verify(&args[1..]),
        Some("sign") => cmd_sign(&args[1..]),
        _ => return usage(),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("offsets-tool: {error}");
            ExitCode::FAILURE
        }
    }
}
