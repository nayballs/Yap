//! **The releases feed**: ggml-org/llama.cpp's GitHub releases, read once a
//! day through `privacy/net.rs` ("On-device AI · engine update check" in
//! Network connections). Only builds (`b12345` tags; llama.cpp also tags
//! other things, `v0.8.0`), never drafts, each file with the SHA-256 GitHub
//! publishes for it (an asset without one is left out: nothing unverified
//! is ever downloaded), and only once a build is [`COOLING_OFF`] old — a
//! broken build is usually followed by a fix within a day or two.
//!
//! llama.cpp publishes ~15 builds a day, each a GitHub release (marked a
//! pre-release) of ~70 KB of JSON: the feed is read 50 at a time, newest
//! first, until it reaches back past the cooling-off (usually one or two
//! pages, ~3.5 MB each).

use std::time::Duration;

use super::{Asset, Build, Variant};
use crate::privacy::net::Recorded as _;

/// The feed: ggml-org/llama.cpp's releases on GitHub's API.
pub const FEED: &str = "https://api.github.com/repos/ggml-org/llama.cpp/releases";
/// A build is taken once it's this old (seconds): 3 days.
pub const COOLING_OFF: u64 = 3 * 86_400;
const PER_PAGE: u32 = 50;
const MAX_PAGES: u32 = 4;

/// One build on the feed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub build: Build,
    /// When it was published (unix seconds).
    pub published: u64,
    pub assets: Vec<Asset>,
}

/// GitHub's asset digest, "sha256:<64 hex>", as the hex. Pure.
pub fn digest(s: &str) -> Option<String> {
    let hex = s.trim().strip_prefix("sha256:")?;
    (hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit())).then(|| hex.to_ascii_lowercase())
}

/// "2026-10-29T12:17:04Z" as unix seconds. Pure.
pub fn timestamp(s: &str) -> Option<u64> {
    chrono::DateTime::parse_from_rfc3339(s.trim()).ok().and_then(|t| u64::try_from(t.timestamp()).ok())
}

/// The builds on one page of GitHub's releases API: `b12345` tags only,
/// not drafts, each asset uploaded whole with its SHA-256 (any other left
/// out), a plain file name. Pure.
pub fn parse_page(v: &serde_json::Value) -> Vec<Release> {
    let Some(list) = v.as_array() else { return Vec::new() };
    list.iter()
        .filter_map(|r| {
            if r["draft"].as_bool().unwrap_or(true) {
                return None;
            }
            let build = Build::parse(r["tag_name"].as_str()?)?;
            let published = timestamp(r["published_at"].as_str()?)?;
            let assets = r["assets"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|a| {
                            let name = a["name"].as_str()?;
                            let plain = !name.is_empty() && !name.contains(['/', '\\', ':']) && !name.contains("..");
                            let uploaded = a["state"].as_str().is_none_or(|s| s == "uploaded");
                            if !plain || !uploaded {
                                return None;
                            }
                            Some(Asset { name: name.to_string(), sha256: digest(a["digest"].as_str()?)?, size: a["size"].as_u64().unwrap_or(0), url: None })
                        })
                        .collect()
                })
                .unwrap_or_default();
            Some(Release { build, published, assets })
        })
        .collect()
}

/// The CUDA version in a CUDA file's name after `cuda-` up to `-x64.zip`
/// ("13.4"), as numbers for ordering. Pure.
fn cuda_version(name: &str) -> Option<(String, Vec<u32>)> {
    let rest = name.split("-cuda-").nth(1)?.strip_suffix("-x64.zip")?;
    let parts: Option<Vec<u32>> = rest.split('.').map(|p| p.parse().ok()).collect();
    Some((rest.to_string(), parts?))
}

