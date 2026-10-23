//! Capturing every monitor at once, HDR right (Windows only).
//!
//! **Windows.Graphics.Capture** (WGC), one frame per monitor: a monitor
//! with HDR on is captured in **FP16** (scRGB, linear) and converted with
//! its own SDR white level ([`super::hdr`]); every other monitor in 8-bit
//! BGRA, which is exact there. The monitors' HDR state and white levels come
//! from DisplayConfig (`QueryDisplayConfig` + `DisplayConfigGetDeviceInfo`:
//! the source's GDI name, its advanced colour state, its SDR white level),
//! matched to `MONITORINFOEX.szDevice`. Frames are in physical pixels.
//!
//! The WGC single-frame pattern (a Direct3D 11 device with a WARP fallback,
//! `IGraphicsCaptureItemInterop::CreateForMonitor`, a free-threaded frame
//! pool of one, `FrameArrived` → `TryGetNextFrame`, a staging copy, Map, the
//! row pitch stripped) is ported from **robmikh/screenshot-rs** (MIT,
//! Copyright (c) 2020 Robert Mikhayelyan), with the borderless request and
//! the cursor switched off as **windows-capture** does (MIT, Copyright (c)
//! 2023 NiiightmareXD); notices in `THIRD-PARTY-NOTICES.md`. Unlike either,
//! Yap keeps one device warm on its own thread (`Worker`), starts every
//! monitor's session together, and tone-maps FP16 itself.
//!
//! **GDI fallback** (`BitBlt` of the monitor): when WGC can't (Windows'
//! "Let desktop apps take screenshots" privacy switch, a monitor it won't
//! give a frame for within [`FRAME_WAIT`]). SDR only: on an HDR monitor it's
//! the washed-out picture, and the result says so (`Method::Gdi`).
//!
//! What no API can capture: the secure desktop (UAC prompts, the lock
//! screen) — Yap's shortcut doesn't fire there anyway; DRM-protected video
//! and windows that ask to be left out (`WDA_EXCLUDEFROMCAPTURE`, some
//! password managers) come out black or missing, exactly as the frozen
//! picture shows them.

use std::time::{Duration, Instant};

/// How long a monitor's first frame may take before it's captured with GDI
/// instead.
pub const FRAME_WAIT: Duration = Duration::from_millis(800);

/// One monitor, in physical pixels (the virtual desktop's coordinates).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Monitor {
    /// The HMONITOR (0 for a test's fake monitor).
    #[serde(default)]
    pub handle: isize,
    /// Its GDI name ("\\.\DISPLAY1").
    #[serde(default)]
    pub device: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    /// Its scale (1.0 = 96 DPI, 1.5 = 150 %).
    pub scale: f32,
    #[serde(default)]
    pub primary: bool,
    /// HDR on (advanced colour, not just wide-colour management).
    #[serde(default)]
    pub hdr: bool,
    /// The SDR white level in nits (80 = no boost), when HDR is on.
    #[serde(default = "default_white")]
    pub white_nits: f32,
}

fn default_white() -> f32 {
    super::hdr::SCRGB_WHITE_NITS
}

/// How a frame was captured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Method {
    /// WGC in 8-bit BGRA (an SDR monitor).
    Wgc,
    /// WGC in FP16, converted with the monitor's SDR white level.
    WgcHdr,
    /// GDI `BitBlt`: SDR only.
    Gdi,
    /// A test's fake frame (`e2e.rs`): never the real screen.
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    Fake,
}

/// One monitor's frozen picture: BGRA8, tightly packed, physical pixels.
#[derive(Clone)]
pub struct Frame {
    pub monitor: Monitor,
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
    pub method: Method,
    /// Time from the request to this frame being read back (ms), when it
    /// arrived from Windows, and the FP16 conversion's share.
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub capture_ms: f64,
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub arrived_ms: f64,
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub convert_ms: f64,
}

impl std::fmt::Debug for Frame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never the pixels.
        f.debug_struct("Frame")
            .field("monitor", &self.monitor.device)
            .field("size", &(self.width, self.height))
            .field("method", &self.method)
            .finish()
    }
}

/// Timings of one capture of every monitor (the spike and the log).
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Timings {
    /// The Direct3D device (0 when it was warm).
    pub device_ms: f64,
    /// Sessions started → every frame read back and converted.
    pub frames_ms: f64,
    pub total_ms: f64,
    /// Whether Windows let Yap turn the yellow capture border off.
    pub borderless: Option<bool>,
}

