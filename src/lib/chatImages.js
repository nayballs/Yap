// Chat's screenshots (Snip to Chat, src-tauri/src/snip.rs): their address
// for an <img> — by id only: the yapsnip scheme serves
// <data>/chat-images/<id>.png and nothing else — the one-tap prompts, and
// how a removed one is said.
import { convertFileSrc } from '@tauri-apps/api/core';

/** The one-tap prompts above the message box while a screenshot waits. */
export const SNIP_PROMPTS = ["What's this?", 'Explain this error', 'Summarise'];

/** What Enter on an empty box sends with a screenshot. */
export const DEFAULT_PROMPT = "What's this?";

/** Screenshots one message carries, at most (snip/images.rs). */
export const MAX_SHOTS = 4;

/** A screenshot's address (`http://yapsnip.localhost/<id>` in WebView2). */
export function chatImageUrl(id) {
  return convertFileSrc(id, 'yapsnip');
}

/** "Screenshot removed after 30 days" (the setting's period), in its place. */
export function removedLine(days) {
  return days > 0 ? `Screenshot removed after ${days} days` : 'Screenshot removed';
}

/** "812 × 430" */
export function sizeLine(img) {
  return img?.w && img?.h ? `${img.w} × ${img.h}` : '';
}

/** Whether a failed send was the model not reading the screenshot (snip/vision.rs). */
export function visionRefused(e) {
  const s = String(e ?? '');
  return s.includes("couldn't read the screenshot") || s.includes("can't read images");
}
