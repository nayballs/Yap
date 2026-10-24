//! What `yap.exe bench` knows about the PC (the processor's name, cores and
//! threads, memory, graphics adapters) and the processor limit it emulates a
//! smaller PC with: **process affinity** to N logical processors, one per
//! physical core while there are cores to go round (a 4-thread laptop
//! without its siblings' shared execution units), inherited by the
//! llamafile server it starts. Read only; the affinity is this process's
//! own and put back when the [`Limit`] drops.

use serde::Serialize;

/// The PC, as the report names it. Only the class of the hardware: no
/// serial numbers, driver versions or names of anything else.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Hardware {
    pub cpu: String,
    pub cores: u32,
    pub threads: u32,
    /// Memory, rounded to whole GB.
    pub ram_gb: u32,
    pub adapters: Vec<crate::gpu::Adapter>,
}

impl Hardware {
    pub fn this_pc() -> Hardware {
        let (cores, threads) = core_count();
        Hardware { cpu: cpu_name(), cores, threads, ram_gb: ram_gb(), adapters: crate::gpu::all_adapters() }
    }

    /// The hardware adapters (no software one).
    pub fn gpus(&self) -> impl Iterator<Item = &crate::gpu::Adapter> {
        self.adapters.iter().filter(|a| !a.software)
    }
}

/// The physical cores' logical-processor masks, in Windows' order.
#[cfg(windows)]
fn core_masks() -> Vec<usize> {
    use windows::Win32::System::SystemInformation::{
        GetLogicalProcessorInformation, RelationProcessorCore, SYSTEM_LOGICAL_PROCESSOR_INFORMATION,
    };
    let mut len = 0u32;
    // SAFETY: the first call only asks for the size; the second fills a
    // buffer of that many bytes.
    unsafe {
        let _ = GetLogicalProcessorInformation(None, &mut len);
        let n = len as usize / std::mem::size_of::<SYSTEM_LOGICAL_PROCESSOR_INFORMATION>();
        if n == 0 {
            return Vec::new();
        }
        let mut buf = vec![SYSTEM_LOGICAL_PROCESSOR_INFORMATION::default(); n];
        if GetLogicalProcessorInformation(Some(buf.as_mut_ptr()), &mut len).is_err() {
            return Vec::new();
        }
        buf.iter().filter(|i| i.Relationship == RelationProcessorCore).map(|i| i.ProcessorMask).collect()
    }
}

#[cfg(not(windows))]
fn core_masks() -> Vec<usize> {
    Vec::new()
}

/// (physical cores, logical processors).
fn core_count() -> (u32, u32) {
    let masks = core_masks();
    let threads: u32 = masks.iter().map(|m| m.count_ones()).sum();
    if masks.is_empty() {
        let n = std::thread::available_parallelism().map(|n| n.get() as u32).unwrap_or(1);
        return (n, n);
    }
    (masks.len() as u32, threads)
}

/// The affinity mask for `threads` logical processors: one per physical
/// core first (`cores`: each core's mask), then their siblings. Pure.
pub fn mask_for(cores: &[usize], threads: u32) -> usize {
    let mut mask = 0usize;
    let mut left = threads;
    // Round 1: the lowest processor of each core; round 2: the next…
    for round in 0..usize::BITS {
        for core in cores {
            if left == 0 {
                return mask;
            }
            // The core's `round`-th set bit.
            let mut bits = *core;
            for _ in 0..round {
                bits &= bits.wrapping_sub(1);
            }
            if bits != 0 {
                mask |= bits.isolate_lowest_one();
                left -= 1;
            }
        }
        if cores.iter().all(|c| c.count_ones() <= round + 1) {
            break;
        }
    }
    mask
}

/// This process held to N logical processors until it drops (then as it was).
pub struct Limit {
    #[cfg_attr(not(windows), allow(dead_code))]
    before: Option<usize>,
}

impl Limit {
    /// Hold the process to `threads` logical processors (`None`: as it is).
    pub fn new(threads: Option<u32>) -> Limit {
        let Some(n) = threads else { return Limit { before: None } };
        Limit { before: set_affinity(mask_for(&core_masks(), n)) }
    }
}

impl Drop for Limit {
    fn drop(&mut self) {
        #[cfg(windows)]
        if let Some(mask) = self.before {
            let _ = set_affinity(mask);
        }
    }
}

/// Set this process's affinity mask; the one it had.
#[cfg(windows)]
fn set_affinity(mask: usize) -> Option<usize> {
    use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessAffinityMask, SetProcessAffinityMask};
    if mask == 0 {
        return None;
    }
    let (mut process, mut system) = (0usize, 0usize);
    // SAFETY: the current process's pseudo-handle and two out-pointers.
    unsafe {
        let me = GetCurrentProcess();
        GetProcessAffinityMask(me, &mut process, &mut system).ok()?;
        SetProcessAffinityMask(me, mask).ok()?;
    }
    Some(process)
}

#[cfg(not(windows))]
fn set_affinity(_mask: usize) -> Option<usize> {
    None
}

/// The processor's name ("AMD Ryzen 7 9800X3D 8-Core Processor").
fn cpu_name() -> String {
    #[cfg(windows)]
    {
        use windows::core::HSTRING;
        use windows::Win32::Foundation::ERROR_SUCCESS;
        use windows::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};
        let mut buf = [0u16; 256];
        let mut size = std::mem::size_of_val(&buf) as u32;
        // SAFETY: a value read into a local buffer of `size` bytes.
        let r = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                &HSTRING::from(r"HARDWARE\DESCRIPTION\System\CentralProcessor\0"),
                &HSTRING::from("ProcessorNameString"),
                RRF_RT_REG_SZ,
                None,
                Some(buf.as_mut_ptr() as *mut core::ffi::c_void),
                Some(&mut size),
            )
        };
        if r == ERROR_SUCCESS {
            let units = (size as usize / 2).min(buf.len());
            return String::from_utf16_lossy(&buf[..units]).trim_end_matches('\0').trim().to_string();
        }
    }
    "unknown processor".into()
}

/// The PC's memory in whole GB.
fn ram_gb() -> u32 {
    #[cfg(windows)]
    {
        use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
        let mut m = MEMORYSTATUSEX { dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
        // SAFETY: a sized struct this function owns.
        if unsafe { GlobalMemoryStatusEx(&mut m) }.is_ok() {
            return (m.ullTotalPhys as f64 / 1_073_741_824.0).round() as u32;
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_thread_limit_takes_one_processor_per_core_before_their_siblings() {
        // 8 cores with two threads each (0+1, 2+3, …), as Windows numbers an SMT CPU.
        let cores: Vec<usize> = (0..8).map(|c| 0b11 << (c * 2)).collect();
        assert_eq!(mask_for(&cores, 4), 0b01010101);
        assert_eq!(mask_for(&cores, 2), 0b0101);
        assert_eq!(mask_for(&cores, 8), 0x5555);
        // More threads than cores: the siblings next, in core order.
        assert_eq!(mask_for(&cores, 10), 0x5555 | 0b1010);
        assert_eq!(mask_for(&cores, 16), 0xFFFF);
        assert_eq!(mask_for(&cores, 99), 0xFFFF);
        // No SMT: one processor a core.
        let plain: Vec<usize> = (0..4).map(|c| 1 << c).collect();
        assert_eq!(mask_for(&plain, 2), 0b11);
        assert_eq!(mask_for(&plain, 6), 0b1111);
        assert_eq!(mask_for(&[], 4), 0);
    }
}
