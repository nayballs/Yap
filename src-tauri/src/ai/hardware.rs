//! **Picks by hardware** (2026-10-24): what suits this PC — the speech
//! model and where it runs before this PC's own timing exists
//! (`stt_speed.rs` measures it a minute after start), and which AI cleanup
//! — from its graphics card, processor threads and memory. The thresholds
//! and the latencies quoted come from `yap.exe bench` (bench.rs,
//! docs/benchmarks.md): this PC, and the same PC held to 4 and 2 threads
//! (emulated small laptops).
//!
//! Three tiers ([`tier`]):
//! - **card**: a graphics card with memory of its own ([`crate::ai::gpu::is_small`]
//!   says it isn't integrated graphics or an entry card): speech on the card,
//!   the on-device cleanup on the card (Qwen3.5 4B where it fits, else the
//!   1.5B);
//! - **processor**: no such card, but 8 or more threads: speech on the
//!   processor (int8 Parakeet is quick there), the 1.5B on the processor;
//! - **small**: fewer threads (or under 8 GB of memory): speech on the
//!   processor still, and cleanup by a cloud provider is offered first —
//!   the 1.5B adds about 1.6 s to each dictation there on this desktop's
//!   cores held to 2 threads (more on a budget laptop's) and takes ~1.5 GB
//!   of a small memory, so it's offered with its time said.

use serde::Serialize;

use crate::ai::gpu::Gpu;
use crate::dictation::stt::Device;

/// The PC as the picks see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Machine {
    /// The best graphics adapter (`gpu::this_pc`).
    pub gpu: Option<Gpu>,
    /// Logical processors.
    pub threads: u32,
    /// Memory, MiB.
    pub ram_mib: u64,
}

impl Machine {
    /// From what `yap.exe bench` read.
    pub fn from_hardware(hw: &crate::shell::bench::Hardware) -> Machine {
        let gpu = crate::ai::gpu::best(&hw.gpus().map(|a| a.gpu.clone()).collect::<Vec<_>>()).cloned();
        Machine { gpu, threads: hw.threads, ram_mib: u64::from(hw.ram_gb) * 1024 }
    }

    /// This PC (cached). Test mode (debug builds): `YAP_E2E_CPU_THREADS` and
    /// `YAP_E2E_RAM_MIB` stand in for the processor and memory, as
    /// `YAP_E2E_GPU` does for the card.
    pub fn this_pc() -> Machine {
        static CACHE: std::sync::OnceLock<(u32, u64)> = std::sync::OnceLock::new();
        let (threads, ram_mib) = *CACHE.get_or_init(|| {
            let threads = std::thread::available_parallelism().map(|n| n.get() as u32).unwrap_or(4);
            (threads, ram_mib())
        });
        #[cfg_attr(not(debug_assertions), allow(unused_mut))]
        let mut m = Machine { gpu: crate::ai::gpu::this_pc(), threads, ram_mib };
        #[cfg(debug_assertions)]
        if crate::shell::e2e::active() {
            let var = |k: &str| std::env::var(k).ok().and_then(|v| v.trim().parse::<u64>().ok());
            if let Some(t) = var("YAP_E2E_CPU_THREADS") {
                m.threads = t as u32;
            }
            if let Some(r) = var("YAP_E2E_RAM_MIB") {
                m.ram_mib = r;
            }
        }
        m
    }
}

fn ram_mib() -> u64 {
    #[cfg(windows)]
    {
        use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
        let mut m = MEMORYSTATUSEX { dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
        // SAFETY: a sized struct this function owns.
        if unsafe { GlobalMemoryStatusEx(&mut m) }.is_ok() {
            return m.ullTotalPhys / 1_048_576;
        }
    }
    16 * 1024
}

/// What kind of PC it is for Yap's models.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    Card,
    Processor,
    Small,
}

/// A processor with fewer threads than this is a small PC's.
pub const PROCESSOR_THREADS: u32 = 8;
/// …and so is one with less memory than this (MiB): Windows, a browser and
/// the 1.5B's ~1.5 GB leave little room under 8 GB.
pub const PROCESSOR_RAM_MIB: u64 = 7_680;

