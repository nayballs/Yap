/// The desktop's own files Yap Link's page serves as they are: (where the
/// page asks for it, where it is). Chat's picker (`src/lib/core/chatPicker.js`
/// and the pure modules it imports, the effort's words) and the marks it
/// shows. Each must stay pure (no imports beyond this list, no Tauri, no
/// Svelte): `e2e-link/shared.spec.js` holds them to it.
const LINK_SHARED: &[(&str, &str)] = &[
    ("core/chatPicker.js", "../src/lib/core/chatPicker.js"),
    ("core/modelCatalog.js", "../src/lib/core/modelCatalog.js"),
    ("core/latestModels.js", "../src/lib/core/latestModels.js"),
    ("core/ppModels.js", "../src/lib/core/ppModels.js"),
    ("core/localContext.js", "../src/lib/core/localContext.js"),
    ("core/modelIcons.js", "../src/lib/core/modelIcons.js"),
    ("core/providerIds.js", "../src/lib/core/providerIds.js"),
    ("core/effort.js", "../src/lib/core/effort.js"),
    ("providers/openai.svg", "../src/assets/providers/openai.svg"),
    ("providers/openai-oss.svg", "../src/assets/providers/openai-oss.svg"),
    ("providers/claude.svg", "../src/assets/providers/claude.svg"),
    ("providers/gemini.svg", "../src/assets/providers/gemini.svg"),
    ("providers/gemma.svg", "../src/assets/providers/gemma.svg"),
    ("providers/groq.svg", "../src/assets/providers/groq.svg"),
    ("providers/llama.svg", "../src/assets/providers/llama.svg"),
    ("providers/mistral.svg", "../src/assets/providers/mistral.svg"),
    ("providers/openrouter.svg", "../src/assets/providers/openrouter.svg"),
    ("providers/qwen.svg", "../src/assets/providers/qwen.svg"),
    ("providers/xai.svg", "../src/assets/providers/xai.svg"),
];

/// Yap Link's phone page (`src/link/page.rs`): every file under `link-page/`
/// embedded in yap.exe, listed in `$OUT_DIR/link_page.rs` as
/// `(path, bytes)` — the page's own files, whatever they are, with nothing
/// read from disk at run time. Hidden files (`.…`) and the folder's
/// README.md (its notes, not the page) are left out.
fn embed_link_page() {
    use std::path::{Path, PathBuf};
    fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || (dir == root && name.eq_ignore_ascii_case("README.md")) {
                continue;
            }
            if path.is_dir() {
                walk(root, &path, out);
            } else if path.is_file() {
                let rel = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
                out.push((rel, path.canonicalize().unwrap_or(path)));
            }
        }
    }
    let root = Path::new("link-page");
    println!("cargo:rerun-if-changed=link-page");
    let mut files = Vec::new();
    walk(root, root, &mut files);
    // Chat's own model picker code and its logos, shared with the desktop
    // as they are (link-page/README.md, "Chat's own modules"): the pure
    // modules under core/, the provider and family marks under providers/.
    for (served, src) in LINK_SHARED {
        println!("cargo:rerun-if-changed={src}");
        let path = Path::new(src);
        let abs = path.canonicalize().unwrap_or_else(|e| panic!("Yap Link's shared file {src}: {e}"));
        files.push((served.to_string(), abs));
    }
    files.sort();
    let mut code = String::from("/// Yap Link's page: (path under link-page/, its bytes).\npub static FILES: &[(&str, &[u8])] = &[\n");
    for (rel, abs) in &files {
        code.push_str(&format!("    ({rel:?}, include_bytes!({:?})),\n", abs.to_string_lossy()));
    }
    code.push_str("];\n");
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR")).join("link_page.rs");
    std::fs::write(out, code).expect("write link_page.rs");
}

fn main() {
    embed_link_page();
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