/// The monitors now, in physical pixels, with their HDR state. Never fails:
/// a DisplayConfig error leaves every monitor SDR.
#[cfg(windows)]
pub fn monitors() -> Vec<Monitor> {
    win::monitors()
}

#[cfg(not(windows))]
pub fn monitors() -> Vec<Monitor> {
    Vec::new()
}

/// Capture `monitors` (WGC, GDI as the fallback), every one at once.
#[cfg(windows)]
pub fn capture(monitors: &[Monitor], force: Force) -> (Vec<Result<Frame, String>>, Timings) {
    win::worker().capture(monitors.to_vec(), force)
}

#[cfg(not(windows))]
pub fn capture(monitors: &[Monitor], _force: Force) -> (Vec<Result<Frame, String>>, Timings) {
    (monitors.iter().map(|_| Err("Screen capture is Windows only".to_string())).collect(), Timings::default())
}

/// Warm the capture device up in the background (the shortcut is bound):
/// the first snip then skips creating it.
#[cfg(windows)]
pub fn warm_up() {
    win::worker().warm();
}

#[cfg(not(windows))]
pub fn warm_up() {}

/// Which path to take (the spike compares them; a snip uses `Auto`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Force {
    /// FP16 on HDR monitors, BGRA elsewhere, GDI if WGC fails.
    Auto,
    /// WGC in 8-bit everywhere (what most tools do: clipped on HDR).
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    Wgc8,
    /// GDI everywhere.
    Gdi,
}

#[cfg(windows)]
pub(crate) mod win {
    use super::*;
    use std::sync::mpsc;
    use std::sync::OnceLock;

