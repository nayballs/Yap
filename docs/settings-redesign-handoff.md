# Yap settings redesign — handoff / context

> **Historical.** This handoff predates the 2026-07-05/06 parity pushes; the work it
> describes has shipped (see [`ROADMAP.md`](../ROADMAP.md) and
> [`docs/openwhispr-parity.md`](./openwhispr-parity.md)).

> A cold-start briefing for another AI assistant (or a future me). It explains **what we're
> doing, why, what's done, the decisions that are locked, and what's next.** Paired with
> [`CLAUDE.md`](../CLAUDE.md) (how Yap works), [`ROADMAP.md`](../ROADMAP.md) (where it's going),
> and [`docs/openwhispr-teardown.md`](./openwhispr-teardown.md) (the reference app we're learning
> from). Last updated: **2026-07-05**.

---

## 1. The goal in one paragraph

Yap is a Windows-first **local voice-dictation pill** (Tauri 2 + Svelte 5 + Rust). Its settings
window worked but looked "functional, not designed." We're giving it a **professional visual
redesign** modeled on **OpenWhispr**'s UI (a polished Electron reference app we cloned to
`E:\Projects\references\openwhispr`), using OpenWhispr's calm **charcoal palette** but a
**superwhisper-style violet accent**. Separately, we researched OpenWhispr's bigger features
(AI Notepad, AI Chat, Audio Upload, and its Google-account system) and captured what's worth
porting — see the teardown doc and `ROADMAP.md` Phase 7. **This handoff is specifically about the
settings-UX redesign that is now implemented.**

## 2. How we got here (context)

1. Cloned OpenWhispr as a read-only reference and ran research agents over it. Findings →
   [`docs/openwhispr-teardown.md`](./openwhispr-teardown.md) (settings UX, notepad, chat, upload)
   and a new **Phase 7** in `ROADMAP.md` ("from pill to notes surface").
2. Extracted OpenWhispr's design system (tokens, component recipes) and built an approved HTML
   mockup, then **recolored the accent to superwhisper violet** at the user's request.
3. Implemented the redesign in Yap's real Svelte code (this doc's subject).

## 3. Decisions that are LOCKED (do not re-litigate)

- **Palette = OpenWhispr charcoal.** A single-hue (~260) low-chroma elevation ladder where depth
  comes from *lightness*, not borders. The user explicitly likes this look.
