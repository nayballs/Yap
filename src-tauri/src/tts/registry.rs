//! Kokoro's download: where each file comes from (pinned by size and
//! SHA-256), and how it's laid out in `<data>/models/tts/`:
//!
//! ```text
//! models/tts/
//!   kokoro-v1.0/        model.onnx (fp32), voices/<id>.bin (54),
//!                       lexicon/{us,gb}_{gold,silver}.json + LICENSE, NOTICE.txt
//!   espeak-ng-1.52.0/   espeak-ng.exe, libespeak-ng.dll, espeak-ng-data/
//!                       (the languages Kokoro reads), COPYING (GPL-3.0), NOTICE.txt
//! ```
//!
//! Sources, and nothing else: **Hugging Face onnx-community/Kokoro-82M-
//! v1.0-ONNX** at commit 1939ad2a (the research's; Apache-2.0 — the model
//! and the voices), **hexgrad/misaki** at fba12365 (Apache-2.0 — the
//! pronunciation dictionaries Kokoro was trained with), and **eSpeak NG
//! 1.52.0**'s own release (GPL-3.0; its Windows installer, unpacked with
//! Windows' `msiexec /a` — an administrative install only unpacks files,
//! nothing is registered — and run later as a separate program).
//!
//! **The fp32 model, not the fp16 one** (half the size): the fp16 export
//! returns nothing but NaN for longer sentences in the British voices and
//! bm_george (2 of the research answer's 6 sentences with bf_emma, 1 with
//! bm_george; at every optimisation level), the fp32 one never does — found
//! by `kokoro::real::checks::nan_probe`.
//!
//! Install = download each file (resuming a partial one; a file already
//! installed at its size and SHA-256 is kept), check its size and SHA-256,
//! put it in `<folder>.installing/`, then rename that into place: a folder
//! is either whole or not there. Same helpers as the speaker models'
//! (`diarize/registry.rs`).

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::diarize::registry as dl;

pub const MODEL_FILE: &str = "model.onnx";
pub const KOKORO_FOLDER: &str = "kokoro-v1.0";
pub const ESPEAK_FOLDER: &str = "espeak-ng-1.52.0";

/// How a downloaded file goes into its folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dest {
    /// Saved at this path in the folder.
    File(&'static str),
    /// eSpeak NG's Windows installer: unpacked, its program and the data
    /// Kokoro needs kept.
    EspeakMsi,
}

#[derive(Debug, Clone, Copy)]
pub struct Artifact {
    pub url: &'static str,
    pub sha256: &'static str,
    pub size: u64,
    pub dest: Dest,
}

pub struct Package {
    pub id: &'static str,
    pub folder: &'static str,
    pub artifacts: &'static [Artifact],
    /// Must be in the folder for it to count as installed (besides every
    /// `Dest::File` at its pinned size).
    pub required: &'static [&'static str],
    pub notice: &'static str,
}

#[cfg(test)]
const HF_COMMIT: &str = "1939ad2a8e416c0acfeecc08a694d14ef25f2231";
#[cfg(test)]
const MISAKI_COMMIT: &str = "fba1236595f2d2bf21d414ba6e57d25256afada3";

macro_rules! hf {
    ($path:literal) => {
        concat!("https://huggingface.co/onnx-community/Kokoro-82M-v1.0-ONNX/resolve/1939ad2a8e416c0acfeecc08a694d14ef25f2231/", $path)
    };
}
macro_rules! misaki {
    ($path:literal) => {
        concat!("https://raw.githubusercontent.com/hexgrad/misaki/fba1236595f2d2bf21d414ba6e57d25256afada3/", $path)
    };
}
macro_rules! voice {
    ($id:literal, $sha:literal) => {
        Artifact {
            url: concat!("https://huggingface.co/onnx-community/Kokoro-82M-v1.0-ONNX/resolve/1939ad2a8e416c0acfeecc08a694d14ef25f2231/voices/", $id, ".bin"),
            sha256: $sha,
            size: 522_240,
            dest: Dest::File(concat!("voices/", $id, ".bin")),
        }
    };
}