/// The tier ([`Tier`]). Pure.
pub fn tier(m: &Machine) -> Tier {
    if m.gpu.as_ref().is_some_and(|g| !crate::ai::gpu::is_small(g)) {
        Tier::Card
    } else if m.threads >= PROCESSOR_THREADS && m.ram_mib >= PROCESSOR_RAM_MIB {
        Tier::Processor
    } else {
        Tier::Small
    }
}

/// Where a speech model runs until this PC's own timing says (`stt_speed`):
/// the graphics card when there's one that isn't small, else the processor
/// — without a card DirectML would run on Windows' software renderer (no
/// graphics driver: Parakeet V3 ~1 s for 11 s of speech) or a weak
/// integrated GPU (~4 s: the encoder runs at its 30-s plan), both slower
/// than int8 Parakeet on 4 processor threads (~0.4 s; docs/benchmarks.md).
/// Pure.
pub fn speech_device_before_measuring(gpu: Option<&Gpu>) -> Device {
    match gpu {
        Some(g) if !crate::ai::gpu::is_small(g) => Device::Gpu,
        _ => Device::Cpu,
    }
}

/// [`speech_device_before_measuring`] for this PC. (A test run without a
/// stand-in card, `YAP_E2E_GPU`, keeps the graphics card, whatever the
/// machine it runs on has.)
pub fn speech_device_here() -> Device {
    #[cfg(debug_assertions)]
    if crate::shell::e2e::active() && std::env::var_os("YAP_E2E_GPU").is_none() {
        return Device::Gpu;
    }
    speech_device_before_measuring(crate::ai::gpu::this_pc().as_ref())
}

/// The speech model recommended everywhere: Parakeet V3 (int8). It's the
/// most accurate of the quick ones on every tier measured, the processor
/// included (docs/benchmarks.md).
pub const SPEECH_MODEL: &str = "parakeet-tdt-0.6b-v3";

/// How the cleanup is best done here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CleanupPick {
    /// The on-device model (private).
    Local,
    /// A cloud provider with the person's key (quicker here).
    Cloud,
}

/// Yap's picks for a PC.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Advice {
    pub tier: Tier,
    pub speech_model: &'static str,
    pub speech_device: Device,
    pub cleanup: CleanupPick,
    /// The on-device model for this PC (a curated id).
    pub local_model: &'static str,
    /// What the on-device cleanup adds to a dictation here, about (ms).
    pub local_ms: u64,
    /// What a cloud cleanup adds, about (ms; Groq's free tier, measured).
    pub cloud_ms: u64,
    /// One line for people.
    pub summary: String,
}

/// Measured cleanup times (ms per dictation, median; `yap.exe bench` on the
/// reference PC, 2026-10-24, docs/benchmarks.md): Qwen3.5 4B and Qwen2.5
/// 1.5B on an RTX 5070 Ti (159 and 80 ms); the 1.5B on the processor held
/// to 4 threads (1,082 ms) for the processor tier — a laptop's 8 or more
/// threads are slower than this desktop's 4 — and to 2 threads (1,606 ms)
/// for a small PC, its cores slower still; Groq's openai/gpt-oss-20b, the
/// cloud default (474–849 ms).
pub const LOCAL_MS_CARD_4B: u64 = 160;
pub const LOCAL_MS_CARD_1_5B: u64 = 80;
pub const LOCAL_MS_PROCESSOR: u64 = 1_100;
pub const LOCAL_MS_SMALL: u64 = 1_600;
pub const CLOUD_MS: u64 = 800;