- **Accent = superwhisper violet `#6d5cf5`** (NOT Yap's old blue `#3b82f6`). Used sparingly, at low
  alpha, only for active states / focus rings / primary buttons / the "Active" pill.
- **Committed dark theme** (single visual world — the settings window is dark by design).
- **Accounts / Google login = UI ONLY.** The Account section is a polished *placeholder*: an inert
  "Continue with Google — Coming soon" button + "Planned" tags. **Do NOT wire real auth.** Yap is
  local-first ("no accounts, no telemetry"); OpenWhispr's login needs a hosted Better Auth server +
  a private Cloud API that **do not exist in its repo**. If a Pro tier is ever pursued, the *minimal*
  path is Supabase Google OAuth + `tauri-plugin-deep-link` + the `keyring` crate, opt-in, one hosted
  endpoint — nothing more (no sync/workspaces/billing). See teardown §"Accounts".
- **Native font** (Segoe UI / system-ui) is deliberate — correct for a native desktop app; webfonts
  aren't used.

## 4. What's DONE (implemented + builds clean)

All in `E:\Projects\Yap`. `npm run build` passes; the app runs.

- **Design-token system** — `src/app.css` now defines `--yap-*` custom properties (surfaces,
  text-by-opacity, violet accent, borders, radii, duration). This is the single source of truth;
  everything references it instead of hardcoded hex.
- **All `ui/` primitives refactored** onto the tokens with tighter recipes: `Row` (now also supports
  an inline `desc` prop), `Group` (caps section header + hairline-divided panel), `Toggle`, `Select`,
  `Slider`, `Input`, `Button` (adds the `active:scale(.985)` press), `Tooltip`.
- **Two new primitives:**
  - `src/lib/ui/ModeSelector.svelte` — a radio-list "pick one" panel (icon chip + label +
    description + custom radio). **Wired into AI Cleanup → provider** (replaces the dropdown; still
    calls `onProviderChange`, supports `disabled`). Values are the unchanged `PP_PROVIDERS` ids, so
    all downstream cleanup logic is intact.
  - `src/lib/ui/Segmented.svelte` — a sliding segmented control. **Wired into Recording mode**
    (Toggle / Push-to-talk).
- **`src/lib/Settings.svelte`** — reskinned shell: wider/darker sidebar, **icon-chip active nav**,
  a proper **keycap** for hotkeys, roomier content padding, page-header styles. Added a new
  **Account** section (nav footer "Sign in" + a page with the UI-only Google CTA and "Planned"
  extras). Bulk-replaced the old hardcoded palette with tokens.
- **2026-07-05 follow-up (handoff items 1–3 + model picker):**
  - **Page headers** (`.page-h` h1 + subtitle) on every section — General/Models/AI Cleanup/
    History/Advanced/About now open like the Account page.
  - **Inline row descriptions** — the important rows moved from ⓘ-tooltip to the `desc` prop
    (Row **and** Toggle now both support `desc`); long help text stays as `hint` tooltips.
  - **Grouped sidebar nav** — OpenWhispr-style caps labels (`.navcap`): App / AI models /
    Data / System.
  - **Models page ported to OpenWhispr's picker exactly**: `ui/PillTabs.svelte` (vendor tabs
    All/NVIDIA/OpenAI/Community with brand icons) + `ModelRow.svelte` (compact rows: status
    dot with glow states, brand icon, name, size, language, Recommended/Active chips, violet
    Download button, hover-reveal delete). Brand SVGs copied from OpenWhispr (MIT) into
    `src/assets/providers/` with a `providerIcons.js` registry (monochrome logos get
    `invert(1)`). The old big `ModelCard.svelte` now only serves Onboarding.
  - **Last blue-accent stragglers migrated to tokens**: ModelCard/ModelManager, StatusBar
    (menu bg + link hover), Textarea (focus ring + full token refactor), stats hero, About link.
  - **Nav renamed to OpenWhispr's AI-models split**: "Models" → **Speech-to-Text** (mic icon),
    "AI Cleanup" → **Language Models** (brain icon); General got a preferences-sliders icon.
  - **Language Models page rebuilt to OpenWhispr's exact layout+wiring**
    (ReasoningModelSelector): enable toggle → 3-option mode selector (Cloud Providers /
    Local / Self-Hosted) → provider pill tabs with brand icons → "API Key" heading with
    right-aligned "Get your API key →" console link → masked key display (`gsk…okrN` + edit)
    → "Select Model" radio-row list from a per-provider registry (`src/lib/ppModels.js`,
    ported from OW's modelRegistryData.json; first model = auto-selected default on provider
    switch). Custom tab + Self-Hosted mode collapse to Base URL/key/model inputs; Local mode
    is the existing built-in llamafile panel. New primitive: `src/lib/ui/SelectList.svelte`.
    Persisted contract unchanged (`ppProvider`/`ppBaseUrl`/`ppApiKey`/`ppModel`) — `ppMode` +
    `cloudProvider` are UI-only.
  - **Per-provider API keys** (matches OW): new `pp_api_keys: HashMap<String,String>` in
    `config.rs`; Settings stashes/restores the active `ppApiKey` from the map on every
    provider/mode switch (+ live on key input), and reconciles on load (active key wins for
    the active provider; a lone pre-migration key is credited to the derived cloud provider —
    guard added after that path once wiped a key). The backend still reads only the active
    `pp_api_key`.
  - **Live transcription preview** moved from General → Speech-to-Text (below the model
    browser), matching OW's placement.
  - **Prompt Studio** (`src/lib/PromptStudio.svelte`, ported from OW's `ui/PromptStudio.tsx`
    with its wording): replaces the old "Cleanup style" group. One card, three tabs —
    **View** (DEFAULT/CUSTOM PROMPT caps label + Modified chip + Copy; shows the FULL
    effective prompt via the new `get_base_prompt` command composing `llm::BASE_PROMPT` +
    the editable body), **Customize** (caution line, preset select — Yap extra — 12-row
    mono textarea, explicit Save/Reset like OW replacing the old live-save), **Test**
    (MODEL | PROVIDER caps row, Input + CLEANUP chip, full-width Run Test, Output + copy;
    temporarily applies the edited prompt during the run, OW semantics). Adapted wording
    where OW's agent features don't exist in Yap (no {{agentName}}, no instruction
    detection).
  - **OW STT-page features Yap does NOT have yet** (backend work, roadmap candidates):
    purpose tabs (Dictation / Note Recording / Audio Upload — ROADMAP Phase 7), cloud/
    self-hosted STT engines (Yap is deliberately local-only for now), Silero **VAD**
    (toggles + threshold/duration tuning grid — would pair well with the existing pre-roll),
    and the contextual "Enable GPU" banner (Yap defaults GPU on instead).