pub static KOKORO: Package = Package {
    id: "kokoro-v1.0",
    folder: KOKORO_FOLDER,
    artifacts: &[
        Artifact {
            url: hf!("onnx/model.onnx"),
            sha256: "8fbea51ea711f2af382e88c833d9e288c6dc82ce5e98421ea61c058ce21a34cb",
            size: 325_532_232,
            dest: Dest::File(MODEL_FILE),
        },
        Artifact {
            url: misaki!("misaki/data/us_gold.json"),
            sha256: "dc414872a49a28ae6c141463d502fd945f3b2fde040484fdc47d00cc4612686f",
            size: 3_000_469,
            dest: Dest::File("lexicon/us_gold.json"),
        },
        Artifact {
            url: misaki!("misaki/data/us_silver.json"),
            sha256: "de8f67be911bb6c659187b4a65fd966b6a30e56350e0f790d763210b053ac475",
            size: 3_099_517,
            dest: Dest::File("lexicon/us_silver.json"),
        },
        Artifact {
            url: misaki!("misaki/data/gb_gold.json"),
            sha256: "29e62f4b60261c88f7f3c2c7811ca3825978948090b72d2b27d565b729282f71",
            size: 2_838_552,
            dest: Dest::File("lexicon/gb_gold.json"),
        },
        Artifact {
            url: misaki!("misaki/data/gb_silver.json"),
            sha256: "48131e2d92ccc41655f4543e87e0f938e71463eb5a54be7f0693bb712ebb6bce",
            size: 3_663_898,
            dest: Dest::File("lexicon/gb_silver.json"),
        },
        Artifact {
            url: misaki!("LICENSE"),
            sha256: "c71d239df91726fc519c6eb72d318ec65820627232b2f796219e87dcf35d0ab4",
            size: 11_357,
            dest: Dest::File("lexicon/LICENSE"),
        },
        voice!("af_alloy", "c4a6b876047fd7fb472edf4ebd63cfac7c3b958a7cae7c106e8f038ca6308c45"),
        voice!("af_aoede", "4a004c33430762e2461eedb2013fad808ef4ab3121f5300f554476caf58d8361"),
        voice!("af_bella", "f69d836209b78eb8c66e75e3cda491e26ea838a3674257e9d4e5703cbaf55c8b"),
        voice!("af_heart", "d583ccff3cdca2f7fae535cb998ac07e9fcb90f09737b9a41fa2734ec44a8f0b"),
        voice!("af_jessica", "a240a5e3c15b43563d6e923bdca8ef5613a23471d9b77653694012435df23bd8"),
        voice!("af_kore", "9be5221b6a941c04b561959b8ff0b06e809444dcc4ab7e75a7b23606f691819e"),
        voice!("af_nicole", "cd2191ab31b914ed7b318416b0e4440fdf392ddad9106a060819aa600a64f59a"),
        voice!("af_nova", "18778272caa0d0eebaea251c35fd635f038434f9eee5e691d02a174bd328414f"),
        voice!("af_river", "00a2bcf82b1d86e8f19902ede58c65ccf6c0e43b44b7d74fad54e5d8933c9c30"),
        voice!("af_sarah", "4409fbc125afabacc615d94db5398d847006a737b0247d6892b7a9a0007a2f0a"),
        voice!("af_sky", "4435255c9744f3f31659e0d714ab7689bf65d9e77ec1cce060f083912614f0b9"),
        voice!("am_adam", "162b035ed91cfc48b6046982184c645f72edcdd1b82843347f605d7bf7b15716"),
        voice!("am_echo", "3968b92c3c4cd1c4416dbded36c13eaa388a90d5788d02a13e4d781f5f8cf3c3"),
        voice!("am_eric", "e8b5be17edd1e3636901ce7598baafe2dc8dd8ff707a0c23bf9e461add7e2832"),
        voice!("am_fenrir", "c27989f741f7ee34d273a39d8a595cc0837d35f5ced9a29b7cc162614616df43"),
        voice!("am_liam", "52403be32fd047c6a44517cb0bcd6b134f2a18baa73e70ef41651e0eab921ade"),
        voice!("am_michael", "1d1f21dd8da39c30705cd4c75d039d265e9bc4a2a93ed09bc9e1b1225eb95ba1"),
        voice!("am_onyx", "da5d135b424164916d75a68ffb4c2abce3d7d5ccc82dd1ee6cf447ce286145e6"),
        voice!("am_puck", "fcf73c989033e9233e0b98713eca600c8c74dcc1614b37009d5450ff4a2274a0"),
        voice!("am_santa", "61150cf726ab6c5ed7a99f90a304f91f5a72c00c592e89ec94e5df11c319227a"),
        voice!("bf_alice", "08afa6ba24da61ea5e8efa139e5aadc938d83f0a6da5a900adaf763ac1da5573"),
        voice!("bf_emma", "669fe0647f9dd04fcab92f1439a40eeb4c8b4ab1f82e4996fe3d918ce4a63b73"),
        voice!("bf_isabella", "3754352c4aaa46d17f27654ab7518d65b62ad6163a0f55a5f4330c2da2c4e94f"),
        voice!("bf_lily", "5e0ee32ebe64a467124976b14e69590746f1c4ce41a12b587a50c862edfea335"),
        voice!("bm_daniel", "6b3194bbceffb746733cbc22c8f593dd44e401a71d53895a2dca891bc595a1e8"),
        voice!("bm_fable", "f889083196807b4adb15e9204252165f503b8d33d3982e681c52443c49d798f1"),
        voice!("bm_george", "c4b235a4c1f2cd3b939fed08b899ce9385638b763f7b73a59616c4fc9bd6c9bc"),
        voice!("bm_lewis", "b8f671cef828c30e66fdf0b0756a76bba58f6bb3398cbbf27058642acbcedb97"),
        voice!("ef_dora", "f66ec66bd295acb18372e37008533a9a3228483ccd294e7538d5d9294ac9a532"),
        voice!("em_alex", "27809e9eafdcbcfff90a3016c697568676531de2a2c39cee29c96c7bd6b83e95"),
        voice!("em_santa", "ad43b774e1ca24d05c6161297d8aeb770ac3d29bb95daf516727af5f7d543683"),
        voice!("ff_siwis", "a35f5675ad08948e326ae75fd0ea16ba5d0042e4f76b5f3d1df77d0a48c54861"),
        voice!("hf_alpha", "040be6a4425411cc01fda5fd06693c76bfa78572632852bc8cda9c99232ffb56"),
        voice!("hf_beta", "cd83ae0bb9b2e4e4fb92b4973bd8d1822ca0036d3c498bf4fc89aa8e33917cc7"),
        voice!("hm_omega", "b02d9222d9ed00ce26b302173a862c2c93f96cc40b5c422b8d14910b9ff34137"),
        voice!("hm_psi", "644daf88ba8aeb7bd08950bbdcd4453bb280864e49dc4df93fabc6be32e03f37"),
        voice!("if_sara", "409b69248798fcdc2542330c76953d230710f19b057e59cb82fdc3c4cf71265c"),
        voice!("im_nicola", "bc578e510d52a96d6940d46f12e96d7b3df00905dbea075113226d100e6e1ab0"),
        voice!("jf_alpha", "56b479360aad9f367aeb8cef908f9201cf48b4555e488c5f4590c9dfcd978bb6"),
        voice!("jf_gongitsune", "0f1181f3772d27b7c12aaf4bcd71e31b186c4146e330d074a3dc64ee392af396"),
        voice!("jf_nezumi", "13cb71eebb0b48739d444558322aa35a8c9a489b80e1e631f14d2e6aea93026b"),
        voice!("jf_tebukuro", "29c6c0561b4288d59639677bebe7533c919743d5ea68d0d2ae992644beea6696"),
        voice!("jm_kumo", "09e959d239724c734d65661f06f14cdabcddfd476bfaaad905a937099ae9e64f"),
        voice!("pf_dora", "3da7b5b2d91847ebf5646f57631af6ececae3c29a89cd300f06edf9aa6cfe9ee"),
        voice!("pm_alex", "0175c753f59c54e7fd5a995bedef0c5ff2fb67e0043dd3dcb2ae74ec2acbeb2a"),
        voice!("pm_santa", "8b012db3185778afe2e45a62cbad69db73021774fe68dda634bcc748a982eede"),
        voice!("zf_xiaobei", "5dde6e1c9c4f12c8b327bc29c0cee361a23b52b952c04636858ba637ec66e640"),
        voice!("zf_xiaoni", "08892b62a39af0a615cd0581238db7e19e44c578e8fa0bfd0e586e93327d9cba"),
        voice!("zf_xiaoxiao", "03adb5d5e3ddd88b047954e974e651cb0a4b524c985057e5d872e962c7be1169"),
        voice!("zf_xiaoyi", "bc1555c5c486099196ac254bae5e0bb543c121952a3092f50b7d8724f1bc36b3"),
        voice!("zm_yunjian", "de48a00bdbf3649f07162269a2b6e0513604389bfac8a2e6c75cb34b323ad6fa"),
        voice!("zm_yunxi", "7243892fb4e560d47014090ddf010f8b8b790f3c6b029ff82b2ac06aa4e27c8b"),
        voice!("zm_yunxia", "6b2b8fc15b3df19a368daebe5c581c7fabf433ee5b8a17ffd6b3d723cff8936d"),
        voice!("zm_yunyang", "261e2c89470534dbbcb8fd98b8fdc495ec94063d9bb6c8277f7be43cccba3f42"),
    ],
    required: &[MODEL_FILE, "voices/af_bella.bin", "lexicon/us_gold.json", "lexicon/gb_gold.json"],
    notice: "Kokoro-82M v1.0 (fp32 ONNX export and its 54 voices)\n\
             Copyright (c) hexgrad. Licensed under the Apache License, Version 2.0:\n\
             https://www.apache.org/licenses/LICENSE-2.0\n\
             Model: https://huggingface.co/hexgrad/Kokoro-82M\n\
             Downloaded from https://huggingface.co/onnx-community/Kokoro-82M-v1.0-ONNX\n\
             at commit 1939ad2a8e416c0acfeecc08a694d14ef25f2231.\n\
             \n\
             lexicon/: misaki's English pronunciation dictionaries (US and GB),\n\
             Copyright (c) hexgrad, Apache License 2.0 (lexicon/LICENSE), from\n\
             https://github.com/hexgrad/misaki at commit fba1236595f2d2bf21d414ba6e57d25256afada3.\n",
};