/// **The files of `release` for `variant`**: its Windows x64 zip, and for
/// CUDA NVIDIA's runtime of the same CUDA version (the newest minor where
/// llama.cpp ships two). Pure.
pub fn pick_assets(release: &Release, variant: Variant) -> Option<(Asset, Option<Asset>)> {
    let prefix = format!("llama-{}-bin-win-{}", release.build, variant.asset_part());
    if !variant.cuda() {
        let want = format!("{prefix}-x64.zip");
        return release.assets.iter().find(|a| a.name == want).map(|a| (a.clone(), None));
    }
    let mut zips: Vec<(&Asset, String, Vec<u32>)> = release
        .assets
        .iter()
        .filter(|a| a.name.starts_with(&prefix) && a.name.ends_with("-x64.zip"))
        .filter_map(|a| cuda_version(&a.name).map(|(s, v)| (a, s, v)))
        .collect();
    zips.sort_by(|a, b| b.2.cmp(&a.2));
    zips.into_iter().find_map(|(zip, version, _)| {
        let want = format!("cudart-llama-bin-win-cuda-{version}-x64.zip");
        release.assets.iter().find(|a| a.name == want).map(|rt| (zip.clone(), Some(rt.clone())))
    })
}

/// May `release` be taken at `now` for one of `need`: old enough, with the
/// files for that variant, not refused here (`refused(key)`)? Pure.
pub fn takes(release: &Release, now: u64, need: &[Variant], refused: &dyn Fn(&str) -> bool) -> bool {
    release.published + COOLING_OFF <= now
        && need.iter().any(|v| pick_assets(release, *v).is_some() && !refused(&format!("{}-{}", release.build, v.id())))
}

/// **The newest build that may be taken** at `now` ([`takes`]). Pure.
pub fn eligible<'a>(releases: &'a [Release], now: u64, need: &[Variant], refused: &dyn Fn(&str) -> bool) -> Option<&'a Release> {
    releases.iter().filter(|r| takes(r, now, need, refused)).max_by_key(|r| r.build)
}

/// Do `releases` reach back past the cooling-off (so the newest that may
/// be taken is among them, or there's none newer than what they hold)?
/// Pure.
pub fn reaches_back(releases: &[Release], now: u64) -> bool {
    releases.iter().any(|r| r.published + COOLING_OFF <= now)
}

/// The test run's stand-in feed (`YAP_E2E_ENGINE_FEED`, its base address on
/// 127.0.0.1; debug builds): its releases at `/releases`, its files at
/// `/download/<tag>/<name>`.
#[cfg(debug_assertions)]
pub fn stand_in_base() -> Option<String> {
    if !crate::shell::e2e::active() {
        return None;
    }
    std::env::var("YAP_E2E_ENGINE_FEED").ok().map(|u| u.trim().trim_end_matches('/').to_string()).filter(|u| !u.is_empty())
}

fn feed_url() -> String {
    #[cfg(debug_assertions)]
    if let Some(base) = stand_in_base() {
        return format!("{base}/releases");
    }
    FEED.to_string()
}

