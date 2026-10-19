// 构建脚本：注入 Windows manifest 与难度反馈签名密钥。
//
// 密钥只从环境变量读取，仓库里只保留开发用 seed（`key_id=dev`，生产后端拒绝）。

use std::path::Path;

const DEV_SEED_HEX: &str = "6d6d612d666565646261636b2d6465762d736565642d323032362d3130313721";

fn is_hex_64(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

fn main() {
    println!("cargo:rerun-if-env-changed=MMA_FEEDBACK_SIGNING_SEED");
    println!("cargo:rerun-if-env-changed=MMA_FEEDBACK_KEY_ID");

    // 用 CARGO_MANIFEST_DIR 拼绝对路径：构建脚本不保证 cwd，相对路径在沙箱里会 EACCES。
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let root = Path::new(&manifest_dir);

    let mut attrs = tauri_build::Attributes::new();
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let manifest_path = root.join("app.manifest");
        let manifest = std::fs::read_to_string(&manifest_path).expect("failed to read app.manifest");
        println!("cargo:rerun-if-changed=app.manifest");
        println!("cargo:rerun-if-changed=icons/icon.ico");
        let windows = tauri_build::WindowsAttributes::new()
            .app_manifest(manifest)
            .window_icon_path("icons/icon.ico");
        attrs = attrs.windows_attributes(windows);
    }
    tauri_build::try_build(attrs).expect("failed to run build script");

    let seed = std::env::var("MMA_FEEDBACK_SIGNING_SEED")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| is_hex_64(value))
        .unwrap_or_else(|| {
            if std::env::var("MMA_FEEDBACK_SIGNING_SEED").is_ok() {
                println!("cargo:warning=MMA_FEEDBACK_SIGNING_SEED is not 64 hex chars; using dev seed");
            }
            DEV_SEED_HEX.to_string()
        });
    let key_id = std::env::var("MMA_FEEDBACK_KEY_ID")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| {
            !value.is_empty()
                && value.len() <= 32
                && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        })
        .unwrap_or_else(|| "dev".to_string());

    println!("cargo:rustc-env=MMA_FEEDBACK_SIGNING_SEED={seed}");
    println!("cargo:rustc-env=MMA_FEEDBACK_KEY_ID={key_id}");
}
