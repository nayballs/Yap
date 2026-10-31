// Chat's screenshots, drawn the way the Claude app draws them (31 Oct 2026,
// docs/architecture/snip.md "How a screenshot looks"), at 100 % here and at
// a real 150 % in chat-shots-150.spec.js (the tests: support/chat-shots.js).
//
// - The box's tile: a 120 px square, the whole picture in it (never
//   cropped) at exactly its device pixels — Rust's thumbnail
//   (snip/thumbs.rs), kept in chat-images/thumbs/ — on the frame's fill,
//   the ring laid over its edge one device pixel wide (white at 20 % in the
//   dark theme: the "white tinge"; warm ink at 16 % in the light), read back
//   from the screen's own pixels; ✕ and the size on hover, out over the
//   corner.
// - A question's pictures over its bubble: a small snip at its own pixels
//   (the picture itself), a screen as a thumbnail at its device pixels, at
//   most 480 × 320 (240 wide each of several), never under a tile.
// - The right-click menu still opens, copies and saves (the tile, a sent
//   picture, the viewer); the viewer shows a small picture 1:1 and a screen
//   as big as the window lets it, at its device pixels.
// - Measured: a made-up screenshot of a dark editor (support/sharpness.js)
//   drawn in the box, in a question and opened large, against its ideal
//   area-averaged copy at the same device pixels — a mean difference under
//   1 and the same edge contrast. The numbers and enlarged crops go to
//   test-results/app/screenshots/chat-shots/ (SHOTS_LABEL=before with
//   YAP_E2E_EXE pointing at an older build measures that one).
// - Named screenshots at the device's own pixels, light and dark.
import { shotTests } from './support/chat-shots.js';

shotTests(1);
