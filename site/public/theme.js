// The footer's theme choice, put on <html> before the first paint so a dark
// visitor never sees a light flash. "System" is no attribute at all: the CSS
// follows prefers-color-scheme. Kept in localStorage on this device only.
try {
  var saved = localStorage.getItem('theme');
  if (saved === 'light' || saved === 'dark') document.documentElement.setAttribute('data-theme', saved);
} catch (e) {
  /* storage blocked: follow the system */
}