pub static ESPEAK: Package = Package {
    id: "espeak-ng-1.52.0",
    folder: ESPEAK_FOLDER,
    artifacts: &[
        Artifact {
            url: "https://github.com/espeak-ng/espeak-ng/releases/download/1.52.0/espeak-ng.msi",
            sha256: "7f673c709ea5dd579d3b5ebb98688cc575328a6ab7438d2bc405b88cedaeafb9",
            size: 12_765_862,
            dest: Dest::EspeakMsi,
        },
        Artifact {
            url: "https://raw.githubusercontent.com/espeak-ng/espeak-ng/1.52.0/COPYING",
            sha256: "8ceb4b9ee5adedde47b31e975c1d90c73ad27b6b165a1dcd80c7c545eb65b903",
            size: 35_147,
            dest: Dest::File("COPYING"),
        },
    ],
    required: &["espeak-ng.exe", "libespeak-ng.dll", "espeak-ng-data/phontab", "espeak-ng-data/en_dict", "COPYING"],
    notice: "eSpeak NG 1.52.0 (Windows x64), a separate program Yap runs to sound\n\
             out words its pronunciation dictionary doesn't have.\n\
             Copyright (C) 2005-2024 Reece H. Dunn, Jonathan Duddington and the\n\
             eSpeak NG contributors.\n\
             Licensed under the GNU General Public License, version 3 or later\n\
             (COPYING). It is not linked into Yap.\n\
             Downloaded from https://github.com/espeak-ng/espeak-ng/releases/tag/1.52.0\n\
             Source code: https://github.com/espeak-ng/espeak-ng/tree/1.52.0\n\
             (https://github.com/espeak-ng/espeak-ng/archive/refs/tags/1.52.0.tar.gz)\n",
};

