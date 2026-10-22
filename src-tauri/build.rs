fn main() {
    // Yap's Windows manifest (tauri's default: a dependency on Common
    // Controls v6, where the tray, menus and dialogs find TaskDialogIndirect)
    // is embedded by the linker into every binary it makes, the unit-test
    // exe included. Left to tauri-build it went into yap.exe only, and
    // Windows refused to start a test exe that linked the tray or menu code
    // (STATUS_ENTRYPOINT_NOT_FOUND, 22 Oct 2026).
    let windows = tauri_build::WindowsAttributes::new_without_app_manifest();
    tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows))
        .expect("failed to run tauri-build");
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        let manifest = std::path::Path::new("windows-app-manifest.xml");
        println!("cargo:rerun-if-changed={}", manifest.display());
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg=/MANIFESTINPUT:{}",
            manifest.canonicalize().expect("windows-app-manifest.xml").display()
        );
    }
}
