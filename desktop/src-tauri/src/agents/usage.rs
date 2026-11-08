//! **What an agent's turn used** — the Agent panel's numbers
//! (`views/chat/AgentPanel.svelte`, `core/agentPanel.js`): its tokens, how
//! full its conversation is (the context), and — while it works, before
//! Claude Code's own figure at its end — what it would cost on the API,
//! estimated from its tokens.
//!
//! - **Each model call's usage** comes with its `assistant` messages
//!   (`message.usage`: what it read new, from the cache and into it, and
//!   what it wrote so far). Claude Code sends a message a block at a time,
//!   each with the call's usage as it stands, so a call is kept by its
//!   message id, the latest of each, and counted once ([`Meter`]).
//! - **The context** is the latest call of the agent's own conversation
//!   (never a helper's, under a Task): what it read and wrote, against the
//!   model's window — Claude Code's own word at a result (`modelUsage`'s
//!   `contextWindow`), else an earlier turn's, else Yap's catalog
//!   (`chat::budget::context_window`), as a plan chat's ring has it.
//! - **The estimate** ([`cost_of`]): each call's tokens at the model's list
//!   price for input, output, cache reads and cache writes ([`PRICES`]).
//!   Claude Code's own `total_cost_usd` replaces it at the turn's result
//!   (the turn's `cost`); a process that ended without one keeps its
//!   estimate (`estimate`), as it was spent.
//! - **Stored with the turn** (`store::TurnNote::usage`, `agents.json`), so a
//!   restart shows the same numbers; while it works it goes to the page as
//!   `yap-agent`'s `usage`, never into the agent's log (many a turn).
//!
//! The price table and the cost and context maths are adapted from
//! savvy-progress's agents panel (`hooks/register.tsx`: `PRICES`,
//! `HAIKU_LONG`, `costOf`, the context fill; list prices as of 2026-10-10),
//! github.com/JohnnyVizz/claude-kit, under its MIT License:
//!
//! > MIT License
//! >
//! > Copyright (c) 2026 johnnyvizz
//! >
//! > Permission is hereby granted, free of charge, to any person obtaining a
//! > copy of this software and associated documentation files (the
//! > "Software"), to deal in the Software without restriction, including
//! > without limitation the rights to use, copy, modify, merge, publish,
//! > distribute, sublicense, and/or sell copies of the Software, and to
//! > permit persons to whom the Software is furnished to do so, subject to
//! > the following conditions:
//! >
//! > The above copyright notice and this permission notice shall be included
//! > in all copies or substantial portions of the Software.
//! >
//! > THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
//! > OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
//! > MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.
//! > IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
//! > CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT,
//! > TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE
//! > SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
//!
//! Pure: no store, no Tauri.

use serde::{Deserialize, Serialize};

use crate::ai::claude_code::events::{TurnResult, Usage};

/// **What one turn used** (its processes added up: Continue carries a turn
/// on in a new one). A field of the turn's own in `agents.json`: an older
/// Yap drops it on its save, and the turn then shows no numbers.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TurnUsage {
    /// Tokens read new, written, read from the cache, written into it.
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    /// **How full its conversation is**: the latest call's prompt and answer
    /// (what the next call starts from).
    pub context: u64,
    /// The model's context window (0: not known).
    pub window: u64,
    /// What its tokens would cost on the API, estimated, where Claude Code
    /// hasn't said yet (while it works; a process that ended without a
    /// result). Claude Code's own figure is the turn's `cost`.
    #[serde(skip_serializing_if = "is_zero")]
    pub estimate: f64,
}

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

impl TurnUsage {
    /// Every token it used, read and written (savvy's "Tokens").
    pub fn tokens(&self) -> u64 {
        self.input + self.output + self.cache_read + self.cache_write
    }
}

/// One price: US dollars per million tokens — input, output, cache read,
/// cache write.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Price {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_write: f64,
}