/// eSpeak NG's data Kokoro needs: the phoneme tables, the voices and
/// languages, and the dictionaries of the languages its voices read
/// (English, Spanish, French, Hindi, Italian, Portuguese).
pub fn keep_espeak_data(rel: &str) -> bool {
    let rel = rel.replace('\\', "/");
    let Some(rest) = rel.strip_prefix("espeak-ng-data/") else {
        return false;
    };
    matches!(
        rest,
        "phontab" | "phonindex" | "phondata" | "phondata-manifest" | "intonations" | "en_dict" | "es_dict"
            | "fr_dict" | "hi_dict" | "it_dict" | "pt_dict"
    ) || rest.starts_with("lang/")
        || rest.starts_with("voices/")
}

/// `<data>/models/tts`, or in a debug build `YAP_TTS_MODELS` (the e2e suite
/// and the real-engine checks).
pub fn root() -> PathBuf {
    #[cfg(any(debug_assertions, test))]
    if let Some(dir) = std::env::var_os("YAP_TTS_MODELS").filter(|v| !v.is_empty()) {
        return PathBuf::from(dir);
    }
    crate::config::data_dir().join("models").join("tts")
}

pub fn kokoro_dir(root: &Path) -> PathBuf {
    root.join(KOKORO_FOLDER)
}
pub fn espeak_dir(root: &Path) -> PathBuf {
    root.join(ESPEAK_FOLDER)
}

/// A file as it will be fetched (a debug build's stand-ins applied).
#[derive(Debug, Clone, PartialEq)]
pub struct Source {
    pub url: String,
    pub sha256: String,
    pub size: u64,
    pub dest: Dest,
}

// Only the debug-build stand-ins and the tests name a destination.
#[cfg(any(debug_assertions, test))]
fn dest_name(d: Dest) -> &'static str {
    match d {
        Dest::File(p) => p,
        Dest::EspeakMsi => "espeak-ng.msi",
    }
}

/// The files of `pkg`. A debug build reads stand-ins from the JSON file
/// `YAP_TTS_SOURCES` (`{"<package id>": {"<dest>": {"url", "sha256",
/// "size"}}}`, dest as in the folder, the installer as "espeak-ng.msi"),
/// how the e2e suite downloads small fakes from a local server.
pub fn sources(pkg: &Package) -> Vec<Source> {
    #[cfg_attr(not(debug_assertions), allow(unused_mut))]
    let mut out: Vec<Source> = pkg
        .artifacts
        .iter()
        .map(|a| Source { url: a.url.to_string(), sha256: a.sha256.to_string(), size: a.size, dest: a.dest })
        .collect();
    #[cfg(debug_assertions)]
    if let Some(path) = std::env::var_os("YAP_TTS_SOURCES").filter(|v| !v.is_empty()) {
        let stand_ins = std::fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok());
        if let Some(map) = stand_ins.as_ref().and_then(|v| v.get(pkg.id)) {
            for src in out.iter_mut() {
                let s = &map[dest_name(src.dest)];
                if let (Some(url), Some(sha), Some(size)) = (s["url"].as_str(), s["sha256"].as_str(), s["size"].as_u64()) {
                    *src = Source { url: url.to_string(), sha256: sha.to_ascii_lowercase(), size, dest: src.dest };
                }
            }
        }
    }
    out
}

