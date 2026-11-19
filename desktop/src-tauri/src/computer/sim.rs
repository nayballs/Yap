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
    /// Its focused widget (the keyboard goes to the front window's).
    pub focus: Option<usize>,
    /// Its focused field's text is all selected (Ctrl+A, a double or triple
    /// click): the next typing replaces it.
    pub selected: bool,
}

/// **A desktop**: its displays, its windows topmost first (the first is in
/// front), the pointer, the focused field, and what input did to it.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Scene {
    pub displays: Vec<SimDisplay>,
    pub windows: Vec<SimWindow>,
    pub cursor: Option<(i32, i32)>,
    /// What input did, in order ("pressed Send", "typed into Message"…).
    pub log: Vec<String>,
    /// The keys held now.
    #[serde(skip)]
    pub held: Vec<String>,
    /// The last button press: where, when, and how many in a row there.
    #[serde(skip)]
    pub last_down: Option<((i32, i32), std::time::Instant, u32)>,
    /// The clipboard's text.
    pub clipboard: Option<String>,
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
            "button" | "key" => Kind::Control,
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
    }

    /// The focused field: the front window's.
    fn focus(&self) -> Option<(usize, usize)> {
        self.windows.first().and_then(|w| w.focus).map(|g| (0, g))
    }

    /// A calculator's key pressed: digits and operators onto its display, `=` works it out, `C` clears.
    fn calc(&mut self, key: &str) {
        let w = &mut self.windows[0];
        let Some(d) = w.widgets.iter().position(|g| g.kind == "text" && g.name == "Display") else { return };
        let shown = w.widgets[d].text.clone();
        w.widgets[d].text = match key {
            "C" => "0".into(),
            "=" => calculate(&shown),
            k if shown == "0" && k.chars().all(|c| c.is_ascii_digit()) => k.into(),
            k => format!("{shown}{k}"),
        };
    }

    /// **One step of input** applied.
    pub fn apply(&mut self, step: &Step) {
        match step {
            Step::Move(x, y) => self.cursor = Some((*x, *y)),
            Step::Down(button) => {
                let Some((x, y)) = self.cursor else { return };
                // Presses at the same spot within half a second: a double or triple click.
                let presses = match self.last_down {
                    Some((at, when, n)) if at == (x, y) && when.elapsed() < std::time::Duration::from_millis(500) => n + 1,
                    _ => 1,
                };
                self.last_down = Some(((x, y), std::time::Instant::now(), presses));
                let Some(i) = self.index_at(x, y) else { return };
                self.raise(i);
                let widget = self.widget_at(0, x, y);
                let name = |s: &Scene, g: usize| s.windows[0].widgets[g].name.clone();
                match (button, widget) {
                    (Button::Left, Some(g)) => match self.windows[0].widgets[g].kind.as_str() {
                        "field" | "password" => {
                            self.windows[0].focus = Some(g);
                            // A double or triple click selects it all; a single one puts the cursor at the end.
                            self.windows[0].selected = presses >= 2;
                            self.log.push(format!("focused {}", name(self, g)));
                        }
                        "key" => {
                            let k = name(self, g);
                            self.calc(&k);
                            self.log.push(format!("pressed {k}"));
                        }
                        "button" => {
                            let b = name(self, g);
                            // A "Delete …" button empties the window's text fields, so a screenshot shows it worked.
                            if b.starts_with("Delete") {
                                for f in self.windows[0].widgets.iter_mut().filter(|f| f.kind == "field") {
                                    f.text.clear();
                                }
                            }
                            self.log.push(format!("pressed {b}"));
                        }
                        "link" => self.log.push(format!("opened {}", name(self, g))),
                        _ => self.log.push(format!("clicked {}", name(self, g))),
                    },
                    (Button::Left, None) => {
                        // A click on the window itself: the keyboard goes to it, no field.
                        self.windows[0].focus = None;
                        self.log.push(format!("clicked {}", self.windows[0].title));
                    }
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
                match (k.as_str(), self.focus()) {
                    ("a", Some((w, _))) if self.held == ["ctrl"] => {
                        self.windows[w].selected = true;
                        self.log.push("selected all".into());
                    }
                    ("backspace" | "delete", Some((w, g))) if self.held.is_empty() => {
                        if std::mem::take(&mut self.windows[w].selected) {
                            self.windows[w].widgets[g].text.clear();
                        } else if k == "backspace" {
                            self.windows[w].widgets[g].text.pop();
                        }
                    }
                    (k, None) if self.held.is_empty() && self.windows.first().is_some_and(|w| w.widgets.iter().any(|g| g.kind == "key" && g.name == k)) => {
                        // A calculator takes its keys from the keyboard too.
                        self.calc(k);
                        self.log.push(format!("pressed {k}"));
                    }
                    ("return", Some((w, g))) if self.held.is_empty() => self.log.push(format!("enter in {}", self.windows[w].widgets[g].name)),
                    _ => self.log.push(format!("key {combo}")),
                }
            }
            Step::KeyUp(k) => self.held.retain(|h| h != k),
            Step::Text(t) => match self.focus() {
                Some((w, g)) => {
                    if std::mem::take(&mut self.windows[w].selected) {
                        self.windows[w].widgets[g].text.clear();
                    }
                    self.windows[w].widgets[g].text.push_str(t);
                    let n = &self.windows[w].widgets[g].name;
                    self.log.push(format!("typed into {n}"));
                }
                // A calculator with no field: its keys typed.
                None if self.windows.first().is_some_and(|w| w.widgets.iter().any(|g| g.kind == "key")) => {
                    for c in t.chars() {
                        let k = match c {
                            '*' | 'x' => "×".to_string(),
                            '/' => "÷".to_string(),
                            c => c.to_string(),
                        };
                        self.calc(&k);
                    }
                    self.log.push(format!("typed {t}"));
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
        // Bottom to top: each window (its colour its own, whatever's raised), its title strip, its widgets.
        let count = self.windows.len();
        for (n, win) in self.windows.iter().rev().enumerate() {
            let r = local(&Rect::new(win.x, win.y, win.w, win.h));
            let c = if win.title.is_empty() { colour_of(n) } else { colour_of(win.title.bytes().fold(count, |h, b| h.wrapping_mul(31).wrapping_add(b as usize))) };
            paint(&mut rgba, w, h, &r, c);
            paint(&mut rgba, w, h, &Rect::new(r.x, r.y, r.w, 24.min(r.h)), c.map(|v| v / 2));
            text(&mut rgba, w, h, r.x + 8, r.y + 5, &win.title, [250, 250, 250]);
            for g in &win.widgets {
                let gr = local(&g.rect_in(win));
                match g.kind.as_str() {
                    "button" | "key" => {
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
        self.focus().map(|(w, g)| self.windows[w].widgets[g].element())
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

    fn clipboard(&self) -> Result<Option<String>, String> {
        with(|s| s.clipboard.clone()).ok_or_else(|| "no stand-in desktop".to_string())
    }

    fn set_clipboard(&self, text: &str) -> Result<(), String> {
        with(|s| {
            s.clipboard = Some(text.to_string());
            s.log.push(format!("copied {} characters", text.chars().count()));
        })
        .ok_or_else(|| "no stand-in desktop".to_string())
    }

    fn open(&self, g: &crate::computer::grants::Granted, app: Option<&crate::apps::App>) -> Result<crate::computer::open::Opened, String> {
        // (A test run's open is recorded, never launched: `e2e_opened_apps`.)
        if let Some(app) = app {
            crate::apps::open(app)?;
        }
        with(|s| s.open(g)).ok_or_else(|| "no stand-in desktop".to_string())
    }
}

impl Scene {
    /// **An allowed app opened**: a window of it raised, else a new one in
    /// front (its title the app's name).
    fn open(&mut self, g: &crate::computer::grants::Granted) -> crate::computer::open::Opened {
        use crate::computer::open::Opened;
        let shell = |w: &Win| crate::computer::windows::SHELL_CLASSES.contains(&w.class.as_str());
        if let Some(i) = (0..self.windows.len()).find(|&i| {
            let w = self.win(i);
            g.owns(&w) && !shell(&w)
        }) {
            self.raise(i);
            self.log.push(format!("brought {} to the front", g.name));
            return Opened::Brought;
        }
        let (exe, aumid) = g.stand_in();
        let at = self.displays.first().map(|d| (d.x + 160, d.y + 120)).unwrap_or((160, 120));
        self.windows.insert(0, SimWindow { x: at.0, y: at.1, w: 640, h: 420, exe, aumid, title: g.name.clone(), ..Default::default() });
        self.log.push(format!("opened {}", g.name));
        Opened::Started
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

/// **A calculator's sum** ("7+8" → "15"; `×`, `÷`, `-` too, left to right). Pure.
pub fn calculate(s: &str) -> String {
    let mut total: Option<f64> = None;
    let mut op = '+';
    let mut num = String::new();
    let flush = |total: &mut Option<f64>, op: char, num: &str| {
        let Ok(n) = num.parse::<f64>() else { return };
        let t = total.unwrap_or(0.0);
        *total = Some(match op {
            '-' => t - n,
            '×' | '*' => t * n,
            '÷' | '/' => if n == 0.0 { f64::NAN } else { t / n },
            _ => t + n,
        });
    };
    for c in s.chars() {
        if c.is_ascii_digit() || c == '.' {
            num.push(c);
        } else if matches!(c, '+' | '-' | '×' | '÷' | '*' | '/') {
            flush(&mut total, op, &num);
            num.clear();
            op = c;
        }
    }
    flush(&mut total, op, &num);
    match total {
        Some(t) if t.is_finite() => {
            if t.fract() == 0.0 {
                format!("{}", t as i64)
            } else {
                format!("{t}")
            }
        }
        _ => "Error".into(),
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

    /// The parity pass's gaps (20 Nov 2026): Ctrl+A and a double click
    /// select a field's text, so typing replaces it; a click in another
    /// window takes the keyboard there; a calculator's keys work it out.
    #[test]
    fn select_all_a_double_click_and_a_calculators_keys_work_as_in_an_app() {
        use super::super::input::{click, key, keys_of, text};
        assert_eq!(calculate("7+8"), "15");
        assert_eq!(calculate("6×7"), "42");
        assert_eq!(calculate("1÷0"), "Error");
        assert_eq!(calculate("2.5+1"), "3.5");
        let calc = SimWindow {
            x: 400,
            y: 10,
            w: 200,
            h: 200,
            title: "Calculator".into(),
            widgets: vec![
                Widget { kind: "text".into(), name: "Display".into(), x: 10, y: 30, w: 180, h: 30, text: "0".into() },
                Widget { kind: "key".into(), name: "7".into(), x: 10, y: 70, w: 40, h: 40, ..Default::default() },
                Widget { kind: "key".into(), name: "+".into(), x: 60, y: 70, w: 40, h: 40, ..Default::default() },
                Widget { kind: "key".into(), name: "=".into(), x: 110, y: 70, w: 40, h: 40, ..Default::default() },
            ],
            ..Default::default()
        };
        let mut s = Scene { displays: vec![SimDisplay { x: 0, y: 0, w: 800, h: 400, primary: true }], windows: vec![pad(), calc], ..Default::default() };
        s.windows[0].widgets[0].text = "Milk, eggs".into();
        let run = |s: &mut Scene, steps: Vec<Step>| steps.iter().for_each(|st| s.apply(st));
        run(&mut s, click((50, 60), Button::Left, 1, &[]));
        run(&mut s, key(&keys_of("ctrl+a"), 1));
        run(&mut s, text("Bread"));
        assert_eq!(s.windows[0].widgets[0].text, "Bread", "Ctrl+A then typing replaces it");
        run(&mut s, click((50, 60), Button::Left, 2, &[]));
        run(&mut s, text("Jam"));
        assert_eq!(s.windows[0].widgets[0].text, "Jam", "a double click selects it");
        // The calculator: its keys clicked, then typed with the keyboard there.
        run(&mut s, click((430, 100), Button::Left, 1, &[]));
        run(&mut s, click((480, 100), Button::Left, 1, &[]));
        run(&mut s, text("8"));
        run(&mut s, click((530, 100), Button::Left, 1, &[]));
        let calc = s.windows.iter().find(|w| w.title == "Calculator").unwrap();
        assert_eq!(calc.widgets[0].text, "15");
        assert!(s.focused().is_none(), "the keyboard is the calculator's, not the pad's field");
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
