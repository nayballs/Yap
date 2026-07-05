# Yap settings redesign — handoff / context

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
1. **Page headers per section** — add the `h1` + subtitle block (styles already exist as `.page-h`)
   to General/Models/AI Cleanup/History/Advanced so every page opens like the Account one does.
2. **Inline row descriptions** — `Row` now takes a `desc` prop; convert the most important rows from
   ⓘ-tooltip to an inline muted description (matches the mockup; it's a copy pass, not new code).
3. **Reuse the new components elsewhere** — the per-profile provider dropdowns + overlay-position /
   output-device selects in AI Cleanup could adopt `ModeSelector`/refined `Select`; Models engine
   choice could use `Segmented`/`ModeSelector`.
4. **Onboarding.svelte** — it has its own hardcoded styles; adopt the `--yap-*` tokens for a
   consistent look across the first-run flow.
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