/// Whether `pkg` is installed under `root`: its required files, and each
/// file saved as-is at its pinned size.
pub fn installed(root: &Path, pkg: &Package) -> bool {
    let dir = root.join(pkg.folder);
    pkg.required.iter().all(|f| dir.join(f).is_file())
        && sources(pkg).iter().all(|s| match s.dest {
            Dest::File(name) => std::fs::metadata(dir.join(name)).is_ok_and(|m| m.len() == s.size),
            Dest::EspeakMsi => true,
        })
}

pub fn kokoro_installed(root: &Path) -> bool {
    installed(root, &KOKORO)
}
pub fn espeak_installed(root: &Path) -> bool {
    installed(root, &ESPEAK)
}

fn bytes(pkg: &Package) -> u64 {
    sources(pkg).iter().map(|s| s.size).sum()
}

/// Kokoro as Settings shows it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// This build can run it (an `engines` build, or test mode).
    pub in_build: bool,
    pub installed: bool,
    /// eSpeak NG (for words the dictionary lacks) is there too.
    pub espeak: bool,
    /// What a download fetches now.
    pub download_bytes: u64,
    /// Everything, on disk.
    pub size_bytes: u64,
}

pub fn status(root: &Path) -> Status {
    let (k, e) = (kokoro_installed(root), espeak_installed(root));
    Status {
        in_build: super::kokoro::in_build(),
        installed: k,
        espeak: e,
        download_bytes: if k { 0 } else { bytes(&KOKORO) } + if e { 0 } else { bytes(&ESPEAK) },
        size_bytes: bytes(&KOKORO) + bytes(&ESPEAK),
    }
}

/// Download progress, for `tts-download-progress`.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub percent: u8,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
}

/// A test run fetches only from this PC (the e2e suite's local server).
fn allowed(url: &str) -> bool {
    !crate::e2e::active() || ["http://127.0.0.1:", "http://localhost:", "http://[::1]:"].iter().any(|p| url.starts_with(p))
}

/// Install Kokoro and eSpeak NG (whichever is missing) under `root`.
pub async fn install(root: &Path, on_progress: &(dyn Fn(Progress) + Send + Sync)) -> Result<(), String> {
    let todo: Vec<&Package> = [&KOKORO, &ESPEAK].into_iter().filter(|p| !installed(root, p)).collect();
    let total: u64 = todo.iter().map(|p| bytes(p)).sum();
    std::fs::create_dir_all(root).map_err(|e| format!("create {}: {e}", root.display()))?;
    let mut done: u64 = 0;
    for pkg in todo {
        tracing::info!(package = pkg.id, "Kokoro: installing");
        let staging = root.join(format!("{}.installing", pkg.folder));
        let _ = std::fs::remove_dir_all(&staging);
        std::fs::create_dir_all(&staging).map_err(|e| format!("create {}: {e}", staging.display()))?;
        // A failure leaves no half-made folder (partial downloads stay, to resume).
        if let Err(e) = install_package(root, pkg, &staging, total, &mut done, on_progress).await {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(e);
        }
    }
    on_progress(Progress { percent: 100, downloaded_bytes: total, total_bytes: total });
    Ok(())
}

