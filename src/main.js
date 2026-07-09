import { mount } from 'svelte';
// Bundled typefaces — the exact pair Wispr Flow ships (both OFL/Google Fonts):
// Figtree = UI sans, EB Garamond (+italic) = display serif. Segoe UI fallback.
import '@fontsource-variable/figtree';
import '@fontsource-variable/eb-garamond';
import '@fontsource-variable/eb-garamond/wght-italic.css';
import App from './App.svelte';
import './app.css';

const app = mount(App, { target: document.getElementById('app') });

export default app;