/// The picks for `m`. Pure.
pub fn advise(m: &Machine) -> Advice {
    let tier = tier(m);
    let big = crate::ai::gpu::can_run_big_model(m.gpu.as_ref());
    let local_model = crate::ai::local_llm::models::recommended_id(big);
    let (cleanup, local_ms) = match tier {
        Tier::Card => (CleanupPick::Local, if big { LOCAL_MS_CARD_4B } else { LOCAL_MS_CARD_1_5B }),
        Tier::Processor => (CleanupPick::Local, LOCAL_MS_PROCESSOR),
        Tier::Small => (CleanupPick::Cloud, LOCAL_MS_SMALL),
    };
    let speech_device = speech_device_before_measuring(m.gpu.as_ref());
    let where_ = |d: Device| if d == Device::Gpu { "the graphics card" } else { "the processor" };
    let model_name = crate::ai::local_llm::models::curated_by_id(local_model).map(|c| c.display).unwrap_or(local_model);
    let summary = match cleanup {
        CleanupPick::Local => format!(
            "Parakeet V3 on {}; AI cleanup on this PC with {model_name} (adds about {:.1} s a dictation)",
            where_(speech_device),
            local_ms as f32 / 1000.0
        ),
        CleanupPick::Cloud => format!(
            "Parakeet V3 on {}; AI cleanup by a cloud provider (about {:.1} s) — on this PC {model_name} adds about {:.1} s",
            where_(speech_device),
            CLOUD_MS as f32 / 1000.0,
            local_ms as f32 / 1000.0
        ),
    };
    Advice { tier, speech_model: SPEECH_MODEL, speech_device, cleanup, local_model, local_ms, cloud_ms: CLOUD_MS, summary }
}

/// What Onboarding and Settings show for this PC.
#[tauri::command]
pub fn hardware_advice() -> Advice {
    advise(&Machine::this_pc())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gpu(name: &str, mib: u64) -> Option<Gpu> {
        Some(Gpu { name: name.into(), vram_mib: mib, vendor: 0x10DE })
    }

    fn pc(gpu: Option<Gpu>, threads: u32, ram_gb: u64) -> Machine {
        Machine { gpu, threads, ram_mib: ram_gb * 1024 }
    }

    #[test]
    fn a_card_runs_speech_and_cleanup_and_the_4b_where_it_fits() {
        let a = advise(&pc(gpu("NVIDIA GeForce RTX 5070 Ti", 15_907), 16, 64));
        assert_eq!((a.tier, a.speech_device, a.cleanup, a.local_model), (Tier::Card, Device::Gpu, CleanupPick::Local, "qwen3.5-4b"));
        // A 4 GB card: still speech on the card, the 1.5B there.
        let a = advise(&pc(gpu("NVIDIA GeForce GTX 1650", 3_935), 8, 16));
        assert_eq!((a.tier, a.speech_device, a.local_model), (Tier::Card, Device::Gpu, "qwen2.5-1.5b"));
        assert_eq!(a.speech_model, "parakeet-tdt-0.6b-v3");
    }

    #[test]
    fn without_a_card_speech_starts_on_the_processor() {
        // Integrated graphics only, a desktop-class processor.
        let a = advise(&pc(gpu("Intel(R) Iris(R) Xe Graphics", 128), 12, 16));
        assert_eq!((a.tier, a.speech_device, a.cleanup, a.local_model), (Tier::Processor, Device::Cpu, CleanupPick::Local, "qwen2.5-1.5b"));
        // No graphics driver at all.
        assert_eq!(advise(&pc(None, 8, 8)).speech_device, Device::Cpu);
        assert_eq!(speech_device_before_measuring(None), Device::Cpu);
        assert_eq!(speech_device_before_measuring(gpu("AMD Radeon(TM) Graphics", 2_048).as_ref()), Device::Cpu);
        assert_eq!(speech_device_before_measuring(gpu("NVIDIA GeForce RTX 3060", 12_288).as_ref()), Device::Gpu);
    }

    #[test]
    fn a_small_pc_is_offered_the_cloud_first_with_both_times_said() {
        // A 4-thread laptop, and an 8-thread one with 4 GB.
        for m in [pc(gpu("Intel(R) UHD Graphics", 128), 4, 8), pc(None, 8, 4)] {
            let a = advise(&m);
            assert_eq!((a.tier, a.cleanup, a.speech_device), (Tier::Small, CleanupPick::Cloud, Device::Cpu), "{m:?}");
            assert!(a.local_ms > a.cloud_ms);
            assert!(a.summary.contains("cloud") && a.summary.contains("Qwen2.5 1.5B"), "{}", a.summary);
        }
    }
}