    use windows::core::{IInspectable, Interface};
    use windows::Foundation::TypedEventHandler;
    use windows::Graphics::Capture::{
        Direct3D11CaptureFrame, Direct3D11CaptureFramePool, GraphicsCaptureAccess, GraphicsCaptureAccessKind,
        GraphicsCaptureItem, GraphicsCaptureSession,
    };
    use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
    use windows::Graphics::DirectX::DirectXPixelFormat;
    use windows::Security::Authorization::AppCapabilityAccess::AppCapabilityAccessStatus;
    use windows::Win32::Devices::Display::{
        DisplayConfigGetDeviceInfo, GetDisplayConfigBufferSizes, QueryDisplayConfig, DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO,
        DISPLAYCONFIG_DEVICE_INFO_GET_SDR_WHITE_LEVEL, DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME, DISPLAYCONFIG_DEVICE_INFO_HEADER,
        DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO, DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_PATH_INFO, DISPLAYCONFIG_SDR_WHITE_LEVEL,
        DISPLAYCONFIG_SOURCE_DEVICE_NAME, QDC_ONLY_ACTIVE_PATHS,
    };
    use windows::Win32::Foundation::{HMODULE, LPARAM, RECT};
    use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE, D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP};
    use windows::Win32::Graphics::Direct3D11::{
        D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Resource, ID3D11Texture2D, D3D11_CPU_ACCESS_READ,
        D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC,
        D3D11_USAGE_STAGING,
    };
    use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_R16G16B16A16_FLOAT};
    use windows::Win32::Graphics::Dxgi::{IDXGIDevice, DXGI_ERROR_UNSUPPORTED};
    use windows::Win32::Graphics::Gdi::{
        BitBlt, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, EnumDisplayMonitors, GetDC, GetMonitorInfoW, ReleaseDC,
        SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CAPTUREBLT, DIB_RGB_COLORS, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW,
        SRCCOPY,
    };
    use windows::Win32::System::WinRT::Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess};
    use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
    use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};
    use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};

    // ---- monitors -------------------------------------------------------------------

    unsafe extern "system" fn each_monitor(handle: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> windows::core::BOOL {
        let list = &mut *(data.0 as *mut Vec<HMONITOR>);
        list.push(handle);
        true.into()
    }

    fn wide(s: &[u16]) -> String {
        let end = s.iter().position(|&c| c == 0).unwrap_or(s.len());
        String::from_utf16_lossy(&s[..end])
    }

    /// Every active display path's GDI name → (HDR on, SDR white nits).
    fn hdr_states() -> std::collections::HashMap<String, (bool, f32)> {
        let mut out = std::collections::HashMap::new();
        unsafe {
            let (mut paths, mut modes) = (0u32, 0u32);
            if GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut paths, &mut modes).is_err() {
                return out;
            }
            let mut path_list = vec![DISPLAYCONFIG_PATH_INFO::default(); paths as usize];
            let mut mode_list = vec![DISPLAYCONFIG_MODE_INFO::default(); modes as usize];
            if QueryDisplayConfig(QDC_ONLY_ACTIVE_PATHS, &mut paths, path_list.as_mut_ptr(), &mut modes, mode_list.as_mut_ptr(), None)
                .is_err()
            {
                return out;
            }
            path_list.truncate(paths as usize);
            for path in &path_list {
                let mut source = DISPLAYCONFIG_SOURCE_DEVICE_NAME {
                    header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                        r#type: DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
                        size: std::mem::size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32,
                        adapterId: path.sourceInfo.adapterId,
                        id: path.sourceInfo.id,
                    },
                    ..Default::default()
                };
                if DisplayConfigGetDeviceInfo(&mut source.header) != 0 {
                    continue;
                }
                let name = wide(&source.viewGdiDeviceName);
                let mut color = DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO {
                    header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                        r#type: DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO,
                        size: std::mem::size_of::<DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO>() as u32,
                        adapterId: path.targetInfo.adapterId,
                        id: path.targetInfo.id,
                    },
                    ..Default::default()
                };
                let hdr = DisplayConfigGetDeviceInfo(&mut color.header) == 0 && {
                    // Bit 1: advanced colour enabled; bit 2: wide colour
                    // enforced (auto colour management on an SDR display,
                    // where 1.0 is the display's white: no boost).
                    let bits = color.Anonymous.value;
                    bits & 0b10 != 0 && bits & 0b100 == 0
                };
                let mut white = DISPLAYCONFIG_SDR_WHITE_LEVEL {
                    header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                        r#type: DISPLAYCONFIG_DEVICE_INFO_GET_SDR_WHITE_LEVEL,
                        size: std::mem::size_of::<DISPLAYCONFIG_SDR_WHITE_LEVEL>() as u32,
                        adapterId: path.targetInfo.adapterId,
                        id: path.targetInfo.id,
                    },
                    SDRWhiteLevel: 1000,
                };
                // "A multiplier of 80 nits, multiplied by 1000".
                let nits = if DisplayConfigGetDeviceInfo(&mut white.header) == 0 && white.SDRWhiteLevel > 0 {
                    white.SDRWhiteLevel as f32 / 1000.0 * super::super::hdr::SCRGB_WHITE_NITS
                } else {
                    super::super::hdr::SCRGB_WHITE_NITS
                };
                out.insert(name, (hdr, nits));
            }
        }
        out
    }

    pub fn monitors() -> Vec<Monitor> {
        let mut handles: Vec<HMONITOR> = Vec::new();
        unsafe {
            let _ = EnumDisplayMonitors(None, None, Some(each_monitor), LPARAM(&mut handles as *mut _ as isize));
        }
        let states = hdr_states();
        let mut out = Vec::new();
        for handle in handles {
            let mut info = MONITORINFOEXW::default();
            info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
            if !unsafe { GetMonitorInfoW(handle, &mut info as *mut MONITORINFOEXW as *mut MONITORINFO) }.as_bool() {
                continue;
            }
            let r = info.monitorInfo.rcMonitor;
            let (mut dx, mut dy) = (96u32, 96u32);
            let dpi = if unsafe { GetDpiForMonitor(handle, MDT_EFFECTIVE_DPI, &mut dx, &mut dy) }.is_ok() { dx } else { 96 };
            let device = wide(&info.szDevice);
            let (hdr, white_nits) = states.get(&device).copied().unwrap_or((false, super::super::hdr::SCRGB_WHITE_NITS));
            out.push(Monitor {
                handle: handle.0 as isize,
                device,
                x: r.left,
                y: r.top,
                width: (r.right - r.left).max(0) as u32,
                height: (r.bottom - r.top).max(0) as u32,
                scale: dpi as f32 / 96.0,
                primary: info.monitorInfo.dwFlags & 1 != 0,
                hdr,
                white_nits,
            });
        }
        // The primary first, then left to right.
        out.sort_by_key(|m| (!m.primary, m.x, m.y));
        out
    }

    // ---- GDI ---------------------------------------------------------------------------

    /// The monitor's pixels with `BitBlt` (layered windows included).
    pub fn gdi_frame(m: &Monitor) -> Result<Vec<u8>, String> {
        let (w, h) = (m.width as i32, m.height as i32);
        if w <= 0 || h <= 0 {
            return Err("an empty monitor".into());
        }
        unsafe {
            let screen = GetDC(None);
            if screen.is_invalid() {
                return Err("no screen DC".into());
            }
            let mem = CreateCompatibleDC(Some(screen));
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w,
                    biHeight: -h, // top-down
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
            let result = (|| {
                let bitmap = CreateDIBSection(Some(mem), &info, DIB_RGB_COLORS, &mut bits, None, 0).map_err(|e| e.to_string())?;
                let old = SelectObject(mem, bitmap.into());
                let blit = BitBlt(mem, 0, 0, w, h, Some(screen), m.x, m.y, SRCCOPY | CAPTUREBLT);
                // (GDI leaves alpha at 0: nothing reads it — the overlay
                // copies colour, the crop writes 255.)
                let mut out = Vec::new();
                if blit.is_ok() && !bits.is_null() {
                    let len = (w * h * 4) as usize;
                    out.extend_from_slice(std::slice::from_raw_parts(bits as *const u8, len));
                }
                SelectObject(mem, old);
                let _ = DeleteObject(bitmap.into());
                blit.map_err(|e| e.to_string())?;
                Ok(out)
            })();
            let _ = DeleteDC(mem);
            ReleaseDC(None, screen);
            result
        }
    }

    // ---- WGC ---------------------------------------------------------------------------

    struct Gpu {
        device: ID3D11Device,
        context: ID3D11DeviceContext,
        winrt: IDirect3DDevice,
    }

    fn create_device(kind: D3D_DRIVER_TYPE) -> windows::core::Result<ID3D11Device> {
        let mut device = None;
        unsafe {
            D3D11CreateDevice(None, kind, HMODULE::default(), D3D11_CREATE_DEVICE_BGRA_SUPPORT, None, D3D11_SDK_VERSION, Some(&mut device), None, None)?;
        }
        device.ok_or_else(|| windows::core::Error::from(windows::Win32::Foundation::E_FAIL))
    }

    fn gpu() -> windows::core::Result<Gpu> {
        let device = match create_device(D3D_DRIVER_TYPE_HARDWARE) {
            Err(e) if e.code() == DXGI_ERROR_UNSUPPORTED => create_device(D3D_DRIVER_TYPE_WARP)?,
            other => other?,
        };
        let context = unsafe { device.GetImmediateContext()? };
        let dxgi: IDXGIDevice = device.cast()?;
        let winrt: IDirect3DDevice = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi)? }.cast()?;
        Ok(Gpu { device, context, winrt })
    }

    /// "Let me turn the yellow border off": Windows asks nothing of an
    /// unpackaged app, but without the request it may ignore
    /// `IsBorderRequired(false)` (Kiri's note). Asked once.
    fn borderless_allowed() -> bool {
        static ALLOWED: OnceLock<bool> = OnceLock::new();
        *ALLOWED.get_or_init(|| {
            let status = GraphicsCaptureAccess::RequestAccessAsync(GraphicsCaptureAccessKind::Borderless)
                .and_then(|op| op.get());
            match status {
                Ok(s) => {
                    tracing::info!(status = s.0, "snip: borderless capture request answered");
                    s == AppCapabilityAccessStatus::Allowed
                }
                Err(e) => {
                    tracing::info!("snip: borderless capture request failed ({e})");
                    false
                }
            }
        })
    }

    struct Pending {
        index: usize,
        hdr: bool,
        white: f32,
        pool: Direct3D11CaptureFramePool,
        session: GraphicsCaptureSession,
    }

    /// Start one monitor's capture session (its frame arrives on `tx`).
    fn start(gpu: &Gpu, index: usize, m: &Monitor, hdr: bool, tx: &mpsc::Sender<(usize, Instant)>, borderless: bool) -> windows::core::Result<Pending> {
        let interop = windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
        let item: GraphicsCaptureItem = unsafe { interop.CreateForMonitor(HMONITOR(m.handle as *mut _))? };
        let format = if hdr { DirectXPixelFormat::R16G16B16A16Float } else { DirectXPixelFormat::B8G8R8A8UIntNormalized };
        let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(&gpu.winrt, format, 1, item.Size()?)?;
        let session = pool.CreateCaptureSession(&item)?;
        let _ = session.SetIsCursorCaptureEnabled(false);
        if borderless {
            let _ = session.SetIsBorderRequired(false);
        }
        let tx = tx.clone();
        pool.FrameArrived(&TypedEventHandler::<Direct3D11CaptureFramePool, IInspectable>::new(move |_, _| {
            let _ = tx.send((index, Instant::now()));
            Ok(())
        }))?;
        session.StartCapture()?;
        Ok(Pending { index, hdr, white: m.white_nits, pool, session })
    }

    /// The frame's pixels: a staging copy, mapped, the pitch stripped.
    /// BGRA8 as it is; FP16 converted with the monitor's white level.
    fn read_back(gpu: &Gpu, frame: &Direct3D11CaptureFrame, hdr: bool, white: f32) -> windows::core::Result<(u32, u32, Vec<u8>, f64)> {
        unsafe {
            let access: IDirect3DDxgiInterfaceAccess = frame.Surface()?.cast()?;
            let texture: ID3D11Texture2D = access.GetInterface()?;
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            texture.GetDesc(&mut desc);
            desc.BindFlags = 0;
            desc.MiscFlags = 0;
            desc.Usage = D3D11_USAGE_STAGING;
            desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
            let mut staging = None;
            gpu.device.CreateTexture2D(&desc, None, Some(&mut staging))?;
            let staging = staging.ok_or_else(|| windows::core::Error::from(windows::Win32::Foundation::E_FAIL))?;
            let staging_res: ID3D11Resource = staging.cast()?;
            let source_res: ID3D11Resource = texture.cast()?;
            gpu.context.CopyResource(&staging_res, &source_res);
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            gpu.context.Map(&staging_res, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;
            let (w, h, pitch) = (desc.Width as usize, desc.Height as usize, mapped.RowPitch as usize);
            let mut convert_ms = 0.0;
            let out = if desc.Format == DXGI_FORMAT_R16G16B16A16_FLOAT && hdr {
                // Halves, `pitch / 2` apart.
                let halves = std::slice::from_raw_parts(mapped.pData as *const u16, pitch / 2 * h);
                let t = Instant::now();
                let out = super::super::hdr::cached(white).frame_to_bgra8(halves, pitch / 2, w, h);
                convert_ms = t.elapsed().as_secs_f64() * 1000.0;
                out
            } else if desc.Format == DXGI_FORMAT_B8G8R8A8_UNORM {
                let bytes = std::slice::from_raw_parts(mapped.pData as *const u8, pitch * h);
                let mut out = vec![0u8; w * h * 4];
                for (row, dst) in out.chunks_exact_mut(w * 4).enumerate() {
                    dst.copy_from_slice(&bytes[row * pitch..row * pitch + w * 4]);
                }
                out
            } else {
                gpu.context.Unmap(&staging_res, 0);
                return Err(windows::core::Error::new(windows::Win32::Foundation::E_INVALIDARG, "unexpected pixel format"));
            };
            gpu.context.Unmap(&staging_res, 0);
            Ok((desc.Width, desc.Height, out, convert_ms))
        }
    }

    // ---- the worker thread ---------------------------------------------------------------

    enum Job {
        Warm,
        Capture(Vec<Monitor>, Force, mpsc::Sender<(Vec<Result<Frame, String>>, Timings)>),
    }

    pub struct Worker {
        tx: std::sync::Mutex<mpsc::Sender<Job>>,
    }

    impl Worker {
        pub fn warm(&self) {
            if let Ok(tx) = self.tx.lock() {
                let _ = tx.send(Job::Warm);
            }
        }

        pub fn capture(&self, monitors: Vec<Monitor>, force: Force) -> (Vec<Result<Frame, String>>, Timings) {
            let (reply, answer) = mpsc::channel();
            let sent = self.tx.lock().map(|tx| tx.send(Job::Capture(monitors.clone(), force, reply)).is_ok()).unwrap_or(false);
            if sent {
                if let Ok(done) = answer.recv_timeout(Duration::from_secs(10)) {
                    return done;
                }
            }
            (monitors.iter().map(|_| Err("the capture thread didn't answer".to_string())).collect(), Timings::default())
        }
    }

    /// The capture thread: an MTA thread holding the Direct3D device, so a
    /// snip after the first doesn't create one again.
    pub fn worker() -> &'static Worker {
        static WORKER: OnceLock<Worker> = OnceLock::new();
        WORKER.get_or_init(|| {
            let (tx, rx) = mpsc::channel::<Job>();
            let spawned = std::thread::Builder::new().name("snip-capture".into()).spawn(move || {
                unsafe {
                    let _ = RoInitialize(RO_INIT_MULTITHREADED);
                }
                let mut gpu: Option<Gpu> = None;
                while let Ok(job) = rx.recv() {
                    match job {
                        Job::Warm => {
                            if gpu.is_none() {
                                let t = Instant::now();
                                gpu = self::gpu().map_err(|e| tracing::warn!("snip: no Direct3D device ({e})")).ok();
                                // Asked here too, off the shortcut's path.
                                let borderless = borderless_allowed();
                                tracing::info!(ms = t.elapsed().as_millis() as u64, borderless, "snip: capture device ready");
                            }
                        }
                        Job::Capture(monitors, force, reply) => {
                            let _ = reply.send(capture_now(&mut gpu, &monitors, force));
                        }
                    }
                }
            });
            if let Err(e) = spawned {
                tracing::warn!("snip: couldn't start the capture thread ({e})");
            }
            Worker { tx: std::sync::Mutex::new(tx) }
        })
    }

    fn capture_now(gpu_slot: &mut Option<Gpu>, monitors: &[Monitor], force: Force) -> (Vec<Result<Frame, String>>, Timings) {
        let began = Instant::now();
        let mut timings = Timings::default();
        let mut out: Vec<Option<Result<Frame, String>>> = monitors.iter().map(|_| None).collect();
        let gdi = |m: &Monitor, t0: Instant| -> Result<Frame, String> {
            let bgra = gdi_frame(m)?;
            Ok(Frame {
                monitor: m.clone(),
                width: m.width,
                height: m.height,
                bgra,
                method: Method::Gdi,
                capture_ms: t0.elapsed().as_secs_f64() * 1000.0,
                arrived_ms: 0.0,
                convert_ms: 0.0,
            })
        };
        if force != Force::Gdi {
            if gpu_slot.is_none() {
                let t = Instant::now();
                *gpu_slot = gpu().map_err(|e| tracing::warn!("snip: no Direct3D device ({e})")).ok();
                timings.device_ms = t.elapsed().as_secs_f64() * 1000.0;
            }
            if let Some(gpu) = gpu_slot.as_ref() {
                let borderless = borderless_allowed();
                timings.borderless = Some(borderless);
                let t0 = Instant::now();
                let (tx, rx) = mpsc::channel();
                let mut pending: Vec<Pending> = Vec::new();
                for (i, m) in monitors.iter().enumerate() {
                    let hdr = m.hdr && force == Force::Auto;
                    match start(gpu, i, m, hdr, &tx, borderless) {
                        Ok(p) => pending.push(p),
                        Err(e) => {
                            tracing::info!(monitor = i, "snip: WGC refused this monitor ({e}); GDI instead");
                        }
                    }
                }
                let deadline = Instant::now() + FRAME_WAIT;
                let mut waiting = pending.len();
                while waiting > 0 {
                    let left = deadline.saturating_duration_since(Instant::now());
                    let Ok((index, arrived)) = rx.recv_timeout(left) else { break };
                    let arrived_ms = arrived.saturating_duration_since(t0).as_secs_f64() * 1000.0;
                    let Some(p) = pending.iter().find(|p| p.index == index) else { continue };
                    if out[index].is_some() {
                        continue;
                    }
                    let Ok(frame) = p.pool.TryGetNextFrame() else { continue };
                    let m = &monitors[index];
                    let read = read_back(gpu, &frame, p.hdr, p.white);
                    let _ = frame.Close();
                    out[index] = Some(match read {
                        Ok((w, h, bgra, convert_ms)) => Ok(Frame {
                            monitor: m.clone(),
                            width: w,
                            height: h,
                            bgra,
                            method: if p.hdr { Method::WgcHdr } else { Method::Wgc },
                            capture_ms: t0.elapsed().as_secs_f64() * 1000.0,
                            arrived_ms,
                            convert_ms,
                        }),
                        Err(e) => Err(format!("couldn't read the frame back ({e})")),
                    });
                    waiting -= 1;
                }
                for p in &pending {
                    let _ = p.session.Close();
                    let _ = p.pool.Close();
                }
            }
        }
        // Whatever WGC didn't give: GDI.
        let results: Vec<Result<Frame, String>> = out
            .into_iter()
            .zip(monitors)
            .map(|(r, m)| match r {
                Some(Ok(f)) => Ok(f),
                Some(Err(e)) => {
                    tracing::info!("snip: {e}; GDI instead");
                    gdi(m, Instant::now())
                }
                None => gdi(m, Instant::now()),
            })
            .collect();
        timings.total_ms = began.elapsed().as_secs_f64() * 1000.0;
        timings.frames_ms = timings.total_ms - timings.device_ms;
        (results, timings)
    }
}