async fn install_package(
    root: &Path,
    pkg: &Package,
    staging: &Path,
    total: u64,
    done: &mut u64,
    on_progress: &(dyn Fn(Progress) + Send + Sync),
) -> Result<(), String> {
    {
        for (i, src) in sources(pkg).into_iter().enumerate() {
            if !allowed(&src.url) {
                return Err("Test runs never download from the internet".to_string());
            }
            let partial = root.join(format!("{}.{i}.partial", pkg.folder));
            let base = *done;
            let report = |got: u64| {
                let so_far = base + got;
                let percent = (so_far * 100).checked_div(total).map_or(100, |p| p.min(100) as u8);
                on_progress(Progress { percent, downloaded_bytes: so_far, total_bytes: total });
            };
            let as_dl = dl::Source { url: src.url.clone(), sha256: src.sha256.clone(), size: src.size, unpack: dl::Unpack::NotYet };
            // Already there from an earlier version of the package, and right:
            // kept (a new model file doesn't fetch the voices again).
            if let Dest::File(name) = src.dest {
                let have = root.join(pkg.folder).join(name);
                let (have2, sha, size) = (have.clone(), src.sha256.clone(), src.size);
                let same = tokio::task::spawn_blocking(move || {
                    std::fs::metadata(&have2).is_ok_and(|m| m.len() == size) && dl::sha256(&have2).is_ok_and(|s| s == sha)
                })
                .await
                .unwrap_or(false);
                if same {
                    let to = staging.join(name);
                    if let Some(parent) = to.parent() {
                        std::fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
                    }
                    // A second link (else a copy): the installed folder stays
                    // whole until the new one replaces it.
                    std::fs::hard_link(&have, &to)
                        .or_else(|_| std::fs::copy(&have, &to).map(|_| ()))
                        .map_err(|e| format!("keep {name}: {e}"))?;
                    *done += src.size;
                    report(0);
                    continue;
                }
            }
            dl::download(&as_dl, &partial, &report, crate::privacy::net::Download::Voice).await?;
            dl::verify(&partial, &as_dl).await?;
            match src.dest {
                Dest::File(name) => {
                    let to = staging.join(name);
                    if let Some(parent) = to.parent() {
                        std::fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
                    }
                    std::fs::rename(&partial, &to).map_err(|e| format!("move {name}: {e}"))?;
                }
                Dest::EspeakMsi => {
                    let (msi, dest) = (partial.clone(), staging.to_path_buf());
                    let n = tokio::task::spawn_blocking(move || unpack_espeak(&msi, &dest))
                        .await
                        .map_err(|e| format!("unpack task: {e}"))??;
                    tracing::info!(files = n, "Kokoro: eSpeak NG unpacked");
                    let _ = std::fs::remove_file(&partial);
                }
            }
            *done += src.size;
        }
        std::fs::write(staging.join("NOTICE.txt"), pkg.notice).map_err(|e| format!("write NOTICE.txt: {e}"))?;
        if !pkg.required.iter().all(|f| staging.join(f).is_file()) {
            let _ = std::fs::remove_dir_all(staging);
            return Err(format!("{} is missing files after unpacking", pkg.id));
        }
        let dest = root.join(pkg.folder);
        if dest.exists() {
            std::fs::remove_dir_all(&dest).map_err(|e| format!("replace {}: {e}", dest.display()))?;
        }
        std::fs::rename(staging, &dest).map_err(|e| format!("install {}: {e}", pkg.id))?;
        tracing::info!(package = pkg.id, "Kokoro: installed");
    }
    Ok(())
}

/// Copy what Kokoro needs out of an unpacked eSpeak NG (the folder holding
/// `espeak-ng.exe`) into `dest`. Returns how many files.
fn copy_espeak(from: &Path, dest: &Path) -> Result<usize, String> {
    let mut n = 0;
    for name in ["espeak-ng.exe", "libespeak-ng.dll"] {
        std::fs::copy(from.join(name), dest.join(name)).map_err(|e| format!("eSpeak NG's {name}: {e}"))?;
        n += 1;
    }
    let mut stack = vec![from.join("espeak-ng-data")];
    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir).map_err(|e| format!("eSpeak NG's data: {e}"))?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let rel = path.strip_prefix(from).map_err(|e| e.to_string())?;
            let rel_s = rel.to_string_lossy().replace('\\', "/");
            if !keep_espeak_data(&rel_s) {
                continue;
            }
            let to = dest.join(rel);
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            std::fs::copy(&path, &to).map_err(|e| format!("eSpeak NG's {rel_s}: {e}"))?;
            n += 1;
        }
    }
    Ok(n)
}

/// The folder in `dir` (or below, two levels) holding `espeak-ng.exe`.
fn find_espeak(dir: &Path) -> Option<PathBuf> {
    if dir.join("espeak-ng.exe").is_file() {
        return Some(dir.to_path_buf());
    }
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let p = entry.path();
        if p.is_dir() {
            if p.join("espeak-ng.exe").is_file() {
                return Some(p);
            }
            for sub in std::fs::read_dir(&p).ok()?.flatten() {
                if sub.path().join("espeak-ng.exe").is_file() {
                    return Some(sub.path());
                }
            }
        }
    }
    None
}

