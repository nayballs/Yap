//! **A stand-in desktop** (debug builds and unit tests only): displays and
//! windows from a scene (`e2e_computer_scene`), drawn in flat colours — so a
//! test run, a unit test or a Yap Lab run in test mode exercises computer
//! use whole (the card, the grants, masking, the frames, the displays)
//! without ever capturing the real screen. Modelled on the browser's
//! stand-in (`chat/pages/sim.rs`).
//!
//! Each window is drawn in its own colour with a darker title strip, the
//! desktop behind them in another; a window's `exe` and `aumid` say whose it
//! is, as a real window's do (`super::windows::Win`).

use std::sync::Mutex;

use serde::Deserialize;

use super::shot::{display_name, stand_in_monitor, Desk, Display};
use super::windows::Win;
use crate::snip::geom::Rect;

/// One display of a scene, in physical pixels.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct SimDisplay {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub primary: bool,
}

/// One window of a scene.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct SimWindow {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    /// Its exe's full path ("C:\\Windows\\System32\\notepad.exe").
    pub exe: String,
    pub aumid: String,
    pub class: String,
    /// One of Yap's own.
    pub own: bool,
}

/// **A desktop**: its displays, its windows topmost first, the pointer.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Scene {
    pub displays: Vec<SimDisplay>,
    pub windows: Vec<SimWindow>,
    pub cursor: Option<(i32, i32)>,
}

static SCENE: Mutex<Option<Scene>> = Mutex::new(None);

/// The scene set for this run, if any.
pub fn scene() -> Option<Scene> {
    SCENE.lock().unwrap_or_else(|p| p.into_inner()).clone()
}

/// Sets (or clears) the scene.
pub fn set(scene: Option<Scene>) {
    *SCENE.lock().unwrap_or_else(|p| p.into_inner()) = scene;
}

/// The desktop's colour, and each window's (in turn).
const DESKTOP: [u8; 3] = [24, 96, 120];
const COLOURS: &[[u8; 3]] = &[[236, 236, 230], [250, 214, 120], [150, 200, 240], [200, 150, 220], [160, 220, 160], [240, 160, 140]];

/// A window's colour (by its place from the bottom, so a window keeps its
/// colour whatever's put on top of it). Pure.
pub fn colour_of(from_bottom: usize) -> [u8; 3] {
    COLOURS[from_bottom % COLOURS.len()]
}

impl Desk for Scene {
    fn displays(&self) -> Vec<Display> {
        self.displays
            .iter()
            .enumerate()
            .map(|(i, d)| {
                let rect = Rect::new(d.x, d.y, d.w, d.h);
                Display { name: display_name("", i + 1), rect, primary: d.primary, monitor: stand_in_monitor(rect, d.primary, 1.0) }
            })
            .collect()
    }

    fn windows(&self) -> Vec<Win> {
        self.windows
            .iter()
            .enumerate()
            .map(|(i, w)| Win { hwnd: (i + 1) as isize, pid: if w.own { std::process::id() } else { 1000 + i as u32 }, rect: Rect::new(w.x, w.y, w.w, w.h), class: w.class.clone(), exe: w.exe.to_lowercase(), aumid: w.aumid.to_lowercase(), own: w.own })
            .collect()
    }

    fn capture(&self, d: &Display) -> Result<(Vec<u8>, u32, u32), String> {
        let (w, h) = (d.rect.w.max(1) as u32, d.rect.h.max(1) as u32);
        let mut rgba = vec![255u8; (w * h * 4) as usize];
        let paint = |rgba: &mut Vec<u8>, r: &Rect, c: [u8; 3]| {
            let local = Rect::new(r.x - d.rect.x, r.y - d.rect.y, r.w, r.h).intersect(&Rect::new(0, 0, w as i32, h as i32));
            for y in local.y..local.bottom() {
                for x in local.x..local.right() {
                    let i = (y as usize * w as usize + x as usize) * 4;
                    rgba[i..i + 3].copy_from_slice(&c);
                }
            }
        };
        paint(&mut rgba, &d.rect, DESKTOP);
        // Bottom to top: each window, then its title strip.
        for (n, win) in self.windows.iter().rev().enumerate() {
            let r = Rect::new(win.x, win.y, win.w, win.h);
            let c = colour_of(n);
            paint(&mut rgba, &r, c);
            paint(&mut rgba, &Rect::new(r.x, r.y, r.w, 24.min(r.h)), c.map(|v| v / 2));
        }
        Ok((rgba, w, h))
    }

    fn cursor(&self) -> Option<(i32, i32)> {
        self.cursor
    }
}

/// **Test mode: the stand-in desktop** computer use sees from now on
/// (`None`: none, so a screenshot in a test run is refused). Never in robot
/// mode, which drives the real one's test windows.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_computer_scene(scene: Option<Scene>) -> Result<(), String> {
    if !crate::shell::e2e::active() || crate::shell::realtest::on() {
        return Err("e2e_computer_scene is for test runs (not robot mode)".into());
    }
    set(scene);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scene_draws_its_windows_over_its_desktop() {
        let scene = Scene {
            displays: vec![SimDisplay { x: 0, y: 0, w: 200, h: 100, primary: true }, SimDisplay { x: 200, y: 0, w: 100, h: 100, primary: false }],
            windows: vec![SimWindow { x: 10, y: 10, w: 50, h: 50, exe: r"C:\Windows\notepad.exe".into(), ..Default::default() }, SimWindow { x: 40, y: 40, w: 200, h: 50, own: true, ..Default::default() }],
            cursor: Some((5, 5)),
        };
        let ds = scene.displays();
        assert_eq!((ds[0].name.as_str(), ds[1].name.as_str()), ("Display 1", "Display 2"));
        let ws = scene.windows();
        assert_eq!(ws[0].exe, r"c:\windows\notepad.exe");
        assert!(ws[1].own && !ws[0].own);
        let (rgba, w, h) = scene.capture(&ds[0]).unwrap();
        assert_eq!((w, h), (200, 100));
        let px = |x: usize, y: usize| [rgba[(y * 200 + x) * 4], rgba[(y * 200 + x) * 4 + 1], rgba[(y * 200 + x) * 4 + 2]];
        assert_eq!(px(5, 95), DESKTOP);
        assert_eq!(px(20, 38), colour_of(1), "the top window, below its title strip");
        assert_eq!(px(150, 70), colour_of(0), "the bottom window");
        // The second display: the bottom window's other end.
        let (rgba2, _, _) = scene.capture(&ds[1]).unwrap();
        assert_eq!([rgba2[(70 * 100 + 10) * 4], rgba2[(70 * 100 + 10) * 4 + 1], rgba2[(70 * 100 + 10) * 4 + 2]], colour_of(0));
    }
}
