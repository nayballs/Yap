//! **A stand-in desktop** (debug builds and unit tests only): displays and
//! windows from a scene (`e2e_computer_scene`), drawn by Yap itself — so a
//! test run, a unit test or a Yap Lab run in test mode exercises computer
//! use whole (the card, the grants, masking, the frames, clicks, keys,
//! typing, the checks) without ever touching the real screen, mouse or
//! keyboard. Modelled on the browser's stand-in (`chat/pages/sim.rs`).
//!
//! - **Windows** in their own colours, a title strip with their title, the
//!   desktop behind them; a window's `exe` and `aumid` say whose it is, as a
//!   real window's do (`super::windows::Win`); `elevated` pretends one runs
//!   as administrator.
//! - **Widgets** in a window: buttons, text fields (a password field's text
//!   drawn as dots), links and plain text, drawn with a small built-in font
//!   so a model can read them; UI Automation's stand-in reads their kind and
//!   name ([`super::uia::Element`]).
//! - **Input changes it** ([`SimDesk`] as [`super::input::Hands`]): a click
//!   brings its window to the front and focuses a field, presses a button
//!   or opens a link (each written in the scene's `log`); typing goes into
//!   the focused field; Backspace takes a character back; keys and scrolls
//!   are logged. `e2e_computer_state` reads it all back.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use super::input::{Button, Hands, Step};
use super::shot::{display_name, stand_in_monitor, Desk, Display};
use super::uia::{Element, Kind};
use super::windows::Win;
use crate::snip::geom::Rect;

/// One display of a scene, in physical pixels.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct SimDisplay {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub primary: bool,
}

/// One thing in a window (its position relative to the window).
#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Widget {
    /// "button" | "field" | "password" | "link" | "text".
    pub kind: String,
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    /// A field's text, a text's words.
    pub text: String,
}

/// One window of a scene.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
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
    pub title: String,
    /// One of Yap's own.
    pub own: bool,
    /// Pretends to run as administrator.
    pub elevated: bool,
    pub widgets: Vec<Widget>,
}

/// **A desktop**: its displays, its windows topmost first (the first is in
/// front), the pointer, the focused field, and what input did to it.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Scene {
    pub displays: Vec<SimDisplay>,
    pub windows: Vec<SimWindow>,
    pub cursor: Option<(i32, i32)>,
    /// The focused widget: its window's place in `windows` and its own.
    pub focus: Option<(usize, usize)>,
    /// What input did, in order ("pressed Send", "typed into Message"…).
    pub log: Vec<String>,
    /// The keys held now.
    #[serde(skip)]
    pub held: Vec<String>,
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
const INK: [u8; 3] = [24, 24, 28];
const LINK: [u8; 3] = [20, 70, 200];

/// A window's colour (by its place from the bottom, so a window keeps its
/// colour whatever's put on top of it). Pure.
pub fn colour_of(from_bottom: usize) -> [u8; 3] {
    COLOURS[from_bottom % COLOURS.len()]
}

impl Widget {
    fn rect_in(&self, win: &SimWindow) -> Rect {
        Rect::new(win.x + self.x, win.y + self.y, self.w, self.h)
    }
    fn element(&self) -> Element {
        let kind = match self.kind.as_str() {
            "link" => Kind::Link,
            "button" => Kind::Control,
            "field" | "password" => Kind::Field,
            _ => Kind::Other,
        };
        Element { kind, name: self.name.clone(), password: self.kind == "password" }
    }
}

fn contains(r: &Rect, x: i32, y: i32) -> bool {
    x >= r.x && y >= r.y && x < r.right() && y < r.bottom()
}

impl Scene {
    /// The window under a point: its place in `windows`.
    fn index_at(&self, x: i32, y: i32) -> Option<usize> {
        self.windows.iter().position(|w| contains(&Rect::new(w.x, w.y, w.w, w.h), x, y))
    }

