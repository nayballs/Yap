// Runs in <head> before the first paint.
// 1. The footer's theme choice, put on <html> so a dark visitor never sees a
//    light flash. "System" is no attribute at all: the CSS follows
//    prefers-color-scheme. Kept in localStorage on this device only.
// 2. `js` on <html>: the live demos and tabs style their scripted states
//    behind it, so without script every panel simply shows, and with it
//    nothing jumps when the scripts start.
// 3. The draft notes' switch (preview builds only): shown when chosen.
var root = document.documentElement;
root.classList.add('js');
try {
  var saved = localStorage.getItem('theme');
  if (saved === 'light' || saved === 'dark') root.setAttribute('data-theme', saved);
  if (localStorage.getItem('draft-notes') === 'shown') root.classList.add('show-drafts');
} catch (e) {
  /* storage blocked: follow the system */
}
