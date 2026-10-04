# Yap brand files

- `yap-logo.svg` — the master mark (same as `src/assets/yap-logo.svg`).
- `yap-logo-512.png` — 512×512, for GitHub's OAuth app logo
  (github.com/settings/applications/3897989 → "Upload new logo").
- `yap-logo-120.png` — 120×120, for Google's OAuth consent screen
  (Google Auth Platform → Branding → App logo; Google wants 120×120,
  under 1 MB). Adding a logo there needs Google's brand verification.

The 512 is a copy of the generated app icon (`src-tauri/icons/icon.png`, from
`npx tauri icon`) and the 120 is that icon downscaled (Lanczos); regenerate
the icons, then redo both, if the mark changes.