    fn win(&self, i: usize) -> Win {
        let w = &self.windows[i];
        Win { hwnd: (i + 1) as isize, pid: if w.own { std::process::id() } else { 1000 + i as u32 }, rect: Rect::new(w.x, w.y, w.w, w.h), class: w.class.clone(), exe: w.exe.to_lowercase(), aumid: w.aumid.to_lowercase(), own: w.own }
    }

    /// The widget under a point in window `i`: its place.
    fn widget_at(&self, i: usize, x: i32, y: i32) -> Option<usize> {
        let w = &self.windows[i];
        w.widgets.iter().position(|g| contains(&g.rect_in(w), x, y))
    }

    /// Window `i` to the front (its focus kept with it).
    fn raise(&mut self, i: usize) {
        if i == 0 {
            return;
        }
        let w = self.windows.remove(i);
        self.windows.insert(0, w);
        self.focus = match self.focus {
            Some((f, g)) if f == i => Some((0, g)),
            Some((f, g)) if f < i => Some((f + 1, g)),
            other => other,
        };
    }

    /// **One step of input** applied.
    pub fn apply(&mut self, step: &Step) {
        match step {
            Step::Move(x, y) => self.cursor = Some((*x, *y)),
            Step::Down(button) => {
                let Some((x, y)) = self.cursor else { return };
                let Some(i) = self.index_at(x, y) else { return };
                self.raise(i);
                let widget = self.widget_at(0, x, y);
                let name = |s: &Scene, g: usize| s.windows[0].widgets[g].name.clone();
                match (button, widget) {
                    (Button::Left, Some(g)) => match self.windows[0].widgets[g].kind.as_str() {
                        "field" | "password" => {
                            self.focus = Some((0, g));
                            self.log.push(format!("focused {}", name(self, g)));
                        }
                        "button" => self.log.push(format!("pressed {}", name(self, g))),
                        "link" => self.log.push(format!("opened {}", name(self, g))),
                        _ => self.log.push(format!("clicked {}", name(self, g))),
                    },
                    (Button::Left, None) => self.log.push(format!("clicked {}", self.windows[0].title)),
                    (Button::Right, g) => self.log.push(format!("right-clicked {}", g.map(|g| name(self, g)).unwrap_or_else(|| self.windows[0].title.clone()))),
                    (Button::Middle, g) => self.log.push(format!("middle-clicked {}", g.map(|g| name(self, g)).unwrap_or_else(|| self.windows[0].title.clone()))),
                }
            }
            Step::Up(_) | Step::Pause(_) => {}
            Step::Wheel { ticks, across } => self.log.push(format!("scrolled {} {}", if *across { "across" } else { "down" }, -ticks)),
            Step::KeyDown(k) => {
                if super::input::is_modifier(k) {
                    if !self.held.contains(k) {
                        self.held.push(k.clone());
                    }
                    return;
                }
                let combo = self.held.iter().chain(std::iter::once(k)).cloned().collect::<Vec<_>>().join("+");
                match (k.as_str(), self.focus) {
                    ("backspace", Some((w, g))) if self.held.is_empty() => {
                        self.windows[w].widgets[g].text.pop();
                    }
                    ("return", Some((w, g))) if self.held.is_empty() => self.log.push(format!("enter in {}", self.windows[w].widgets[g].name)),
                    _ => self.log.push(format!("key {combo}")),
                }
            }
            Step::KeyUp(k) => self.held.retain(|h| h != k),
            Step::Text(t) => match self.focus {
                Some((w, g)) => {
                    self.windows[w].widgets[g].text.push_str(t);
                    let n = &self.windows[w].widgets[g].name;
                    self.log.push(format!("typed into {n}"));
                }
                None => self.log.push("typed with nothing focused".into()),
            },
        }
    }
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
        (0..self.windows.len()).map(|i| self.win(i)).collect()
    }

    fn capture(&self, d: &Display) -> Result<(Vec<u8>, u32, u32), String> {
        let (w, h) = (d.rect.w.max(1) as u32, d.rect.h.max(1) as u32);
        let mut rgba = vec![255u8; (w * h * 4) as usize];
        let local = |r: &Rect| Rect::new(r.x - d.rect.x, r.y - d.rect.y, r.w, r.h);
        paint(&mut rgba, w, h, &local(&d.rect), DESKTOP);
        // Bottom to top: each window, its title strip, its widgets.
        for (n, win) in self.windows.iter().rev().enumerate() {
            let r = local(&Rect::new(win.x, win.y, win.w, win.h));
            let c = colour_of(n);
            paint(&mut rgba, w, h, &r, c);
            paint(&mut rgba, w, h, &Rect::new(r.x, r.y, r.w, 24.min(r.h)), c.map(|v| v / 2));
            text(&mut rgba, w, h, r.x + 8, r.y + 5, &win.title, [250, 250, 250]);
            for g in &win.widgets {
                let gr = local(&g.rect_in(win));
                match g.kind.as_str() {
                    "button" => {
                        paint(&mut rgba, w, h, &gr, [70, 70, 78]);
                        paint(&mut rgba, w, h, &gr.inflate(-2), [214, 214, 220]);
                        text(&mut rgba, w, h, gr.x + 8, gr.y + (gr.h - 14) / 2, &g.name, INK);
                    }
                    "field" | "password" => {
                        paint(&mut rgba, w, h, &gr, [90, 90, 98]);
                        paint(&mut rgba, w, h, &gr.inflate(-2), [255, 255, 255]);
                        let shown = if g.kind == "password" { "*".repeat(g.text.chars().count()) } else { g.text.clone() };
                        let words = if shown.is_empty() { g.name.clone() } else { shown };
                        text(&mut rgba, w, h, gr.x + 6, gr.y + (gr.h - 14) / 2, &words, if g.text.is_empty() { [140, 140, 150] } else { INK });
                    }
                    "link" => {
                        text(&mut rgba, w, h, gr.x, gr.y, &g.name, LINK);
                        paint(&mut rgba, w, h, &Rect::new(gr.x, gr.y + 15, (g.name.chars().count() as i32) * 12, 2), LINK);
                    }
                    _ => text(&mut rgba, w, h, gr.x, gr.y, if g.text.is_empty() { &g.name } else { &g.text }, INK),
                }
            }
        }
        Ok((rgba, w, h))
    }

    fn cursor(&self) -> Option<(i32, i32)> {
        self.cursor
    }

    fn front(&self) -> Option<Win> {
        (!self.windows.is_empty()).then(|| self.win(0))
    }

    fn window_at(&self, x: i32, y: i32) -> Option<Win> {
        self.index_at(x, y).map(|i| self.win(i))
    }

    fn element_at(&self, x: i32, y: i32) -> Option<Element> {
        let i = self.index_at(x, y)?;
        self.widget_at(i, x, y).map(|g| self.windows[i].widgets[g].element())
    }

    fn focused(&self) -> Option<Element> {
        self.focus.map(|(w, g)| self.windows[w].widgets[g].element())
    }

    fn elevated(&self, w: &Win) -> bool {
        let i = (w.hwnd - 1) as usize;
        self.windows.get(i).is_some_and(|s| s.elevated)
    }
}

