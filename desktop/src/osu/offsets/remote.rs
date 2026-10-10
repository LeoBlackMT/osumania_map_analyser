use std::path::PathBuf;
use sha2::{Digest, Sha256};
use ed25519_compact::{PublicKey, Signature};

/// ManiaMapAnalyser 远端表发布主公钥（Ed25519 32 字节）
pub const ED25519_MASTER_PUBLIC_KEY: [u8; 32] = [
    206, 229, 44, 102, 228, 179, 117, 67, 18, 115, 220, 52, 238, 212, 17, 70,
    95, 211, 253, 148, 249, 231, 59, 153, 109, 26, 243, 174, 121, 135, 167, 119,
];

/// 远端表清单中的单表条目
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct RemoteManifestTable {
    pub filename: String,
    pub client: String,
    pub game_version: String,
    pub runtime_version: String,
    pub arch: String,
    pub sha256: String,
    pub signature: String,
}

/// 远端表清单（manifest.json）
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct RemoteManifest {
    pub schema_version: u32,
    pub updated_at: String,
    pub tables: Vec<RemoteManifestTable>,
}

/// 验签函数：同时核验 SHA-256 与 Ed25519 签名
pub fn verify_table_signature(
    content: &[u8],
    expected_sha256: &str,
    signature_str: &str,
    public_key_bytes: &[u8; 32],
) -> Result<(), &'static str> {
    // 1. SHA-256 核验
    let mut hasher = Sha256::new();
    hasher.update(content);
    let hash_hex = format!("{:x}", hasher.finalize());
    if !hash_hex.eq_ignore_ascii_case(expected_sha256.trim()) {
        return Err("sha256_mismatch");
    }

    // 2. 解码签名（支持十六进制 128 字符或标准 Base64）
    let sig_bytes = decode_signature_str(signature_str).ok_or("signature_format_error")?;
    let signature = Signature::from_slice(&sig_bytes).map_err(|_| "signature_length_error")?;

    // 3. Ed25519 公钥验签
    let pk = PublicKey::from_slice(public_key_bytes).map_err(|_| "invalid_public_key")?;
    pk.verify(content, &signature).map_err(|_| "signature_verification_failed")?;

    Ok(())
}

pub fn decode_signature_str(s: &str) -> Option<Vec<u8>> {
    let s = s.trim();
    if s.len() == 128 && s.chars().all(|c| c.is_ascii_hexdigit()) {
        return decode_hex(s);
    }
    decode_base64(s)
}

fn decode_hex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    for chunk in s.as_bytes().chunks_exact(2) {
        let h1 = char::from(chunk[0]).to_digit(16)?;
        let h2 = char::from(chunk[1]).to_digit(16)?;
        out.push(((h1 << 4) | h2) as u8);
    }
    Some(out)
}

fn decode_base64(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut buf = 0u32;
    let mut bits = 0u32;
    for &b in s.as_bytes() {
        let val = match b {
            b'A'..=b'Z' => (b - b'A') as u32,
            b'a'..=b'z' => (b - b'a' + 26) as u32,
            b'0'..=b'9' => (b - b'0' + 52) as u32,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' | b' ' | b'\r' | b'\n' => continue,
            _ => return None,
        };
        buf = (buf << 6) | val;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// 将已验签并通过 schema 校验的表原子落盘至本地缓存目录
pub fn save_remote_table(
    client: &str,
    filename: &str,
    content: &[u8],
) -> Result<PathBuf, std::io::Error> {
    let base_dir = if let Ok(appdata) = std::env::var("APPDATA") {
        PathBuf::from(appdata)
    } else if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        PathBuf::from(xdg)
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".local").join("share")
    } else {
        PathBuf::from(".")
    };
    let target_dir = base_dir
        .join("ManiaMapAnalyser")
        .join("offsets")
        .join(client);
    std::fs::create_dir_all(&target_dir)?;

    let target_file = target_dir.join(filename);
    let temp_file = target_dir.join(format!("{filename}.tmp-{}", std::process::id()));
    std::fs::write(&temp_file, content)?;
    std::fs::rename(&temp_file, &target_file)?;

    Ok(target_file)
}