/// Unpack eSpeak NG's installer into `dest`: `msiexec /a` (an
/// administrative install: the files only, nothing registered, no admin
/// rights) into a scratch folder, then the parts Kokoro needs copied over.
/// A debug build also takes a zip of the same layout (the e2e stand-in).
fn unpack_espeak(msi: &Path, dest: &Path) -> Result<usize, String> {
    let scratch = dest.with_extension("msi-unpacked");
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).map_err(|e| e.to_string())?;
    let result = (|| {
        #[cfg(debug_assertions)]
        if std::fs::read(msi).is_ok_and(|b| b.starts_with(b"PK\x03\x04")) {
            let file = std::fs::File::open(msi).map_err(|e| e.to_string())?;
            let mut zip = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
            zip.extract(&scratch).map_err(|e| e.to_string())?;
            let from = find_espeak(&scratch).ok_or("eSpeak NG's program isn't in the download")?;
            return copy_espeak(&from, dest);
        }
        // msiexec wants an .msi, by a Windows path.
        let named = scratch.join("espeak-ng.msi");
        std::fs::copy(msi, &named).map_err(|e| format!("eSpeak NG's installer: {e}"))?;
        let target = scratch.join("files");
        std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
        run_msiexec(&named, &target)?;
        let from = find_espeak(&target).ok_or("eSpeak NG's program isn't in its installer")?;
        copy_espeak(&from, dest)
    })();
    let _ = std::fs::remove_dir_all(&scratch);
    result
}

#[cfg(windows)]
fn run_msiexec(msi: &Path, target: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    use std::time::{Duration, Instant};
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let win = |p: &Path| p.display().to_string().replace('/', "\\");
    let mut child = std::process::Command::new("msiexec.exe")
        .raw_arg(format!("/a \"{}\" /qn TARGETDIR=\"{}\"", win(msi), win(target)))
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| format!("Couldn't unpack eSpeak NG (msiexec): {e}"))?;
    let deadline = Instant::now() + Duration::from_secs(180);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return match status.code() {
                    Some(0) => Ok(()),
                    Some(1618) => Err("Windows is installing something else right now. Try again in a minute.".into()),
                    code => Err(format!("Couldn't unpack eSpeak NG (msiexec said {})", code.map_or("nothing".to_string(), |c| c.to_string()))),
                };
            }
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(100)),
            _ => {
                let _ = child.kill();
                return Err("Unpacking eSpeak NG took too long".into());
            }
        }
    }
}

#[cfg(not(windows))]
fn run_msiexec(_msi: &Path, _target: &Path) -> Result<(), String> {
    Err("eSpeak NG's installer only unpacks on Windows".into())
}

