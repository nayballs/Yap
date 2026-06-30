# TEST-NOTES

## 2026-06-28 — Sandbox window resolution check (post-rebuild)

Goal: confirm the rebuild fixed window disambiguation in the Yap sandbox tools.

1. **Windows list — FAIL.** `sandbox_snapshot` still lists all four windows as
   identical bare entries:
   `[0] Yap — http://localhost:1430/` … `[3] Yap — http://localhost:1430/`.
   Indices are present, but there are **no distinct URLs/routes or titles** — every
   window reports the same title ("Yap") and the same URL. The expected
   distinct-URL-per-window fix did **not** land.

2. **`window:"settings"` — FAIL.** Errors with
   *"No app window matching 'settings'"* because no window URL/title contains a route
   substring. Targeting only works by **index**. By probing indices, window **[2]**
   turned out to be the Settings window (tabs: General / Models / AI Cleanup /
   Advanced / About), but this required guessing the index, not resolving by route.

3. **`sandbox_screenshot` — FAIL.** Returned *"No live app frame yet — open the App
   Preview…"* on repeated attempts. Could **not** capture, so could not confirm the
   screenshot shows the same window/index the snapshot resolved. (The live preview is
   also decoupled from the index-targeted snapshot.)

4. **Click "AI Cleanup" — PASS.** With window [2] targeted, clicking the AI Cleanup
   tab (@e3) landed on the **AI Cleanup panel** (Enable AI cleanup toggle, provider
   dropdown Groq/OpenAI/OpenRouter/Local/Custom, API base URL, API key, model field,
   Test button) — **not** a model-download button. **No model download was triggered.**

**Verdict:** Window disambiguation is still broken — windows are indistinguishable by
URL/title and only addressable by index; screenshot capture is unavailable. Only the
in-window click/navigation (step 4) works correctly.

## 2026-06-28 — Sandbox window resolution re-test (second rebuild)

Goal: re-confirm the labels fix after another rebuild.

1. **Windows list — PASS (fixed).** `sandbox_snapshot` now lists each window with a
   **distinct label**, not four bare "Yap"s:
   `[0] overlay`, `[1] onboarding`, `[2] settings`, `[3] pill`. The labels are
   present and unique. (Caveats: the order differs from the spec's expected
   `[0] pill / [1] settings / [2] onboarding / [3] overlay`; OS titles are still all
   "Yap", though after windows render some report `"Yap Settings" (736×679)`. The
   disambiguation fix — distinct labels — has landed.)

2. **`window:"settings"` — PASS (fixed).** `sandbox_snapshot {window:"settings"}`
   resolves to `[2] settings` by label (no index guessing). `window:"pill"` likewise
   resolved to `[3] pill`.

3. **`sandbox_screenshot` — FAIL (still).** Returned *"No live app frame yet — open
   the App Preview…"* on every attempt, including after targeting the static pill
   window. Could not capture a frame, so it could not echo a label/index. Screenshot
   capture remains unavailable.

4. **Click "AI Cleanup" — PASS.** With Settings targeted, clicking the AI Cleanup tab
   (@e3) landed on the **AI Cleanup panel**: Enable AI cleanup toggle, provider
   dropdown (Groq / OpenAI / OpenRouter / Local · Ollama·LM Studio / Custom), API base
   URL (`https://api.groq.com/openai/v1`), API key (`sk-…`), model
   (`llama-3.1-8b-instant`), and Test button. **No model download triggered.**

**Verdict:** Window disambiguation is now FIXED — labels are distinct and resolve by
name (steps 1, 2, 4 pass). Only `sandbox_screenshot` is still broken (no live frame).

## 2026-06-28 — Sandbox screenshot CDP-capture check (third rebuild)

Goal: confirm the rebuild makes `sandbox_screenshot` return a real frame for an
opaque secondary window (Settings), captured via CDP, without focusing the app first.
No app click/focus before snapshot; no model downloads.

Note: the app was not registered as the active sandbox on first try
(`sandbox_snapshot` → "No sandbox app is running"). `sandbox_start` re-launched the
active project (Yap dev: Vite on :1430, webview CDP on :9653) and it auto-registered.

1. **`sandbox_snapshot {window:"settings"}` — PASS.** Resolved to
   `[2] settings "Yap" — http://localhost:1430/ (port 9653)` by label, 46 refs. No
   prior focus/click needed. Windows list distinct: `[0] overlay / [1] onboarding /
   [2] settings / [3] pill`.

2. **`sandbox_screenshot` (Settings) — PASS (fixed).** Returned a real frame —
   *"Showing [2] settings 'Yap'"* — i.e. captured via the CDP source on the **opaque
   Settings window**, not "No live app frame yet". Image confirmed = Yap **Settings /
   General** tab (Activation: hotkey + recording mode; Audio: mic, output, sound cue,
   cue volume, mute; Appearance: show pill / overlay). Correct window.

3. **`sandbox_screenshot` (Onboarding) — PASS.** `window:"onboarding"` resolved to
   `[1] onboarding` and the screenshot returned a frame — *"Showing [1] onboarding"* —
   showing the **"Welcome to Yap"** model picker (Parakeet V3 Recommended, Parakeet V2,
   Whisper Large v3 Turbo, Whisper Large v3 ✓ Active, …). Correct window.

4. **Click "AI Cleanup" + re-screenshot — PASS (tracks).** Re-targeted Settings,
   clicked AI Cleanup (@e3), re-screenshot now shows the **AI Cleanup panel** live
   (sidebar "AI Cleanup" highlighted; Enable AI cleanup toggle, Provider = Groq, Base
   URL `https://api.groq.com/openai/v1`, API key, Model `llama-3.1-8b-instant`,
   Cleanup prompt, Test button). The capture tracks the current in-window state. **No
   model download triggered.**

**Verdict:** `sandbox_screenshot` is now FIXED — it returns real CDP frames for opaque
secondary windows (Settings, Onboarding) without prior focus, identifies the window in
its output, and tracks live navigation (AI Cleanup click). All four checks pass; the
last remaining sandbox gap from prior runs is closed.