## 5. The design tokens (reference)

Defined in `src/app.css` (`:root`). Dark-committed.

| Token | Value | Role |
|---|---|---|
| `--yap-bg` | `#191a1e` | window / sidebar (darkest) |
| `--yap-s1` | `#1e1f23` | content pane, inputs |
| `--yap-s2` | `#232429` | panels / cards |
| `--yap-s3` | `#2a2b31` | menus, hover |
| `--yap-raised` | `#313239` | toggle-off track, icon chips |
| `--yap-raised-soft` | `#26272d` | active nav row |
| `--yap-fg` | `#e9e9ea` | primary text (off-white) |
| `--yap-fg-80/62/45` | opacity steps | secondary text (one color, many alphas) |
| `--yap-muted` / `-70` / `-55` | `#9a9aa0` + alphas | descriptions, captions |
| `--yap-primary` | `#6d5cf5` | **superwhisper violet accent** |
| `--yap-primary-wash` / `-tint` / `-line` | violet at .15/.22/.42 | active bg / chip / border |
| `--yap-border` / `-subtle` / `-hover` | `#34353c` / `#2c2d33` / `#41434b` | faint borders + hairlines |
| `--yap-success/warning/danger` | green/amber/red | semantic (separate from accent) |
| `--yap-r-sm/-/-lg/-xl/-full` | 4/6/8/10/9999px | radii (tight, native-feeling) |

## 6. What's NEXT (candidate work, not yet done)

Sequenced roughly by value:
1. ~~**Page headers per section**~~ — DONE (2026-07-05).
2. ~~**Inline row descriptions**~~ — DONE (2026-07-05); a few long explanations remain as ⓘ hints
   on purpose.
3. **Reuse the new components elsewhere** — `PillTabs` could replace the AI-Cleanup per-profile
   provider `<select>`s; the cloud model field could become an OpenWhispr-style "Select Model"
   row list (`ModelRow` minus download affordances) fed by a per-provider model registry.
4. **Onboarding.svelte** — it has its own hardcoded styles (incl. old blue); adopt the `--yap-*`
   tokens + `ModelRow` for a consistent look across the first-run flow.
5. Optional: a subtle window titlebar treatment like the mockup.

The approved visual target is the mockup (an OpenWhispr-charcoal + violet settings window). If you
need the exact look, re-derive from OpenWhispr's `src/index.css` + `ui/SettingsSection.tsx` (see the
teardown appendix), or ask the user for the artifact link.

## 7. How to run / verify

- **Dev (fast, no GPU):** `npm run tauri dev` (stub build — UI renders fully; transcription is a
  placeholder). Full pipeline: `scripts\dev.bat` / `npm run tauri dev -- --features engines` (needs
  the Vulkan SDK). Frontend hot-reloads on save (Vite on :51437).
- **See the redesign:** the pill is hidden by default → open Settings from the **tray icon**
  (left-click). Click through **General** (segmented recording mode, keycap, toggles), **AI Cleanup**
  (the provider cards), and the **Sign in** footer → Account page.
- **CI sanity:** `npm run build` (frontend) + `cargo check --locked` (stub). Both must stay green.
- Two harmless pre-existing warnings (`.usage-note`, `.cloud-form .mic-pick` unused CSS) are not from
  this work.

## 8. Key files (this redesign)

- `src/app.css` — the `--yap-*` design tokens.
- `src/lib/ui/ModeSelector.svelte`, `src/lib/ui/Segmented.svelte` — new primitives.
- `src/lib/ui/{Row,Group,Toggle,Select,Slider,Input,Button,Tooltip}.svelte` — token refactor.
- `src/lib/Settings.svelte` — shell + Account section + wiring (the big diff).
- `docs/openwhispr-teardown.md` — the full reference-app teardown (settings/notepad/chat/upload/accounts).
- `ROADMAP.md` — Phase 4 "Settings UX overhaul" + Phase 7 "notes surface" reference this.