/// **The stand-in desktop as it is now** (the scene set for the run): what
/// computer use sees, and where its input goes.
pub struct SimDesk;

fn with<R>(f: impl FnOnce(&mut Scene) -> R) -> Option<R> {
    SCENE.lock().unwrap_or_else(|p| p.into_inner()).as_mut().map(f)
}

impl Desk for SimDesk {
    fn displays(&self) -> Vec<Display> {
        with(|s| s.displays()).unwrap_or_default()
    }
    fn windows(&self) -> Vec<Win> {
        with(|s| s.windows()).unwrap_or_default()
    }
    fn capture(&self, d: &Display) -> Result<(Vec<u8>, u32, u32), String> {
        with(|s| s.capture(d)).unwrap_or_else(|| Err("no stand-in desktop".into()))
    }
    fn cursor(&self) -> Option<(i32, i32)> {
        with(|s| s.cursor).flatten()
    }
    fn front(&self) -> Option<Win> {
        with(|s| s.front()).flatten()
    }
    fn window_at(&self, x: i32, y: i32) -> Option<Win> {
        with(|s| s.window_at(x, y)).flatten()
    }
    fn element_at(&self, x: i32, y: i32) -> Option<Element> {
        with(|s| s.element_at(x, y)).flatten()
    }
    fn focused(&self) -> Option<Element> {
        with(|s| s.focused()).flatten()
    }
    fn elevated(&self, w: &Win) -> bool {
        with(|s| s.elevated(w)).unwrap_or(false)
    }
}

