//! **The graphics card**, for picking the on-device model (`local_llm.rs`):
//! Qwen3.5-4B is recommended where the card can hold it, Qwen2.5-1.5B
//! elsewhere.
//!
//! Read with DXGI (`CreateDXGIFactory1` → `EnumAdapters1` → `GetDesc1`):
//! every adapter's name, vendor and **dedicated video memory**, leaving out
//! software adapters (`DXGI_ADAPTER_FLAG_SOFTWARE`, Microsoft's Basic Render
//! Driver). Read-only, local, cached for the run (cards don't change while
//! Yap runs; a new one is seen at the next start).
//!
//! **The rule** ([`can_run_big_model`]): a card with at least
//! [`BIG_MODEL_VRAM_MIB`] of its own memory. What the 4B needs there
//! (llama.cpp's buffers, read from the sidecar's log on an RTX 5070 Ti, all
//! 34 layers on the card): on llamafile 0.10.6 the model 2,741 MiB, the 8k
//! context's KV 256 MiB and recurrent state 201 MiB, compute 72 MiB —
//! ~3.2 GiB (0.10.3: ~3.7); MTP drafting, only where it fits
//! (`local_llm::mtp_fits`), adds ~0.85 GiB. Besides it,
//! Yap's speech model sits on the same card (Parakeet through DirectML ~1
//! GB, a large Whisper through Vulkan up to ~1.6 GB) and Windows and the
//! apps on screen keep ~0.5–1 GB. A "6 GB" card (which reports 5.9–6 GiB)
//! holds all of it; a 4 GB one would leave layers on the processor, where
//! the 4B takes ~25 s a Chat turn. Integrated graphics report little of
//! their own (they borrow the PC's memory: the test PC's Ryzen's Radeon reports
//! 2 GiB) and don't pass. The sidecar's log says afterwards where the
//! layers really went (`local_llm::placement`), and Settings says so when
//! it was the processor.

use serde::Serialize;
use std::sync::OnceLock;

/// The dedicated video memory from which a card is offered the 4B: 5.5 GiB,
/// so a "6 GB" card counts (see the module comment for the sums).
pub const BIG_MODEL_VRAM_MIB: u64 = 5_632;

/// One graphics adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Gpu {
    pub name: String,
    /// Its own (dedicated) video memory, MiB.
    pub vram_mib: u64,
    /// PCI vendor id (0x10DE NVIDIA, 0x1002 AMD, 0x8086 Intel).
    pub vendor: u32,
}

/// Microsoft's Basic Render Driver (and WARP): software, never a GPU.
const MICROSOFT_VENDOR: u32 = 0x1414;

/// The card with the most memory of its own, if any. Pure.
pub fn best(adapters: &[Gpu]) -> Option<&Gpu> {
    adapters
        .iter()
        .filter(|g| g.vendor != MICROSOFT_VENDOR)
        .max_by_key(|g| g.vram_mib)
}

/// Can this card run the bigger on-device model (Qwen3.5-4B) at speed? Pure.
pub fn can_run_big_model(gpu: Option<&Gpu>) -> bool {
    gpu.is_some_and(|g| g.vendor != MICROSOFT_VENDOR && g.vram_mib >= BIG_MODEL_VRAM_MIB)
}

/// This PC's best graphics card (cached), or `None`.
pub fn this_pc() -> Option<Gpu> {
    static CACHE: OnceLock<Option<Gpu>> = OnceLock::new();
    #[cfg(debug_assertions)]
    if let Some(fake) = e2e_gpu() {
        return fake;
    }
    CACHE
        .get_or_init(|| {
            let all = adapters();
            let gpu = best(&all).cloned();
            tracing::info!(
                gpu = gpu.as_ref().map(|g| g.name.as_str()).unwrap_or("none"),
                vram_mib = gpu.as_ref().map(|g| g.vram_mib).unwrap_or(0),
                adapters = all.len(),
                "Graphics card"
            );
            gpu
        })
        .clone()
}

