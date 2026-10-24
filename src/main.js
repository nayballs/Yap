import { mount } from 'svelte';
// Bundled typefaces — the exact pair Wispr Flow ships (both OFL/Google Fonts):
// Figtree = UI sans, EB Garamond (+italic) = display serif. Segoe UI fallback.
import '@fontsource-variable/figtree';
import '@fontsource-variable/eb-garamond';
import '@fontsource-variable/eb-garamond/wght-italic.css';
// The reading fonts (Settings → General → Appearance, lib/appearance.js; all
// OFL-1.1): Geist (Vercel) = "Sans", Literata (+italic; TypeTogether for
// Google Play Books, picked over Source Serif 4 and Newsreader as the most
// readable at 12–15 px) = "Serif"; OpenDyslexic's @font-face rules are in
// app.css (woff2 only). Bundled, never fetched from the internet; a font's
// files load only once some text uses it (and only the scripts it shows).
import '@fontsource-variable/geist';
import '@fontsource-variable/literata/wght.css';
import '@fontsource-variable/literata/wght-italic.css';
import App from './App.svelte';
import { openExternalLink } from './lib/core/externalLinks.js';
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
