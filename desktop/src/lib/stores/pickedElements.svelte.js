// **Elements picked on a page beside the chat** (14 Nov 2026; "Select an
// element or annotate", chat/pages/pick.rs): waiting in the message box as
// chips until the next question takes them (`chat_send`'s `elements`), as
// the Claude Code app's browser pane attaches a picked element to the prompt.
// Each one's picture is in Chat's box already (Rust's, source "element"):
// its chip shows it, and removing the chip removes it.
import { invoke } from '@tauri-apps/api/core';

/** The elements waiting, oldest first: `{uid, tag, id, classes, text, path, styles, html, siblings, url, title, image}`. */
export const picked = $state({ list: [] });

let seq = 0;

/** A pick from the page (`yap-page-picked`'s `element`). */
export function addPicked(el) {
  if (!el?.tag) return;
  seq += 1;
  picked.list = [...picked.list, { ...el, uid: `el-${Date.now()}-${seq}` }];
}

/** Its chip's ×: gone, and its picture out of Chat's box. */
export function removePicked(uid) {
  const el = picked.list.find((e) => e.uid === uid);
  picked.list = picked.list.filter((e) => e.uid !== uid);
  if (el?.image) invoke('chat_waiting_remove', { id: el.image }).catch(() => {});
}

/** A question takes them (without their chip ids). */
export function takePicked() {
  const out = picked.list.map(({ uid: _uid, ...e }) => e);
  picked.list = [];
  return out;
}

/** A model that can't read images: the pictures stay out (the words go). */
export function pictureIds() {
  return picked.list.map((e) => e.image).filter(Boolean);
}