/// Test mode (debug builds): `YAP_E2E_GPU` = "none", or "<name>,<MiB>",
/// stands in for the card (the e2e suite's "Recommended for this PC").
#[cfg(debug_assertions)]
fn e2e_gpu() -> Option<Option<Gpu>> {
    if !crate::e2e::active() {
        return None;
    }
    let spec = std::env::var("YAP_E2E_GPU").ok()?;
    if spec.trim().eq_ignore_ascii_case("none") || spec.trim().is_empty() {
        return Some(None);
    }
    let (name, mib) = spec.rsplit_once(',')?;
    Some(Some(Gpu { name: name.trim().to_string(), vram_mib: mib.trim().parse().ok()?, vendor: 0x10DE }))
}

/// Less than this much memory of its own: integrated graphics (they borrow
/// the PC's memory and report a small carve-out: this PC's Ryzen Radeon 2
/// GiB, Intel's UHD/Iris 128 MiB) or an entry-level card. Measured
/// (`yap.exe bench`, docs/benchmarks.md): Parakeet on DirectML on such a
/// GPU can be slower than on the processor, so a speech model starts on the
/// processor there until this PC's own timing says otherwise
/// (`hardware::speech_device_before_measuring`). A "4 GB" card (3.9 GiB)
/// isn't small.
pub const SMALL_GPU_MIB: u64 = 3_072;

/// A small graphics adapter ([`SMALL_GPU_MIB`]). Pure.
pub fn is_small(gpu: &Gpu) -> bool {
    gpu.vram_mib < SMALL_GPU_MIB
}

/// One graphics adapter as DXGI lists it: its index there is DirectML's
/// device id (`yap.exe bench` runs a model on each). `software`: Microsoft's
/// Basic Render Driver (WARP), what DirectML gets on a PC with no graphics
/// driver.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Adapter {
    pub index: u32,
    #[serde(flatten)]
    pub gpu: Gpu,
    pub software: bool,
}

/// Every graphics adapter on this PC, software ones too, in DXGI's order.
#[cfg(windows)]
pub fn all_adapters() -> Vec<Adapter> {
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE};
    let mut out = Vec::new();
    // SAFETY: plain COM calls on a factory this function owns; every
    // adapter's description is copied out before it's released.
    unsafe {
        let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory1>() else {
            return out;
        };
        let mut i = 0u32;
        while let Ok(adapter) = factory.EnumAdapters1(i) {
            let index = i;
            i += 1;
            let Ok(desc) = adapter.GetDesc1() else { continue };
            let len = desc.Description.iter().position(|&c| c == 0).unwrap_or(desc.Description.len());
            out.push(Adapter {
                index,
                gpu: Gpu {
                    name: String::from_utf16_lossy(&desc.Description[..len]).trim().to_string(),
                    vram_mib: desc.DedicatedVideoMemory as u64 / 1_048_576,
                    vendor: desc.VendorId,
                },
                software: desc.Flags & (DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32) != 0 || desc.VendorId == MICROSOFT_VENDOR,
            });
            if i > 16 {
                break;
            }
        }
    }
    out
}

#[cfg(not(windows))]
pub fn all_adapters() -> Vec<Adapter> {
    Vec::new()
}

/// The adapter DirectML runs the speech models on: the card with the most
/// memory of its own when DXGI doesn't list it first. A laptop with an
/// integrated GPU and a graphics card typically lists the integrated one
/// first (the screen hangs off it), and DirectML's default device is DXGI's
/// first: Parakeet ran on the integrated GPU, not the card. `None`: the
/// default (it is the first, or there's no card). Pure.
pub fn speech_adapter_of(all: &[Adapter]) -> Option<u32> {
    let best = all.iter().filter(|a| !a.software && a.gpu.vendor != MICROSOFT_VENDOR).max_by_key(|a| a.gpu.vram_mib)?;
    let first = all.first()?;
    (best.index != first.index).then_some(best.index)
}

/// [`speech_adapter_of`] this PC (never in a test run: its card is a
/// stand-in).
pub fn speech_adapter() -> Option<u32> {
    if crate::e2e::active() {
        return None;
    }
    speech_adapter_of(&all_adapters())
}

