// Chat's screenshots at a real 150 % scaling: WebView2 drawn at a device
// scale factor of 1.5 (`--force-device-scale-factor`, as on a 150 % monitor),
// the same tests as chat-shots.spec.js (support/chat-shots.js). Here a 200 px
// snip is 133 CSS px wide in a question, the box's tile holds a 180 × 101
// thumbnail, and the ring is two thirds of a CSS pixel: one device pixel.
// Skipped where WebView2 ignores WEBVIEW2_* variables (an elevated run: CI's
// runners), which leaves it at the monitor's scaling.
import { shotTests } from './support/chat-shots.js';

shotTests(1.5);