const fn p(input: f64, output: f64, cache_read: f64, cache_write: f64) -> Price {
    Price { input, output, cache_read, cache_write }
}

/// **The price table** (savvy-progress's `PRICES`, list prices as of
/// 2026-10-10): the first row any of whose words is in a model's id wins,
/// so a release sits above its family. Its input and output agree with Yap's
/// own table (`ai/usage/prices.rs`, a test holds them together); cache
/// writes are at the 1-hour rate (2× input), which Claude Code writes on a
/// subscription.
pub const PRICES: &[(&[&str], Price)] = &[
    (&["fable-5-1", "mythos-5-1"], p(10.0, 50.0, 0.25, 20.0)),
    (&["fable", "mythos"], p(10.0, 50.0, 1.0, 20.0)),
    (&["opus-5-5"], p(4.0, 20.0, 0.2, 8.0)),
    (&["opus"], p(5.0, 25.0, 0.5, 10.0)),
    (&["sonnet-5-5"], p(2.0, 10.0, 0.1, 4.0)),
    (&["sonnet-5"], p(2.0, 10.0, 0.2, 4.0)),
    (&["sonnet"], p(3.0, 15.0, 0.3, 6.0)),
    (&["haiku-5-5"], p(0.1, 0.5, 0.01, 0.2)),
    (&["haiku"], p(1.0, 5.0, 0.1, 2.0)),
];
/// A model the table doesn't know: Opus 5.5's.
const UNKNOWN: Price = p(4.0, 20.0, 0.2, 8.0);
/// Haiku 5.5 is priced by prompt length: a call whose prompt (input, cache
/// reads and writes together) is over this pays [`HAIKU_LONG`].
pub const HAIKU_LONG_PROMPT: u64 = 100_000;
pub const HAIKU_LONG: Price = p(0.5, 2.5, 0.05, 1.0);

/// `model`'s price for a call with a prompt of `prompt` tokens. Pure.
pub fn price_of(model: &str, prompt: u64) -> Price {
    let id = model.to_ascii_lowercase();
    if prompt > HAIKU_LONG_PROMPT && id.contains("haiku-5-5") {
        return HAIKU_LONG;
    }
    PRICES.iter().find(|(words, _)| words.iter().any(|w| id.contains(w))).map_or(UNKNOWN, |(_, p)| *p)
}

/// The prompt a call read: new, from the cache and into it.
fn prompt_of(u: &Usage) -> u64 {
    u.input_tokens + u.cache_read_input_tokens + u.cache_creation_input_tokens
}

/// **What one model call's tokens cost on the API**, estimated, in US
/// dollars (savvy's `costOf` for a request). Pure.
pub fn cost_of(model: &str, u: &Usage) -> f64 {
    let p = price_of(model, prompt_of(u));
    (u.input_tokens as f64 * p.input
        + u.output_tokens as f64 * p.output
        + u.cache_read_input_tokens as f64 * p.cache_read
        + u.cache_creation_input_tokens as f64 * p.cache_write)
        / 1e6
}

/// One model call as last seen.
#[derive(Debug, Clone)]
struct Call {
    id: String,
    model: String,
    usage: Usage,
}

/// **Following one process of a turn**: its calls' usage as they come, the
/// turn's usage before it, and the model's window.
#[derive(Debug, Clone, Default)]
pub struct Meter {
    before: TurnUsage,
    calls: Vec<Call>,
    /// The latest call of the agent's own conversation: its prompt and answer.
    context: Option<u64>,
    window: u64,
    /// The model the process runs (`system/init`'s): a call that names none.
    model: String,
    /// The estimate at the latest result: Claude Code's figure covers it.
    settled: f64,
}

impl Meter {
    /// A process of a turn whose earlier processes used `before`, on a model
    /// with a `window` (0: not known).
    pub fn new(before: Option<TurnUsage>, model: &str, window: u64) -> Meter {
        let before = before.unwrap_or_default();
        let window = if window > 0 { window } else { before.window };
        Meter { before, window, model: model.to_string(), ..Default::default() }
    }

