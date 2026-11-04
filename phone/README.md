# The phone app

Yap on a phone: ask Yap's AI on your PC from your phone, out loud or typed,
and hear or read the answer. The PC does the work (speech, the AI, Chat's
conversations); the phone is a remote for it.

| Folder | What it is |
|---|---|
| [`web/`](./web/README.md) | **Today's phone app**: the page Yap on the PC serves to a paired phone on the home Wi-Fi (Settings → Phone). Plain HTML, CSS and JavaScript in any phone browser. `web/page/` is what's served, `web/tests/` its tests (`npm run test:link`, in Chromium and Firefox). |

Native apps (Android first, then iPhone) will go beside `web/` as their own
folders, each speaking the same protocol to the PC.

**The PC's side** (the HTTPS server, pairing, the protocol, the voice) is part
of the desktop app: `src-tauri/src/link/`, described in
[`docs/architecture/link.md`](../docs/architecture/link.md). The protocol
(v1, `src-tauri/src/link/wire.rs`) is the contract every phone app keeps to.

**Shared with the desktop**: Chat's model picker code and the providers' logos
are the desktop's own files (`src/lib/core/`, `src/assets/providers/`), served
beside the page as they are (`src-tauri/build.rs` `LINK_SHARED`), so the phone
and the PC never drift apart.
