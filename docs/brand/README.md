# Yap brand files

- `yap-logo.svg` — the master mark (same as `src/assets/yap-logo.svg`).
- `yap-logo-512.png` — 512×512, for GitHub's OAuth app logo
  (github.com/settings/applications/3897989 → "Upload new logo").
- `yap-logo-128.png` — 128×128, for Google's OAuth consent screen
  (Google Auth Platform → Branding → App logo; Google asks for ~120×120,
  under 1 MB). Adding a logo there needs Google's brand verification.

The PNGs are copies of the generated app icons (`src-tauri/icons`, from
`npx tauri icon`); regenerate those, then recopy, if the mark changes.