/// Delete Kokoro and eSpeak NG (only Kokoro uses it).
pub fn delete(root: &Path) -> Result<(), String> {
    for pkg in [&KOKORO, &ESPEAK] {
        let dir = root.join(pkg.folder);
        if dir.exists() {
            std::fs::remove_dir_all(&dir).map_err(|e| format!("Couldn't delete {}: {e}", pkg.id))?;
        }
        let _ = std::fs::remove_dir_all(root.join(format!("{}.installing", pkg.folder)));
        for i in 0..pkg.artifacts.len() {
            let _ = std::fs::remove_file(root.join(format!("{}.{i}.partial", pkg.folder)));
        }
    }
    tracing::info!("Kokoro: deleted");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_registry_is_pinned_to_the_named_sources() {
        let mut dests = Vec::new();
        for pkg in [&KOKORO, &ESPEAK] {
            for a in pkg.artifacts {
                assert_eq!(a.sha256.len(), 64, "{}", a.url);
                assert!(a.sha256.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
                assert!(a.size > 0);
                let d = dest_name(a.dest);
                assert!(!dests.contains(&d), "duplicate {d}");
                dests.push(d);
                assert!(
                    a.url.starts_with(&format!("https://huggingface.co/onnx-community/Kokoro-82M-v1.0-ONNX/resolve/{HF_COMMIT}/"))
                        || a.url.starts_with(&format!("https://raw.githubusercontent.com/hexgrad/misaki/{MISAKI_COMMIT}/"))
                        || a.url.starts_with("https://github.com/espeak-ng/espeak-ng/releases/download/1.52.0/")
                        || a.url == "https://raw.githubusercontent.com/espeak-ng/espeak-ng/1.52.0/COPYING",
                    "{}",
                    a.url
                );
            }
        }
        // Every voice Settings lists is downloaded, and nothing else.
        let voices: Vec<&str> = KOKORO.artifacts.iter().filter_map(|a| match a.dest {
            Dest::File(p) => p.strip_prefix("voices/").and_then(|v| v.strip_suffix(".bin")),
            Dest::EspeakMsi => None,
        }).collect();
        assert_eq!(voices.len(), 54);
        for v in super::super::kokoro::VOICES {
            assert!(voices.contains(v), "{v}");
        }
        assert!(KOKORO.artifacts[0].url.ends_with("onnx/model.onnx"));
        assert_eq!(KOKORO.artifacts[0].size, 325_532_232);
        // ~379 MB in all.
        let total: u64 = [&KOKORO, &ESPEAK].iter().map(|p| bytes(p)).sum();
        assert_eq!(total, 379_147_994);
        assert!(ESPEAK.notice.contains("GNU General Public License") && ESPEAK.notice.contains("Source code"));
        assert!(KOKORO.notice.contains("Apache"));
    }

    #[test]
    fn espeak_keeps_only_what_kokoro_reads() {
        for keep in ["espeak-ng-data/phontab", "espeak-ng-data/en_dict", "espeak-ng-data/fr_dict", "espeak-ng-data/lang/gmw/en-GB-x-rp", "espeak-ng-data/voices/!v/Alex", "espeak-ng-data\\phondata"] {
            assert!(keep_espeak_data(keep), "{keep}");
        }
        for skip in ["espeak-ng-data/ru_dict", "espeak-ng-data/cmn_dict", "espeak-ng.msi", "README", "espeak-ng-data/../x"] {
            assert!(!keep_espeak_data(skip), "{skip}");
        }
    }

    fn temp_root(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("yap-tts-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn installed_means_every_file_at_its_size_and_delete_takes_both() {
        let root = temp_root("installed");
        assert!(!kokoro_installed(&root) && !espeak_installed(&root));
        let st = status(&root);
        assert!(!st.installed && st.download_bytes == st.size_bytes);
        let e = espeak_dir(&root);
        std::fs::create_dir_all(e.join("espeak-ng-data")).unwrap();
        for f in ESPEAK.required {
            std::fs::write(e.join(f), b"x").unwrap();
        }
        // COPYING at the wrong size: not installed.
        assert!(!espeak_installed(&root));
        std::fs::write(e.join("COPYING"), vec![b'x'; 35_147]).unwrap();
        assert!(espeak_installed(&root));
        assert_eq!(status(&root).download_bytes, bytes(&KOKORO));
        delete(&root).unwrap();
        assert!(!e.exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_stand_in_zip_unpacks_like_the_installer() {
        use std::io::Write;
        let root = temp_root("zip");
        let zip_path = root.join("espeak.zip");
        {
            let f = std::fs::File::create(&zip_path).unwrap();
            let mut z = zip::ZipWriter::new(f);
            let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
            for (name, body) in [
                ("eSpeak NG/espeak-ng.exe", "exe"),
                ("eSpeak NG/libespeak-ng.dll", "dll"),
                ("eSpeak NG/espeak-ng-data/phontab", "p"),
                ("eSpeak NG/espeak-ng-data/en_dict", "en"),
                ("eSpeak NG/espeak-ng-data/ru_dict", "ru"),
                ("eSpeak NG/espeak-ng-data/lang/gmw/en", "lang"),
            ] {
                z.start_file(name, opts).unwrap();
                z.write_all(body.as_bytes()).unwrap();
            }
            z.finish().unwrap();
        }
        let dest = root.join("out");
        std::fs::create_dir_all(&dest).unwrap();
        assert_eq!(unpack_espeak(&zip_path, &dest).unwrap(), 5);
        assert_eq!(std::fs::read_to_string(dest.join("espeak-ng-data/en_dict")).unwrap(), "en");
        assert!(dest.join("espeak-ng-data/lang/gmw/en").is_file());
        assert!(!dest.join("espeak-ng-data/ru_dict").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The real download, once, from the pinned sources into
    /// `YAP_TTS_DOWNLOAD_DIR` (else a temp folder): ~379 MB, and eSpeak NG's
    /// installer really unpacked with msiexec. `cargo test
    /// tts::registry::tests::real_download -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn real_download_from_the_pinned_sources() {
        let root = std::env::var_os("YAP_TTS_DOWNLOAD_DIR").map(PathBuf::from).unwrap_or_else(|| temp_root("download"));
        let rt = tokio::runtime::Runtime::new().unwrap();
        let last = std::sync::Mutex::new(0u8);
        rt.block_on(install(&root, &|p: Progress| {
            let mut l = last.lock().unwrap();
            if p.percent >= *l + 10 || p.percent == 100 {
                *l = p.percent;
                eprintln!("{}% ({} / {} bytes)", p.percent, p.downloaded_bytes, p.total_bytes);
            }
        }))
        .unwrap();
        assert!(kokoro_installed(&root) && espeak_installed(&root));
        assert_eq!(dl::sha256(&kokoro_dir(&root).join(MODEL_FILE)).unwrap(), KOKORO.artifacts[0].sha256);
        // eSpeak NG runs from where it was unpacked.
        let e = super::super::g2p::Espeak { dir: espeak_dir(&root) };
        let ipa = e.ipa("en-gb", &["Priya".to_string(), "hello".to_string()]).unwrap();
        eprintln!("eSpeak NG: {ipa:?}");
        assert!(ipa.iter().all(|l| !l.is_empty()));
        eprintln!("installed into {}", root.display());
    }
}
