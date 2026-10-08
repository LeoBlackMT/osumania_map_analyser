fn main() {
    let mut attrs = tauri_build::Attributes::new();
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let manifest = std::fs::read_to_string("app.manifest").expect("failed to read app.manifest");
        println!("cargo:rerun-if-changed=app.manifest");
        println!("cargo:rerun-if-changed=icons/icon.ico");
        let windows = tauri_build::WindowsAttributes::new()
            .app_manifest(manifest)
            .window_icon_path("icons/icon.ico");
        attrs = attrs.windows_attributes(windows);
    }
    tauri_build::try_build(attrs).expect("failed to run build script");
}