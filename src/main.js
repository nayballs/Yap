import { mount } from 'svelte';
// Bundled typefaces — the exact pair Wispr Flow ships (both OFL/Google Fonts):
// Figtree = UI sans, EB Garamond (+italic) = display serif. Segoe UI fallback.
import '@fontsource-variable/figtree';
import '@fontsource-variable/eb-garamond';
import '@fontsource-variable/eb-garamond/wght-italic.css';
import App from './App.svelte';
import { openExternalLink } from './lib/externalLinks.js';
import './app.css';

// Links in rendered markdown (chat answers, notes: `a.md-link`, made by
// lib/markdown.js) open in the default browser. Left alone, a click would
// navigate Yap's own window away from the app.
document.addEventListener('click', (e) => {
  const a = e.target instanceof Element ? e.target.closest('a.md-link') : null;
  if (!a) return;
  e.preventDefault();
  openExternalLink(a.getAttribute('href'));
});

const app = mount(App, { target: document.getElementById('app') });

export default app;