/// The hardware graphics adapters (no software ones).
fn adapters() -> Vec<Gpu> {
    all_adapters().into_iter().filter(|a| !a.software).map(|a| a.gpu).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gpu(name: &str, vram_mib: u64, vendor: u32) -> Gpu {
        Gpu { name: name.into(), vram_mib, vendor }
    }

    #[test]
    fn a_card_with_six_gigabytes_of_its_own_runs_the_bigger_model() {
        // The test PC's RTX 5070 Ti (16 GB), and "6 GB" cards as DXGI reports them.
        assert!(can_run_big_model(Some(&gpu("NVIDIA GeForce RTX 5070 Ti", 15_907, 0x10DE))));
        assert!(can_run_big_model(Some(&gpu("NVIDIA GeForce GTX 1660", 5_966, 0x10DE))));
        assert!(can_run_big_model(Some(&gpu("AMD Radeon RX 7600", 8_176, 0x1002))));
        assert!(can_run_big_model(Some(&gpu("Intel(R) Arc(TM) A750", 8_048, 0x8086))));
        // 4 GB leaves layers on the processor; integrated graphics borrow
        // the PC's memory and report little of their own.
        assert!(!can_run_big_model(Some(&gpu("NVIDIA GeForce GTX 1650", 3_935, 0x10DE))));
        assert!(!can_run_big_model(Some(&gpu("Intel(R) UHD Graphics 770", 128, 0x8086))));
        assert!(!can_run_big_model(Some(&gpu("AMD Radeon(TM) Graphics", 512, 0x1002))));
        // Software rendering, or none at all.
        assert!(!can_run_big_model(Some(&gpu("Microsoft Basic Render Driver", 8_192, 0x1414))));
        assert!(!can_run_big_model(None));
    }

    #[test]
    fn speech_runs_on_the_card_even_when_the_integrated_gpu_is_listed_first() {
        let a = |index, name: &str, mib, vendor, software| Adapter { index, gpu: gpu(name, mib, vendor), software };
        // A hybrid laptop: Intel's integrated GPU first, the card second.
        let laptop = [a(0, "Intel(R) Iris(R) Xe Graphics", 128, 0x8086, false), a(1, "NVIDIA GeForce RTX 4060 Laptop GPU", 7_957, 0x10DE, false), a(2, "Microsoft Basic Render Driver", 0, 0x1414, true)];
        assert_eq!(speech_adapter_of(&laptop), Some(1));
        // A desktop with its card first (and the processor's graphics after): the default.
        let desktop = [a(0, "NVIDIA GeForce RTX 5070 Ti", 15_907, 0x10DE, false), a(1, "AMD Radeon(TM) Graphics", 2_048, 0x1002, false)];
        assert_eq!(speech_adapter_of(&desktop), None);
        // Integrated graphics only, or none at all.
        assert_eq!(speech_adapter_of(&laptop[..1]), None);
        assert_eq!(speech_adapter_of(&laptop[2..]), None);
        assert_eq!(speech_adapter_of(&[]), None);
    }

    #[test]
    fn integrated_graphics_and_entry_cards_are_small() {
        assert!(is_small(&gpu("AMD Radeon(TM) Graphics", 2_048, 0x1002)));
        assert!(is_small(&gpu("Intel(R) Iris(R) Xe Graphics", 128, 0x8086)));
        assert!(is_small(&gpu("NVIDIA GeForce MX450", 2_002, 0x10DE)));
        assert!(!is_small(&gpu("NVIDIA GeForce GTX 1650", 3_935, 0x10DE)));
        assert!(!is_small(&gpu("NVIDIA GeForce RTX 5070 Ti", 15_907, 0x10DE)));
    }

    #[test]
    fn the_best_card_is_the_one_with_the_most_memory_of_its_own() {
        let all = [
            gpu("Intel(R) UHD Graphics 770", 128, 0x8086),
            gpu("NVIDIA GeForce RTX 4060 Laptop GPU", 7_957, 0x10DE),
            gpu("Microsoft Basic Render Driver", 0, 0x1414),
        ];
        assert_eq!(best(&all).map(|g| g.name.as_str()), Some("NVIDIA GeForce RTX 4060 Laptop GPU"));
        assert_eq!(best(&all[2..]), None);
        assert_eq!(best(&[]), None);
    }

    /// This PC's cards, read-only (`cargo test gpu -- --ignored --nocapture`).
    #[test]
    #[ignore]
    fn this_machine() {
        let all = adapters();
        println!("{all:#?}");
        println!("best: {:?}, capable: {}", best(&all), can_run_big_model(best(&all)));
    }
}
