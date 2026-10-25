## What this changes

<!-- What it does, and why. Link the issue: "Fixes #123". -->

## How it was tested

<!-- What you ran and what you tried by hand. A screenshot for anything visual. -->

- [ ] `npm run build`
- [ ] `cargo clippy --locked --all-targets -- -D warnings` and `cargo test --locked` (in `src-tauri`)
- [ ] `npm run test:app`, with a spec added or updated for what changed
- [ ] `npm run test:real` (only for pasting, the clipboard, the microphone or speech-to-text)

## Checklist

- [ ] Fictional data only in tests, fixtures and screenshots (`npm run privacy:scan` passes)
- [ ] Nothing a user said or typed in logs at `info` level or above
- [ ] The area's file in `docs/architecture/` is updated to match the code
