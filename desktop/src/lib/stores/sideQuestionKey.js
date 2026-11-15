// **Ctrl+; — a side question in Chat** (chat/side.rs; the Claude app's side
// chat). The key is heard by the in-page hotkey fallback
// (settings/hotkeyFallback.svelte.js), after the person's own shortcuts: the
// global hook never fires while a Yap window has the focus, and a page beside
// the chat (its own webview) sends its Ctrl combos there too (`yap-page-key`:
// a keyCode, no key). Chat, while it's on screen, takes it (`onSideQuestionKey`).

/** The Windows virtual key of ";" on US and UK keyboards (VK_OEM_1). */
const VK_OEM_1 = 186;

let handler = null;

/** Ctrl+; (or ⌘;), nothing else held. Pure. */
export function isSideQuestionKey(e) {
  if (!e || !(e.ctrlKey || e.metaKey) || e.altKey || e.shiftKey) return false;
  if (e.key === ';' || e.code === 'Semicolon') return true;
  // A page beside the chat's key (keyCode only).
  return !e.key && e.keyCode === VK_OEM_1;
}

/** Chat on screen takes the key: `fn()` returns false when it didn't. Returns the undo. */
export function onSideQuestionKey(fn) {
  handler = fn;
  return () => {
    if (handler === fn) handler = null;
  };
}

/** The fallback's: `e` is Ctrl+; and Chat took it. */
export function sideQuestionKey(e) {
  if (!handler || !isSideQuestionKey(e)) return false;
  return handler(e) !== false;
}
