// Chat's screenshots (Snip to Chat, src-tauri/src/snip.rs): their address
// for an <img> — by id only: the yapsnip scheme serves
// <data>/chat-images/<id>.png and nothing else — the one-tap prompts, how
// a removed one is said, a screenshot's right-click menu (Copy image, Save
// image as…: snip/copy_save.rs), a picture pasted or dropped into the box
// (snip/attach.rs), and a question whose screenshot went as its text
// (snip/ocr.rs: the text after SCREEN_TEXT_MARK, folded in its bubble).
import { convertFileSrc, invoke } from '@tauri-apps/api/core';
import { toast } from '../../ui/toast.svelte.js';

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

/** "Copy image": onto the clipboard, out of Win+V history (snip/copy_save.rs). */
export async function copyImage(id) {
  try {
    await invoke('chat_image_copy', { id });
    toast({ title: 'Screenshot copied', variant: 'success' });
  } catch (e) {
    toast({ title: "Couldn't copy the screenshot", description: String(e), variant: 'destructive' });
  }
}

/** "Save image as…": Windows' Save dialog, in the folder used last. */
export async function saveImage(id) {
  try {
    if (await invoke('chat_image_save', { id })) toast({ title: 'Screenshot saved', variant: 'success' });
  } catch (e) {
    toast({ title: "Couldn't save the screenshot", description: String(e), variant: 'destructive' });
  }
}

/** A screenshot's right-click menu: open it large, copy it, save it. */
export function imageItems(img, { onview = null } = {}) {
  if (!img?.id || img.gone) return [];
  return [
    onview && { label: 'Open', icon: 'open', onselect: () => onview(img.id) },
    { label: 'Copy image', icon: 'copy', onselect: () => copyImage(img.id) },
    { label: 'Save image as…', icon: 'download', onselect: () => saveImage(img.id) },
  ];
}

/** The picture a paste event carries, if it has one (an image copied in any app). */
export function pastedImage(e) {
  const items = [...(e?.clipboardData?.items ?? [])];
  const item = items.find((i) => i.kind === 'file' && i.type.startsWith('image/'));
  return item?.getAsFile() ?? null;
}

/** Keep a pasted picture as a chat screenshot (snip/attach.rs): its bytes as the call's body. */
export async function keepPasted(file) {
  const bytes = new Uint8Array(await file.arrayBuffer());
  return invoke('chat_image_paste', bytes);
}

/**
 * The line before a screenshot's text in a question sent "as text"
 * (snip/ocr.rs `MARK`, `MARK_MANY`): what's after it is folded in the bubble.
 */
export const SCREEN_TEXT_MARKS = [
  '[Text read from the screenshot on this PC, sent instead of the picture]',
  '[Text read from the screenshots on this PC, sent instead of the pictures]',
];

/** A question's own words and the screenshot text that went with them (or null). */
export function splitScreenText(text) {
  const s = String(text ?? '');
  for (const mark of SCREEN_TEXT_MARKS) {
    const at = s.indexOf(`\n\n${mark}\n`);
    if (at >= 0) return { question: s.slice(0, at), screenText: s.slice(at + mark.length + 3) };
  }
  return { question: s, screenText: null };
}