/// **Read the feed** for the newest build that may be taken now for one of
/// `need`: pages of [`PER_PAGE`], newest first, until they reach back past
/// the cooling-off. `Ok(None)`: none (newer than what's refused) yet.
pub async fn newest(need: &[Variant], refused: &(dyn Fn(&str) -> bool + Sync)) -> Result<Option<Release>, String> {
    let url = feed_url();
    if crate::shell::e2e::active() && !crate::ai::eval::active() && !super::super::models::allowed_in_test_run(&url) {
        return Err("A test run never reads the real releases".to_string());
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent(concat!("Yap/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| e.to_string())?;
    let now = super::now_secs();
    let mut seen: Vec<Release> = Vec::new();
    for page in 1..=MAX_PAGES {
        let resp = client
            .get(&url)
            .query(&[("per_page", PER_PAGE), ("page", page)])
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .send_for(crate::privacy::net::Purpose::EngineCheck)
            .await
            .map_err(|e| format!("Couldn't reach GitHub: {e}"))?;
        let status = resp.status();
        if status.as_u16() == 403 || status.as_u16() == 429 {
            return Err("GitHub asked Yap to wait (its limit for checks): it tries again later".to_string());
        }
        if !status.is_success() {
            return Err(format!("GitHub answered HTTP {}", status.as_u16()));
        }
        let v: serde_json::Value = resp.json().await.map_err(|e| format!("GitHub's answer didn't read: {e}"))?;
        let got = parse_page(&v);
        let empty = v.as_array().is_none_or(|a| a.is_empty());
        seen.extend(got);
        if empty || reaches_back(&seen, now) {
            break;
        }
    }
    Ok(eligible(&seen, now, need, refused).cloned())
}

/// One build's release, by its tag (the model evaluations' `--engine
/// b12345`; debug builds).
#[cfg(debug_assertions)]
pub async fn release(build: Build) -> Result<Release, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent(concat!("Yap/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .get(format!("{FEED}/tags/{build}"))
        .header("Accept", "application/vnd.github+json")
        .send_for(crate::privacy::net::Purpose::EngineCheck)
        .await
        .map_err(|e| format!("Couldn't reach GitHub: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("GitHub has no release {build} (HTTP {})", resp.status().as_u16()));
    }
    let v: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    parse_page(&serde_json::Value::Array(vec![v])).into_iter().next().ok_or_else(|| format!("{build} isn't a published build"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: u64 = 86_400;

    fn asset(name: &str) -> Asset {
        Asset { name: name.into(), sha256: "ab".repeat(32), size: 1, url: None }
    }

    fn release(build: u32, published: u64, cuda: &[&str]) -> Release {
        let mut assets = vec![
            asset(&format!("llama-b{build}-bin-win-cpu-x64.zip")),
            asset(&format!("llama-b{build}-bin-win-vulkan-x64.zip")),
            asset(&format!("llama-b{build}-bin-win-vulkan-arm64.zip")),
            asset(&format!("llama-b{build}-bin-ubuntu-x64.tar.gz")),
        ];
        for v in cuda {
            assets.push(asset(&format!("llama-b{build}-bin-win-cuda-{v}-x64.zip")));
            assets.push(asset(&format!("cudart-llama-bin-win-cuda-{v}-x64.zip")));
        }
        Release { build: Build(build), published, assets }
    }

    #[test]
    fn the_feed_reads_builds_with_their_digests_only() {
        let page = serde_json::json!([
            {"tag_name": "b12032", "draft": false, "prerelease": true, "published_at": "2026-11-01T09:00:59Z",
             "assets": [
                {"name": "llama-b12032-bin-win-cpu-x64.zip", "size": 24582754, "state": "uploaded",
                 "digest": "sha256:5296f5d18bb7d422c7c26f4bb9bc796d9d4f41e1312a8c296cff475bdc60d0d4"},
                {"name": "llama-b12032-bin-win-vulkan-x64.zip", "size": 1, "state": "uploaded", "digest": null},
                {"name": "llama-b12032-bin-win-sycl-x64.zip", "size": 1, "state": "starter",
                 "digest": "sha256:5296f5d18bb7d422c7c26f4bb9bc796d9d4f41e1312a8c296cff475bdc60d0d4"},
                {"name": "../evil.zip", "size": 1, "digest": "sha256:5296f5d18bb7d422c7c26f4bb9bc796d9d4f41e1312a8c296cff475bdc60d0d4"}
             ]},
            {"tag_name": "v0.8.0", "draft": false, "published_at": "2026-10-28T12:47:51Z", "assets": []},
            {"tag_name": "b12033", "draft": true, "published_at": "2026-11-01T10:00:00Z", "assets": []},
            {"tag_name": "b12000", "draft": false, "published_at": "not a date", "assets": []}
        ]);
        let r = parse_page(&page);
        assert_eq!(r.len(), 1, "only the published build");
        assert_eq!(r[0].build, Build(12_032));
        assert_eq!(r[0].published, timestamp("2026-11-01T09:00:59Z").unwrap());
        assert_eq!(r[0].assets.len(), 1, "no digest, not uploaded, or not a plain name: left out");
        assert_eq!(r[0].assets[0].sha256, "5296f5d18bb7d422c7c26f4bb9bc796d9d4f41e1312a8c296cff475bdc60d0d4");
        assert_eq!(r[0].assets[0].size, 24_582_754);
        assert_eq!(digest("sha256:ABCD"), None);
        assert_eq!(digest("md5:5296f5d18bb7d422c7c26f4bb9bc796d9d4f41e1312a8c296cff475bdc60d0d4"), None);
        assert!(parse_page(&serde_json::json!({"message": "API rate limit exceeded"})).is_empty());
    }

    #[test]
    fn the_variants_files_cuda_with_its_own_runtime() {
        let r = release(11_982, 0, &["12.4", "13.4"]);
        let (zip, rt) = pick_assets(&r, Variant::Cuda13).unwrap();
        assert_eq!(zip.name, "llama-b11982-bin-win-cuda-13.4-x64.zip");
        assert_eq!(rt.unwrap().name, "cudart-llama-bin-win-cuda-13.4-x64.zip");
        let (zip, rt) = pick_assets(&r, Variant::Cuda12).unwrap();
        assert_eq!((zip.name.as_str(), rt.unwrap().name.as_str()), ("llama-b11982-bin-win-cuda-12.4-x64.zip", "cudart-llama-bin-win-cuda-12.4-x64.zip"));
        assert_eq!(pick_assets(&r, Variant::Vulkan).unwrap(), (asset("llama-b11982-bin-win-vulkan-x64.zip"), None), "x64, not arm64");
        assert_eq!(pick_assets(&r, Variant::Cpu).unwrap().0.name, "llama-b11982-bin-win-cpu-x64.zip");
        // A newer CUDA minor is followed; two of them: the newest.
        let r = release(12_100, 0, &["13.4", "13.6"]);
        assert_eq!(pick_assets(&r, Variant::Cuda13).unwrap().0.name, "llama-b12100-bin-win-cuda-13.6-x64.zip");
        // A CUDA zip without its runtime is no use.
        let mut r = release(12_101, 0, &["13.4"]);
        r.assets.retain(|a| !a.name.starts_with("cudart"));
        assert_eq!(pick_assets(&r, Variant::Cuda13), None);
        assert_eq!(pick_assets(&r, Variant::Cuda12), None);
    }

    #[test]
    fn only_a_build_three_days_old_is_taken_and_never_a_refused_one() {
        let now = 100 * DAY;
        let releases = vec![
            release(12_032, now - DAY / 2, &["13.4"]),
            release(12_000, now - 2 * DAY, &["13.4"]),
            release(11_982, now - 3 * DAY - 1, &["13.4"]),
            release(11_960, now - 4 * DAY, &["13.4"]),
        ];
        let none = |_: &str| false;
        let need = [Variant::Cuda13, Variant::Vulkan];
        assert_eq!(eligible(&releases, now, &need, &none).map(|r| r.build), Some(Build(11_982)));
        assert!(!takes(&releases[1], now, &need, &none), "2 days: too new");
        // Refused for both variants here: the next one down.
        let refused = |k: &str| k.starts_with("b11982");
        assert_eq!(eligible(&releases, now, &need, &refused).map(|r| r.build), Some(Build(11_960)));
        // Refused for CUDA only: still taken (Vulkan can be tried).
        let cuda_only = |k: &str| k == "b11982-cuda13";
        assert_eq!(eligible(&releases, now, &need, &cuda_only).map(|r| r.build), Some(Build(11_982)));
        // A build without the files this PC needs isn't taken.
        let mut no_cpu = releases.clone();
        for r in &mut no_cpu {
            r.assets.retain(|a| !a.name.contains("-cpu-"));
        }
        assert_eq!(eligible(&no_cpu, now, &[Variant::Cpu], &none), None);
        // Reaching back: the page has one older than the cooling-off.
        assert!(reaches_back(&releases, now));
        assert!(!reaches_back(&releases[..2], now));
        assert!(eligible(&[], now, &need, &none).is_none());
    }
}