impl Hands for SimDesk {
    fn send(&self, step: &Step) -> Result<(), String> {
        with(|s| s.apply(step)).ok_or_else(|| "no stand-in desktop".to_string())
    }
}

fn paint(rgba: &mut [u8], w: u32, h: u32, r: &Rect, c: [u8; 3]) {
    let r = r.intersect(&Rect::new(0, 0, w as i32, h as i32));
    for y in r.y..r.bottom() {
        for x in r.x..r.right() {
            let i = (y as usize * w as usize + x as usize) * 4;
            rgba[i..i + 3].copy_from_slice(&c);
        }
    }
}

/// A 5 × 7 font, ASCII 32–126: five columns a glyph, bit 0 the top row.
const FONT: &[[u8; 5]] = &[
    [0x00, 0x00, 0x00, 0x00, 0x00], [0x00, 0x00, 0x5F, 0x00, 0x00], [0x00, 0x07, 0x00, 0x07, 0x00], [0x14, 0x7F, 0x14, 0x7F, 0x14], [0x24, 0x2A, 0x7F, 0x2A, 0x12], [0x23, 0x13, 0x08, 0x64, 0x62],
    [0x36, 0x49, 0x55, 0x22, 0x50], [0x00, 0x05, 0x03, 0x00, 0x00], [0x00, 0x1C, 0x22, 0x41, 0x00], [0x00, 0x41, 0x22, 0x1C, 0x00], [0x08, 0x2A, 0x1C, 0x2A, 0x08], [0x08, 0x08, 0x3E, 0x08, 0x08],
    [0x00, 0x50, 0x30, 0x00, 0x00], [0x08, 0x08, 0x08, 0x08, 0x08], [0x00, 0x60, 0x60, 0x00, 0x00], [0x20, 0x10, 0x08, 0x04, 0x02], [0x3E, 0x51, 0x49, 0x45, 0x3E], [0x00, 0x42, 0x7F, 0x40, 0x00],
    [0x42, 0x61, 0x51, 0x49, 0x46], [0x21, 0x41, 0x45, 0x4B, 0x31], [0x18, 0x14, 0x12, 0x7F, 0x10], [0x27, 0x45, 0x45, 0x45, 0x39], [0x3C, 0x4A, 0x49, 0x49, 0x30], [0x01, 0x71, 0x09, 0x05, 0x03],
    [0x36, 0x49, 0x49, 0x49, 0x36], [0x06, 0x49, 0x49, 0x29, 0x1E], [0x00, 0x36, 0x36, 0x00, 0x00], [0x00, 0x56, 0x36, 0x00, 0x00], [0x08, 0x14, 0x22, 0x41, 0x00], [0x14, 0x14, 0x14, 0x14, 0x14],
    [0x00, 0x41, 0x22, 0x14, 0x08], [0x02, 0x01, 0x51, 0x09, 0x06], [0x32, 0x49, 0x79, 0x41, 0x3E], [0x7E, 0x11, 0x11, 0x11, 0x7E], [0x7F, 0x49, 0x49, 0x49, 0x36], [0x3E, 0x41, 0x41, 0x41, 0x22],
    [0x7F, 0x41, 0x41, 0x22, 0x1C], [0x7F, 0x49, 0x49, 0x49, 0x41], [0x7F, 0x09, 0x09, 0x09, 0x01], [0x3E, 0x41, 0x49, 0x49, 0x7A], [0x7F, 0x08, 0x08, 0x08, 0x7F], [0x00, 0x41, 0x7F, 0x41, 0x00],
    [0x20, 0x40, 0x41, 0x3F, 0x01], [0x7F, 0x08, 0x14, 0x22, 0x41], [0x7F, 0x40, 0x40, 0x40, 0x40], [0x7F, 0x02, 0x0C, 0x02, 0x7F], [0x7F, 0x04, 0x08, 0x10, 0x7F], [0x3E, 0x41, 0x41, 0x41, 0x3E],
    [0x7F, 0x09, 0x09, 0x09, 0x06], [0x3E, 0x41, 0x51, 0x21, 0x5E], [0x7F, 0x09, 0x19, 0x29, 0x46], [0x46, 0x49, 0x49, 0x49, 0x31], [0x01, 0x01, 0x7F, 0x01, 0x01], [0x3F, 0x40, 0x40, 0x40, 0x3F],
    [0x1F, 0x20, 0x40, 0x20, 0x1F], [0x3F, 0x40, 0x38, 0x40, 0x3F], [0x63, 0x14, 0x08, 0x14, 0x63], [0x07, 0x08, 0x70, 0x08, 0x07], [0x61, 0x51, 0x49, 0x45, 0x43], [0x00, 0x7F, 0x41, 0x41, 0x00],
    [0x02, 0x04, 0x08, 0x10, 0x20], [0x00, 0x41, 0x41, 0x7F, 0x00], [0x04, 0x02, 0x01, 0x02, 0x04], [0x40, 0x40, 0x40, 0x40, 0x40], [0x00, 0x01, 0x02, 0x04, 0x00], [0x20, 0x54, 0x54, 0x54, 0x78],
    [0x7F, 0x48, 0x44, 0x44, 0x38], [0x38, 0x44, 0x44, 0x44, 0x20], [0x38, 0x44, 0x44, 0x48, 0x7F], [0x38, 0x54, 0x54, 0x54, 0x18], [0x08, 0x7E, 0x09, 0x01, 0x02], [0x0C, 0x52, 0x52, 0x52, 0x3E],
    [0x7F, 0x08, 0x04, 0x04, 0x78], [0x00, 0x44, 0x7D, 0x40, 0x00], [0x20, 0x40, 0x44, 0x3D, 0x00], [0x7F, 0x10, 0x28, 0x44, 0x00], [0x00, 0x41, 0x7F, 0x40, 0x00], [0x7C, 0x04, 0x18, 0x04, 0x78],
    [0x7C, 0x08, 0x04, 0x04, 0x78], [0x38, 0x44, 0x44, 0x44, 0x38], [0x7C, 0x14, 0x14, 0x14, 0x08], [0x08, 0x14, 0x14, 0x18, 0x7C], [0x7C, 0x08, 0x04, 0x04, 0x08], [0x48, 0x54, 0x54, 0x54, 0x20],
    [0x04, 0x3F, 0x44, 0x40, 0x20], [0x3C, 0x40, 0x40, 0x20, 0x7C], [0x1C, 0x20, 0x40, 0x20, 0x1C], [0x3C, 0x40, 0x30, 0x40, 0x3C], [0x44, 0x28, 0x10, 0x28, 0x44], [0x0C, 0x50, 0x50, 0x50, 0x3C],
    [0x44, 0x64, 0x54, 0x4C, 0x44], [0x00, 0x08, 0x36, 0x41, 0x00], [0x00, 0x00, 0x7F, 0x00, 0x00], [0x00, 0x41, 0x36, 0x08, 0x00], [0x08, 0x04, 0x08, 0x10, 0x08],
];