    /// The model the process runs (`system/init`).
    pub fn model(&mut self, model: &str) {
        if !model.trim().is_empty() {
            self.model = model.trim().to_string();
        }
    }

    /// **A message's usage** (`id` its message's, `model` its, `main` the
    /// agent's own conversation, not a helper's): true when what the turn
    /// shows changed.
    pub fn call(&mut self, id: Option<&str>, model: Option<&str>, usage: &Usage, main: bool) -> bool {
        let id = id.unwrap_or("").to_string();
        let model = model.filter(|m| !m.trim().is_empty()).map_or_else(|| self.model.clone(), str::to_string);
        // A message without an id is a call of its own.
        let at = if id.is_empty() { None } else { self.calls.iter().position(|c| c.id == id) };
        let changed = match at {
            Some(i) if self.calls[i].usage == *usage => false,
            Some(i) => {
                self.calls[i].usage = usage.clone();
                true
            }
            None => {
                self.calls.push(Call { id, model, usage: usage.clone() });
                true
            }
        };
        if main {
            let context = prompt_of(usage) + usage.output_tokens;
            if self.context != Some(context) {
                self.context = Some(context);
                return true;
            }
        }
        changed
    }

    /// This process's calls added up.
    fn sums(&self) -> Usage {
        let mut s = Usage::default();
        for c in &self.calls {
            s.input_tokens += c.usage.input_tokens;
            s.output_tokens += c.usage.output_tokens;
            s.cache_read_input_tokens += c.usage.cache_read_input_tokens;
            s.cache_creation_input_tokens += c.usage.cache_creation_input_tokens;
        }
        s
    }

    fn estimated(&self) -> f64 {
        self.calls.iter().map(|c| cost_of(&c.model, &c.usage)).sum()
    }

    fn with(&self, s: &Usage, estimate: f64) -> TurnUsage {
        let b = &self.before;
        TurnUsage {
            input: b.input + s.input_tokens,
            output: b.output + s.output_tokens,
            cache_read: b.cache_read + s.cache_read_input_tokens,
            cache_write: b.cache_write + s.cache_creation_input_tokens,
            context: self.context.unwrap_or(b.context),
            window: self.window,
            estimate,
        }
    }

    /// **The turn's usage now**: its earlier processes' and this one's so
    /// far, with what Claude Code hasn't priced yet estimated.
    pub fn now(&self) -> TurnUsage {
        let estimate = self.before.estimate + (self.estimated() - self.settled).max(0.0);
        self.with(&self.sums(), estimate)
    }

    /// **At a result**: Claude Code's own sums where they say more (a
    /// helper's calls it counted that weren't seen), its model's window, and
    /// no estimate for what its `total_cost_usd` covers.
    pub fn at_result(&mut self, r: &TurnResult) -> TurnUsage {
        if let Some(w) = r.context_window(&self.model).or_else(|| (r.context_windows.len() == 1).then(|| r.context_windows[0].1).filter(|w| *w > 0)) {
            self.window = w;
        }
        self.settled = self.estimated();
        let seen = self.sums();
        let said = &r.usage;
        let s = Usage {
            input_tokens: seen.input_tokens.max(said.input_tokens),
            output_tokens: seen.output_tokens.max(said.output_tokens),
            cache_read_input_tokens: seen.cache_read_input_tokens.max(said.cache_read_input_tokens),
            cache_creation_input_tokens: seen.cache_creation_input_tokens.max(said.cache_creation_input_tokens),
            ..Usage::default()
        };
        self.with(&s, self.before.estimate)
    }

    /// Anything to say: a call seen, or an earlier process's numbers.
    pub fn said(&self) -> bool {
        !self.calls.is_empty() || self.before.tokens() > 0
    }
}

