// **Maths, drawn** (15 Nov 2026): KaTeX (MIT) in a chunk of its own, loaded
// by ui/markdownVisuals.js the first time an answer, a note or a document
// shows maths (core/markdown.js `span.md-math`). Never in the main chunk.
//
// - A formula as KaTeX's HTML (and MathML, for a screen reader), with no
//   trust (`\href`, `\url`, `\includegraphics`, `\htmlStyle` stay off) and
//   limits on sizes and macro expansion; then sanitised again
//   (core/visualSafety.js `cleanMaths`). One that KaTeX can't read stays
//   as the model wrote it.
// - A copy of drawn maths gives its TeX back (`$…$`: KaTeX's own copy-tex).
import katex from 'katex';
import 'katex/dist/katex.min.css';
import 'katex/contrib/copy-tex';
import { cleanMaths } from '../core/visualSafety.js';

/** Formulas drawn, by display and TeX (most answers draw the same ones again as they're written). */
const drawn = new Map();

/** A formula's safe HTML, or `null`: KaTeX couldn't read it. */
export function mathsHtml(tex, display) {
  const key = `${display ? 'D' : 'I'}${tex}`;
  if (drawn.has(key)) return drawn.get(key);
  let html = null;
  try {
    html = cleanMaths(
      katex.renderToString(tex, {
        displayMode: !!display,
        throwOnError: true,
        trust: false,
        strict: 'ignore',
        output: 'htmlAndMathml',
        maxSize: 40,
        maxExpand: 500,
      })
    );
  } catch {
    html = null;
  }
  if (drawn.size > 500) drawn.clear();
  drawn.set(key, html);
  return html;
}