/// `s` drawn at (`x`, `y`), twice the font's size (glyphs 12 px apart).
fn text(rgba: &mut [u8], w: u32, h: u32, x: i32, y: i32, s: &str, c: [u8; 3]) {
    for (n, ch) in s.chars().enumerate() {
        let code = ch as u32;
        let glyph = if (32..=126).contains(&code) { &FONT[(code - 32) as usize] } else { &FONT[('?' as u32 - 32) as usize] };
        let gx = x + n as i32 * 12;
        for (col, bits) in glyph.iter().enumerate() {
            for row in 0..7 {
                if bits & (1 << row) != 0 {
                    paint(rgba, w, h, &Rect::new(gx + col as i32 * 2, y + row * 2, 2, 2), c);
                }
            }
        }
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
    // A new desktop: nobody holds it, nothing was stopped on it.
    super::watch::reset();
    Ok(())
}

/// **Test mode: the stand-in desktop as input left it** (its windows in
/// order, their fields' text, the focus, the log).
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_computer_state() -> Result<Option<Scene>, String> {
    if !crate::shell::e2e::active() {
        return Err("e2e_computer_state is for test runs".into());
    }
    Ok(scene())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pad() -> SimWindow {
        SimWindow {
            x: 10,
            y: 10,
            w: 300,
            h: 200,
            exe: r"C:\Windows\notepad.exe".into(),
            title: "Pad".into(),
            widgets: vec![
                Widget { kind: "field".into(), name: "Message".into(), x: 10, y: 40, w: 200, h: 30, ..Default::default() },
                Widget { kind: "button".into(), name: "Send".into(), x: 10, y: 90, w: 80, h: 30, ..Default::default() },
                Widget { kind: "password".into(), name: "PIN".into(), x: 10, y: 140, w: 120, h: 30, ..Default::default() },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn a_scene_draws_its_windows_over_its_desktop() {
        let scene = Scene {
            displays: vec![SimDisplay { x: 0, y: 0, w: 200, h: 100, primary: true }, SimDisplay { x: 200, y: 0, w: 100, h: 100, primary: false }],
            windows: vec![SimWindow { x: 10, y: 10, w: 50, h: 50, exe: r"C:\Windows\notepad.exe".into(), ..Default::default() }, SimWindow { x: 40, y: 40, w: 200, h: 50, own: true, ..Default::default() }],
            cursor: Some((5, 5)),
            ..Default::default()
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
        let (rgba2, _, _) = scene.capture(&ds[1]).unwrap();
        assert_eq!([rgba2[(70 * 100 + 10) * 4], rgba2[(70 * 100 + 10) * 4 + 1], rgba2[(70 * 100 + 10) * 4 + 2]], colour_of(0));
    }

    /// Clicks focus, press and bring to the front; typing fills the focused
    /// field; Backspace takes back; UI Automation's stand-in reads it all.
    #[test]
    fn input_changes_the_scene() {
        let other = SimWindow { x: 200, y: 200, w: 100, h: 100, exe: r"C:\Apps\other.exe".into(), title: "Other".into(), ..Default::default() };
        let mut s = Scene { displays: vec![SimDisplay { x: 0, y: 0, w: 400, h: 400, primary: true }], windows: vec![other, pad()], ..Default::default() };
        assert_eq!(s.front().unwrap().exe, r"c:\apps\other.exe");
        assert_eq!(s.element_at(30, 110).unwrap(), Element { kind: Kind::Control, name: "Send".into(), password: false });
        for step in super::super::input::click((50, 60), Button::Left, 1, &[]) {
            s.apply(&step);
        }
        assert_eq!(s.front().unwrap().exe, r"c:\windows\notepad.exe", "clicked to the front");
        assert_eq!(s.focused().unwrap().name, "Message");
        for step in super::super::input::text("Hi there") {
            s.apply(&step);
        }
        for step in super::super::input::key(&["backspace".into()], 2) {
            s.apply(&step);
        }
        assert_eq!(s.windows[0].widgets[0].text, "Hi the");
        for step in super::super::input::click((30, 110), Button::Left, 1, &[]) {
            s.apply(&step);
        }
        for step in super::super::input::key(&super::super::input::keys_of("ctrl+s"), 1) {
            s.apply(&step);
        }
        assert_eq!(s.log, ["focused Message", "typed into Message", "pressed Send", "key ctrl+s"]);
        assert!(s.element_at(30, 160).unwrap().password);
        // Drawn: the password's dots, never its text.
        s.windows[0].widgets[2].text = "1234".into();
        let ds = s.displays();
        assert!(s.capture(&ds[0]).is_ok());
    }
}
