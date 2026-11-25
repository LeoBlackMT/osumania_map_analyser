// lazer & stable 偏移表模块门面
//
// 纯数据 + 加载 + 结构校验，由子模块分别承担：
// - lookup: 字段与类型查找、canonical_type
// - lazer_table: Lazer 偏移表结构与就近回落
// - stable_table: Stable 锚点与拓扑定义及本地查找
// - validation: 纯数据 Schema 安全与合理性校验器
// - remote: 远端表发布签名验证与清单解析

pub mod lookup;
pub mod lazer_table;
pub mod stable_table;
pub mod validation;
pub mod remote;

pub use lookup::*;
pub use lazer_table::*;
pub use stable_table::*;
pub use validation::*;
pub use remote::*;

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::path::PathBuf;

    #[test]
    fn default_tables_are_valid() {
        let stable = default_stable_table();
        assert!(validate_stable_schema(&stable).is_ok());

        let ppysb = default_ppysb_table();
        assert_eq!(ppysb.client, "ppysb");
        assert_eq!(ppysb.arch, "x86");
        assert!(validate_stable_schema(&ppysb).is_ok());

        let lazer = default_lazer_table();
        assert!(validate_lazer_schema(&lazer).is_ok());
        let mods = lazer.mods().expect("default table carries the mods section");
        assert_eq!(
            mods.settings_for("DA")
                .and_then(|fields| fields.get("overall_difficulty")),
            Some(&0x28)
        );
        assert_eq!(
            mods.settings_for("DT")
                .and_then(|fields| fields.get("speed_change")),
            Some(&0x10)
        );
    }

    #[test]
    fn on_disk_stable_table_loads_and_validates() {
        let path = std::path::Path::new("offsets/stable/stable__x86.json");
        if path.exists() {
            let bytes = std::fs::read(path).expect("read stable__x86.json");
            let table = StableTable::load(&bytes).expect("load stable__x86.json");
            assert_eq!(table.client, "stable");
            assert_eq!(table.arch, "x86");
            assert!(table.anchors.contains_key("statusPtr"));
            assert!(table.anchors.contains_key("baseAddr"));
            assert_eq!(table.state_name(2), Some("play"));
            assert_eq!(table.state_name(5), Some("selectPlay"));
            assert_eq!(table.state_name(7), Some("resultScreen"));
        }
    }

    #[test]
    fn stable_schema_rejects_script_injection() {
        let mut table = default_stable_table();
        table.client = "<script>alert(1)</script>".to_string();
        assert!(matches!(
            validate_stable_schema(&table),
            Err(ValidationError::ExecutableContent(_))
        ));

        let mut table2 = default_stable_table();
        table2.evidence = "normal text with javascript:evil() inside".to_string();
        assert!(matches!(
            validate_stable_schema(&table2),
            Err(ValidationError::ExecutableContent(_))
        ));

        let mut table3 = default_stable_table();
        table3.version = "eval(foo)".to_string();
        assert!(matches!(
            validate_stable_schema(&table3),
            Err(ValidationError::ExecutableContent(_))
        ));
    }

    #[test]
    fn stable_schema_rejects_out_of_range_displacement() {
        let mut table = default_stable_table();
        table.topology.beatmap_from_base = 2_000_000;
        assert!(matches!(
            validate_stable_schema(&table),
            Err(ValidationError::DisplacementOutOfRange(..))
        ));

        let mut table2 = default_stable_table();
        table2.topology.hits_candidate_offsets[0] = 5_000_000;
        assert!(matches!(
            validate_stable_schema(&table2),
            Err(ValidationError::DisplacementOutOfRange(..))
        ));
    }

    #[test]
    fn stable_schema_rejects_unaligned_hits_offset() {
        let mut table = default_stable_table();
        table.topology.hits_candidate_offsets[0] = 0x89; // odd offset
        assert!(matches!(
            validate_stable_schema(&table),
            Err(ValidationError::InvalidAlignment(..))
        ));
    }

    #[test]
    fn stable_schema_rejects_hop_depth_exceeded() {
        let mut table = default_stable_table();
        for i in 0..20 {
            table.anchors.insert(
                format!("extra_anchor_{i}"),
                StableAnchorDef {
                    pattern: "90 90".to_string(),
                    offset: 0,
                    derivation: String::new(),
                    evidence: String::new(),
                },
            );
        }
        assert!(matches!(
            validate_stable_schema(&table),
            Err(ValidationError::HopDepthExceeded(_))
        ));
    }

    #[test]
    fn stable_schema_rejects_unknown_state_names() {
        let mut table = default_stable_table();
        table
            .mappings
            .states
            .insert("99".to_string(), "malicious_state".to_string());
        assert!(matches!(
            validate_stable_schema(&table),
            Err(ValidationError::UnknownStateName(_))
        ));
    }

    #[test]
    fn lazer_schema_rejects_out_of_range_displacement() {
        let mut table = default_lazer_table();
        let mut fields = std::collections::BTreeMap::new();
        fields.insert("evil_field".to_string(), 100_000_000);
        table.types.insert("Some.Type".to_string(), fields);
        assert!(matches!(
            validate_lazer_schema(&table),
            Err(ValidationError::DisplacementOutOfRange(..))
        ));
    }

    #[test]
    fn find_stable_table_returns_valid_table() {
        let table = find_stable_table(None).expect("must find table or compile-time fallback");
        assert_eq!(table.client, "stable");
        assert!(validate_stable_schema(&table).is_ok());

        let ppysb = find_stable_table_variant(None, "ppysb").expect("must find ppysb table");
        assert_eq!(ppysb.client, "ppysb");
        assert_eq!(ppysb.arch, "x86");
        assert!(validate_stable_schema(&ppysb).is_ok());
    }

    #[test]
    fn ed25519_signature_verification_and_tamper_detection() {
        use ed25519_compact::KeyPair;
        let keypair = KeyPair::generate();
        let content = br#"{"lazer_version":"2026.1005.0.0","arch":"x64"}"#;

        let mut hasher = Sha256::new();
        hasher.update(content);
        let sha256_hex = format!("{:x}", hasher.finalize());

        let sig = keypair.sk.sign(content, None);
        let sig_hex: String = sig.as_ref().iter().map(|b| format!("{b:02x}")).collect();

        // 1. 正确签名验签通过
        assert!(verify_table_signature(content, &sha256_hex, &sig_hex, keypair.pk.as_slice().try_into().unwrap()).is_ok());

        // 2. 篡改内容 -> SHA-256 不符直接拒绝
        let tampered_content = br#"{"lazer_version":"2026.1005.0.0","arch":"x86"}"#;
        assert_eq!(
            verify_table_signature(tampered_content, &sha256_hex, &sig_hex, keypair.pk.as_slice().try_into().unwrap()),
            Err("sha256_mismatch")
        );

        // 3. 篡改签名 -> 签名验证失败
        let mut bad_sig_hex = sig_hex.clone();
        bad_sig_hex.replace_range(0..2, "00");
        assert_eq!(
            verify_table_signature(content, &sha256_hex, &bad_sig_hex, keypair.pk.as_slice().try_into().unwrap()),
            Err("signature_verification_failed")
        );

        // 4. 伪造公钥 -> 签名验证失败
        let other_keypair = KeyPair::generate();
        assert_eq!(
            verify_table_signature(content, &sha256_hex, &sig_hex, other_keypair.pk.as_slice().try_into().unwrap()),
            Err("signature_verification_failed")
        );
    }

    #[test]
    fn remote_manifest_roundtrip() {
        let manifest_json = r#"{
            "schema_version": 2,
            "updated_at": "2026-10-08T12:00:00Z",
            "tables": [
                {
                    "filename": "2026.1005.0.0__10.0.12__x64.json",
                    "client": "lazer",
                    "game_version": "2026.1005.0.0",
                    "runtime_version": "10.0.12",
                    "arch": "x64",
                    "sha256": "abcdef123456",
                    "signature": "sig123"
                }
            ]
        }"#;

        let manifest: RemoteManifest = serde_json::from_str(manifest_json).expect("parse manifest");
        assert_eq!(manifest.schema_version, 2);
        assert_eq!(manifest.tables.len(), 1);
        assert_eq!(manifest.tables[0].client, "lazer");
        assert_eq!(manifest.tables[0].filename, "2026.1005.0.0__10.0.12__x64.json");
    }

    #[test]
    fn save_remote_table_writes_atomically() {
        let content = b"test table data";
        let path = save_remote_table("lazer", "test_table.json", content).expect("save remote table");
        assert!(path.exists());
        let read_back = std::fs::read(&path).expect("read back");
        assert_eq!(read_back, content);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn bundled_manifest_references_packaged_tables() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("offsets");
        let manifest: RemoteManifest =
            serde_json::from_slice(&std::fs::read(root.join("manifest.json")).unwrap()).unwrap();
        assert_eq!(manifest.schema_version, 2);
        assert!(!manifest.tables.is_empty());
        for table in manifest.tables {
            let path = root.join(&table.client).join(&table.filename);
            assert!(path.is_file(), "missing packaged table: {}", path.display());
        }
    }
}