/// **The window an agent's next turn starts with**: its latest earlier turn's
/// on the same model, as Claude Code said it (0: none). Pure.
pub fn window_before<'a>(turns: impl IntoIterator<Item = (&'a str, Option<&'a TurnUsage>)>, model: &str) -> u64 {
    turns.into_iter().filter(|(m, _)| *m == model).filter_map(|(_, u)| u.map(|u| u.window)).filter(|w| *w > 0).last().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage(input: u64, output: u64, read: u64, write: u64) -> Usage {
        Usage { input_tokens: input, output_tokens: output, cache_read_input_tokens: read, cache_creation_input_tokens: write, ..Default::default() }
    }

    #[test]
    fn a_calls_cost_is_its_tokens_at_the_models_prices() {
        // Sonnet 5.5: 1,000 in × $2 + 200 out × $10 + 40,000 read × $0.1 + 300 written × $4, per million.
        let c = cost_of("claude-sonnet-5-5", &usage(1_000, 200, 40_000, 300));
        assert!((c - (0.002 + 0.002 + 0.004 + 0.0012)).abs() < 1e-12, "{c}");
        // A release above its family; a dated snapshot is its model.
        assert_eq!(price_of("claude-opus-5-5", 0), p(4.0, 20.0, 0.2, 8.0));
        assert_eq!(price_of("claude-opus-4-8", 0), p(5.0, 25.0, 0.5, 10.0));
        assert_eq!(price_of("claude-sonnet-4-5-20250929", 0), p(3.0, 15.0, 0.3, 6.0));
        assert_eq!(price_of("claude-fable-5-1", 0).cache_read, 0.25);
        // Haiku 5.5 over 100K of prompt: its long-prompt prices.
        assert_eq!(price_of("claude-haiku-5-5", 100_000), p(0.1, 0.5, 0.01, 0.2));
        assert_eq!(price_of("claude-haiku-5-5", 100_001), HAIKU_LONG);
        assert_eq!(price_of("claude-haiku-4-5", 500_000), p(1.0, 5.0, 0.1, 2.0));
        // A model nobody knows: Opus 5.5's, never nothing.
        assert_eq!(price_of("something-new", 0), UNKNOWN);
    }

    #[test]
    fn the_table_agrees_with_yaps_own_on_input_and_output() {
        use crate::ai::usage::prices;
        for (model, prompt) in [
            ("claude-fable-5-1", 0),
            ("claude-fable-5", 0),
            ("claude-opus-5-5", 0),
            ("claude-opus-5", 0),
            ("claude-opus-4-8", 0),
            ("claude-sonnet-5-5", 0),
            ("claude-sonnet-5", 0),
            ("claude-sonnet-4-6", 0),
            ("claude-haiku-5-5", 1_000),
            ("claude-haiku-5-5", 200_000),
            ("claude-haiku-4-5", 0),
        ] {
            let ours = prices::price("anthropic", model, "2026-11-08", prompt).unwrap_or_else(|| panic!("{model}"));
            let theirs = price_of(model, prompt);
            assert_eq!((theirs.input, theirs.output), (ours.input, ours.output), "{model} at {prompt}");
        }
    }

    #[test]
    fn a_call_is_counted_once_and_the_context_is_the_agents_latest() {
        let mut m = Meter::new(None, "claude-sonnet-5-5", 200_000);
        assert!(!m.said());
        // A message a block at a time, each with the call's usage as it stands.
        assert!(m.call(Some("m1"), None, &usage(10, 5, 1_000, 200), true));
        assert!(!m.call(Some("m1"), None, &usage(10, 5, 1_000, 200), true), "the same again: nothing new");
        assert!(m.call(Some("m1"), None, &usage(10, 40, 1_000, 200), true), "more written");
        let u = m.now();
        assert_eq!((u.input, u.output, u.cache_read, u.cache_write, u.context, u.window), (10, 40, 1_000, 200, 1_250, 200_000));
        // A helper's call (under a Task) costs too, but isn't the context.
        assert!(m.call(Some("h1"), Some("claude-haiku-5-5"), &usage(500, 50, 0, 0), false));
        let u = m.now();
        assert_eq!((u.input, u.output, u.context), (510, 90, 1_250));
        let want = cost_of("claude-sonnet-5-5", &usage(10, 40, 1_000, 200)) + cost_of("claude-haiku-5-5", &usage(500, 50, 0, 0));
        assert!((u.estimate - want).abs() < 1e-12);
        assert_eq!(u.tokens(), 510 + 90 + 1_000 + 200);
        // The next call of its own: the context is it.
        m.call(Some("m2"), None, &usage(20, 10, 1_300, 0), true);
        assert_eq!(m.now().context, 1_330);
    }

    #[test]
    fn a_result_settles_the_estimate_and_says_the_window() {
        let mut m = Meter::new(None, "claude-sonnet-5-5", 0);
        m.call(Some("m1"), None, &usage(100, 20, 3_000, 0), true);
        assert!(m.now().estimate > 0.0);
        let r = TurnResult { usage: usage(120, 20, 2_000, 0), context_windows: vec![("claude-sonnet-5-5".into(), 1_000_000)], ..Default::default() };
        let u = m.at_result(&r);
        // Each field the larger of what was seen and what Claude Code said.
        assert_eq!((u.input, u.output, u.cache_read, u.window, u.estimate), (120, 20, 3_000, 1_000_000, 0.0));
        // A call after it: only that one estimated.
        m.call(Some("m2"), None, &usage(5, 5, 0, 0), true);
        let after = m.now();
        assert!((after.estimate - cost_of("claude-sonnet-5-5", &usage(5, 5, 0, 0))).abs() < 1e-12);
        assert_eq!(after.window, 1_000_000);
    }

    #[test]
    fn a_turn_carried_on_adds_to_what_it_used_before() {
        // Its earlier process ended without a result: its estimate stays (spent).
        let before = TurnUsage { input: 100, output: 10, cache_read: 1_000, cache_write: 50, context: 1_160, window: 200_000, estimate: 0.01 };
        let mut m = Meter::new(Some(before.clone()), "claude-sonnet-5-5", 0);
        assert!(m.said());
        assert_eq!(m.now(), before, "nothing new yet: as it was, its window kept");
        m.call(Some("c1"), None, &usage(1, 1, 1_200, 0), true);
        let u = m.now();
        assert_eq!((u.input, u.output, u.cache_read, u.cache_write, u.context), (101, 11, 2_200, 50, 1_202));
        assert!(u.estimate > 0.01);
        let r = TurnResult { usage: usage(1, 1, 1_200, 0), ..Default::default() };
        assert!((m.at_result(&r).estimate - 0.01).abs() < 1e-12);
    }

    #[test]
    fn the_next_turns_window_is_the_latest_said_on_its_model() {
        let a = TurnUsage { window: 200_000, ..Default::default() };
        let b = TurnUsage { window: 1_000_000, ..Default::default() };
        let none = TurnUsage::default();
        let turns = [("claude-sonnet-5-5", Some(&a)), ("claude-opus-5-5", Some(&b)), ("claude-sonnet-5-5", Some(&none)), ("claude-sonnet-5-5", None)];
        assert_eq!(window_before(turns, "claude-sonnet-5-5"), 200_000);
        assert_eq!(window_before(turns, "claude-opus-5-5"), 1_000_000);
        assert_eq!(window_before(turns, "claude-haiku-5-5"), 0);
    }

    #[test]
    fn stored_it_says_only_what_it_has() {
        let v = serde_json::to_value(TurnUsage { input: 1, context: 2, window: 3, ..Default::default() }).unwrap();
        assert_eq!(v, serde_json::json!({"input":1,"output":0,"cacheRead":0,"cacheWrite":0,"context":2,"window":3}));
        let back: TurnUsage = serde_json::from_value(serde_json::json!({"input":1,"estimate":0.5,"later":true})).unwrap();
        assert_eq!((back.input, back.estimate), (1, 0.5));
    }
}
