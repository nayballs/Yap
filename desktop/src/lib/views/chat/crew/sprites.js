// **Yap's crew, drawn** (core/crew.js says who a model is): each character a
// pixel grid, 18 × 16 squares drawn 2 CSS px each (36 × 32 px; 4 px each at
// `size: 2`), an SVG of merged <rect>s with classes, never an image file.
// The colours are Yap's tokens (`--yap-crew-*` in app.css, light and dark),
// the motion CSS only (crew.css): nothing here runs on a timer. Pure — no
// Svelte, no Tauri — so the e2e suite draws the crew sheet with the very same
// markup (desktop/e2e/agents-panel.spec.js).
//
// How it moves follows savvy-progress's agents panel (MIT,
// github.com/JohnnyVizz/claude-kit): legs in two groups stepping out of phase
// on `steps(1)`, the body bobbing, a prop in a group of its own with its own
// keyframes, all of it stopped by reduced motion. The characters, their props
// and every pixel are Yap's own.
//
// A character's art: rows of letters, one a square —
//   b body, h its highlight, d its shade, m its mouth, t its speech-bubble
//   tail (body coloured, left out of its bounds), e / E its eyes (the upper
//   and lower square: done shuts the upper, needs looks up), a / A its
//   accent and the accent's shade, k ink, w white, g / G metal and its
//   shade, l glass, n wood, y / Y pencil, p eraser, s a faint mark, v a
//   visor's shine, q / Q the lead's cap and its peak, o its gold badge.
// Its feet: two 2 × 2 squares that step. Its shoulder: where a prop's arm
// starts. `t0`: one step's time (Haiku quick, Opus calm). `talk`: the Claude
// crew's mouth opening while it works (they're Yap's bubbles: they yap).

/** The grid, in squares. */
export const GRID_W = 18;
export const GRID_H = 16;

const CHARACTERS = {
  // **Sonnet**, the all-rounder: Yap's speech bubble, its tail at the lower left.
  sonnet: {
    y: 5,
    art: ['...hhbbbbbb', '..hhbbbbbbbd', '..hbebbbbebd', '..bbEbbbbEbd', '..bbbbmmbbbd', '..bbbbbbbbbd', '.tbbbbbbbbdd', 'tt.dddddddd'],
    feet: [
      [4, 13],
      [8, 13],
    ],
    shoulder: [12, 9],
    t0: 0.6,
    talk: [
      [6, 10],
      [7, 10],
    ],
  },
  // **Haiku**, small and quick: a smaller bubble with a leaf on top; it darts.
  haiku: {
    y: 6,
    art: ['......A', '....hhbbbb', '...hhbbbbbd', '...hbebbebd', '...bbEbbEbd', '..tbbbmmbdd', '.tt.dddddd'],
    fx: { f1: { y: 5, art: ['.......aa'] }, f2: { y: 4, art: ['........a', '.......a'] }, fd: '0.72s' },
    feet: [
      [4, 13],
      [8, 13],
    ],
    shoulder: [11, 10],
    t0: 0.36,
    talk: [
      [6, 12],
      [7, 12],
    ],
  },
  // **Opus**, bigger and wise: a tall bubble in round spectacles; it walks calmly, no bob.
  opus: {
    y: 3,
    art: [
      '..hhhbbbbbbb',
      '.hhbbbbbbbbbd',
      '.hbbbbbbbbbbd',
      '.bkkkkbbkkkkd',
      '.bkwekkkkewkd',
      '.bkwEkbbkEwkd',
      '.bbkkbbbbkkbd',
      '.bbbbbmmbbbbd',
      'tbbbbbbbbbbdd',
      't.dddddddddd',
    ],
    feet: [
      [3, 13],
      [9, 13],
    ],
    shoulder: [13, 8],
    t0: 0.9,
    calm: true,
    talk: [
      [6, 11],
      [7, 11],
    ],
  },
  // **Fable**, the storyteller: a bubble in a knitted scarf, its end streaming behind.
  fable: {
    y: 5,
    art: ['...hhbbbbbb', '..hhbbbbbbbd', '..hbebbbbebd', '..bbEbbbbEbd', '..bbbbmmbbbd', '.aaAaaAaaAaA', '.tbbbbbbbadd', 'tt.ddddddAd'],
    fx: { f1: { y: 10, art: ['A'] }, f2: { y: 9, art: ['A'] }, fd: '0.7s' },
    feet: [
      [4, 13],
      [8, 13],
    ],
    shoulder: [12, 9],
    t0: 0.7,
    talk: [
      [5, 9],
      [8, 9],
    ],
  },
  // **GPT**: a box with a screen for a face, lit eyes, an antenna.
  gpt: {
    y: 3,
    art: ['.......g', '.......g', '..hhhhhhhhhd', '..hkkkkkkkkd', '..bkekkkkekd', '..bkEkkkkEkd', '..bkkkmmkkkd', '..bkkkkkkkkd', '..bbbbbbbbbd', '..dddddddddd'],
    fx: { f1: { y: 2, art: ['.......a'] }, fd: '1s' },
    feet: [
      [3, 13],
      [9, 13],
    ],
    shoulder: [12, 9],
    t0: 0.6,
  },
  // **Gemini** (and Gemma): a tall oval with two bobbles.
  gemini: {
    y: 1,
    art: ['....a....a', '....A....A', '....A....A', '....AhhbbA', '....hhbbbd', '...hhbbbbbd', '...hbebbebd', '...bbEbbEbd', '...bbbmmbbd', '...bbbbbbbd', '....bbbbdd', '.....dddd'],
    feet: [
      [4, 13],
      [8, 13],
    ],
    shoulder: [11, 8],
    t0: 0.6,
  },
  // **Grok**: square, a visor across its eyes, a crest of spikes.
  grok: {
    y: 4,
    art: ['..aaaaaaaaaa', '..hbbbbbbbbd', '..bkkkkkkkkd', '..bkkekkekkd', '..bkkEkkEkkd', '..bbbbbbbbbd', '..bbbmmmmbbd', '..bbbbbbbbbd', '..dddddddddd'],
    fx: { f1: { y: 3, art: ['...a..a..a'] }, f2: { y: 3, art: ['....a..a..a'] }, fd: '0.5s' },
    feet: [
      [3, 13],
      [9, 13],
    ],
    shoulder: [12, 9],
    t0: 0.5,
  },
  // **Llama**: a woolly puff with a face in it.
  llama: {
    y: 4,
    art: ['...hh.hh.hh', '..hbbbbbbbbd', '.hbbbbbbbbbbd', '.bbaeaaaaeabd', '.bbaEaaaaEabd', '.bbaaammaaabd', '..bbaaaaaabd', '.hbbbbbbbbbbd', '..bbb.bb.bbb'],
    feet: [
      [4, 13],
      [8, 13],
    ],
    shoulder: [13, 9],
    t0: 0.7,
  },
  // **Qwen**: a drop, pointed at the top.
  qwen: {
    y: 3,
    art: ['......hb', '.....hhbb', '....hhbbbd', '...hhbbbbbd', '...hbebbebd', '..hbbEbbEbbd', '..bbbbmmbbbd', '..bbbbbbbbbd', '...bbbbbbdd', '....dddddd'],
    fx: { f1: { y: 5, art: ['.....w'] }, fd: '1.2s' },
    feet: [
      [4, 13],
      [8, 13],
    ],
    shoulder: [12, 9],
    t0: 0.6,
  },
  // **Mistral**: round, a curl of wind on its head.
  mistral: {
    y: 4,
    art: ['...a', '...hhbbbbbb', '..hhbbbbbbbd', '..hbebbbbebd', '..bbEbbbbEbd', '..bbbbmmbbbd', '..bbbbbbbbbd', '..bbbbbbbbdd', '...dddddddd'],
    fx: { f1: { y: 1, art: ['...aa', '..a..a', '..a.a'] }, f2: { y: 1, art: ['....aa', '...a..a', '...aa'] }, fd: '0.6s' },
    feet: [
      [4, 13],
      [8, 13],
    ],
    shoulder: [12, 9],
    t0: 0.5,
  },
  // **DeepSeek**: round, a periscope up out of its head.
  deepseek: {
    y: 1,
    art: ['........ggGl', '........g', '........g', '........g', '...hhbbbbbb', '..hhbbbbbbbd', '..hbebbbbebd', '..bbEbbbbEbd', '..bbbbmmbbbd', '..bbbbbbbbbd', '..bbbbbbbbdd', '...dddddddd'],
    fx: { f1: { y: 1, art: ['...........w'] }, fd: '0.9s' },
    feet: [
      [4, 13],
      [8, 13],
    ],
    shoulder: [12, 9],
    t0: 0.6,
  },
  // **The on-device model**: a chip with pins, a notch and a light that says it's on this PC.
  local: {
    y: 5,
    art: ['...hhhddhhh', '..ghbbbbbbdg', '...hbebbebd', '..gbbEbbEbd', '...bbbmmbbd', '..gbbbbbbbdg', '...bbbbbbbd', '...dddddddd'],
    fx: { f1: { y: 11, art: ['.........a'] }, fd: '0.8s' },
    feet: [
      [4, 13],
      [8, 13],
    ],
    shoulder: [11, 8],
    t0: 0.5,
  },
  // **Any other model**: a plain round bubble.
  generic: {
    y: 5,
    art: ['...hhbbbbbb', '..hhbbbbbbbd', '..hbebbbbebd', '..bbEbbbbEbd', '..bbbbmmbbbd', '..bbbbbbbbbd', '..bbbbbbbbdd', '...dddddddd'],
    feet: [
      [4, 13],
      [8, 13],
    ],
    shoulder: [12, 9],
    t0: 0.6,
  },
};

/** The characters there are, in the crew sheet's order. */
export const CHARACTER_IDS = Object.keys(CHARACTERS);

/**
 * **The Modes' props**, drawn from the shoulder (0, 0): `arm` stays put,
 * `art` moves (its keyframes in crew.css), `f1` / `f2` are its two frames
 * (f1 alone when still), `w1` / `w2` frames only while it works.
 */
const PROPS = {
  // Plan: a magnifying glass, swept; its glass glints.
  plan: {
    arm: { y: -1, art: ['.b', 'b'] },
    art: { y: -6, art: ['...GG', '..GllG', '..GllG', '...GG', '..n'] },
    f1: { y: -5, art: ['...w'] },
    fd: '0.6s',
  },
  // Auto: a racing flag on a pole, fluttering (and the character runs).
  auto: {
    arm: { y: -7, art: ['.g', '.g', '.g', '.g', '.g', '.g', '.g', 'bb', '.g'] },
    f1: { y: -7, art: ['..wkwk', '..kwkw', '..wkwk'] },
    f2: { y: -7, art: ['..wk', '..kwkw', '..wkwk', '....kw'] },
    fd: 'calc(var(--t) / 2)',
  },
  // Manual: a wrench, turning (the arm with it, from the shoulder).
  manual: {
    art: { y: -5, art: ['...G.G', '...GGG', '..gg', '..g', '.b', 'b'] },
    turns: true,
  },
  // Accept edits: a pencil, scribbling; its lines come and go.
  edits: {
    arm: { y: -1, art: ['.b', 'b'] },
    art: { y: -4, art: ['.pp', '..yY', '..byY', '....nn', '.....k'] },
    w1: { y: 1, art: ['....GG'] },
    w2: { y: 1, art: ['..GG'] },
    fd: '0.48s',
  },
  // Bypass permissions: a space helmet (drawn round the character), floating; puffs under its feet.
  bypass: { helmet: true, fd: '0.5s' },
};

/** The Modes' props, by id. */
export const PROP_IDS = Object.keys(PROPS);

/**
 * **The lead** (14 Nov 2026; the maintainer: the chat's own Claude, the one
 * that "organises agents", should look "like the leader… so it's clearly
 * different from the agent animations"): a Code chat's own Claude on its
 * working line in the thread, where the agents it sends walk in the Agent
 * panel with their Mode's tool. It stands and conducts: a captain's cap
 * ([`cap`]) and a baton beating up and down (f1 raised, f2 forward), on the
 * beat of `fd`. Not a Mode's prop (`prop: 'lead'`, never in [`PROP_IDS`]).
 */
const LEAD = {
  cap: true,
  f1: { y: -4, art: ['....w', '...k', '..k', '.b', 'b'] },
  f2: { y: -1, art: ['.....w', '..kkk', 'bb'] },
  fd: '0.7s',
};

/** Every square of an art block: `[{x, y, c}]`, at (`dx`, `dy`). */
function squares(block, dx = 0, dy = 0) {
  const out = [];
  if (!block) return out;
  block.art.forEach((row, i) => {
    [...row].forEach((c, x) => {
      if (c !== '.' && c !== ' ') out.push({ x: x + dx, y: block.y + i + dy, c });
    });
  });
  return out;
}

const COLOR = { t: 'b', e: 'e', E: 'e' };
/** A square's colour class (`pb`, `pe`…): the tail is body coloured, both eye squares the eye's. */
const cls = (c) => `p${COLOR[c] ?? c}`;

/** Squares as <rect>s, a row's same-coloured neighbours merged into one. */
function rects(list) {
  const sorted = [...list].sort((a, b) => a.y - b.y || a.x - b.x);
  let out = '';
  for (let i = 0; i < sorted.length; ) {
    const s = sorted[i];
    let w = 1;
    while (i + w < sorted.length && sorted[i + w].y === s.y && sorted[i + w].x === s.x + w && cls(sorted[i + w].c) === cls(s.c)) w += 1;
    out += `<rect x="${s.x}" y="${s.y}" width="${w}" height="1" class="${cls(s.c)}"/>`;
    i += w;
  }
  return out;
}

const group = (name, list, style = '') => (list.length ? `<g class="${name}"${style ? ` style="${style}"` : ''}>${rects(list)}</g>` : '');

/** The body's bounds (its tail and its ornaments left out): `{x0, y0, x1, y1}`. */
function boundsOf(list) {
  const body = list.filter((s) => 'bhdmeEk'.includes(s.c));
  return {
    x0: Math.min(...body.map((s) => s.x)),
    y0: Math.min(...body.map((s) => s.y)),
    x1: Math.max(...body.map((s) => s.x)),
    y1: Math.max(...body.map((s) => s.y)),
  };
}

/** **The space helmet**, round the body `b`: a dome of glass, its rim, a collar, a shine. */
function helmet(b) {
  const glass = [];
  const rim = [];
  const top = b.y0 - 2;
  for (let x = b.x0 + 1; x <= b.x1 - 1; x++) glass.push({ x, y: b.y0 - 1, c: 'l' });
  for (let y = b.y0; y <= b.y0 + 4; y++) for (let x = b.x0; x <= b.x1; x++) glass.push({ x, y, c: 'l' });
  for (let x = b.x0 + 1; x <= b.x1 - 1; x++) rim.push({ x, y: top, c: 'g' });
  rim.push({ x: b.x0, y: b.y0 - 1, c: 'g' }, { x: b.x1, y: b.y0 - 1, c: 'g' });
  for (let y = b.y0; y <= b.y0 + 4; y++) rim.push({ x: b.x0 - 1, y, c: 'g' }, { x: b.x1 + 1, y, c: 'g' });
  for (let x = b.x0 - 1; x <= b.x1 + 1; x++) rim.push({ x, y: b.y0 + 5, c: 'G' });
  const shine = [
    { x: b.x0 + 1, y: b.y0, c: 'v' },
    { x: b.x0, y: b.y0 + 1, c: 'v' },
  ];
  return `<g class="c-helmet">${rects(glass)}${rects(shine)}${rects(rim)}</g>`;
}

/** **The lead's captain's cap**, on the body `b`: a navy crown with a gold badge, its band and peak (forward, to the right) a shade darker. */
function cap(b) {
  const mid = Math.round((b.x0 + b.x1) / 2);
  const squaresOf = [];
  for (let x = b.x0 + 2; x <= b.x1 - 2; x++) squaresOf.push({ x, y: b.y0 - 3, c: 'q' });
  for (let x = b.x0 + 1; x <= b.x1 - 1; x++) squaresOf.push({ x, y: b.y0 - 2, c: x === mid ? 'o' : 'q' });
  for (let x = b.x0; x <= b.x1 + 2; x++) squaresOf.push({ x, y: b.y0 - 1, c: 'Q' });
  return `<g class="c-cap">${rects(squaresOf)}</g>`;
}

/** Auto's speed lines behind the body `b` (inside the grid). */
function speedLines(b) {
  const at = (y, from, to) => {
    const out = [];
    for (let x = Math.max(0, from); x <= to; x++) out.push({ x, y, c: 's' });
    return out;
  };
  const w1 = [...at(b.y0 + 2, b.x0 - 3, b.x0 - 2), ...at(b.y0 + 5, b.x0 - 2, b.x0 - 1)];
  const w2 = [...at(b.y0 + 3, b.x0 - 2, b.x0 - 1), ...at(b.y0 + 6, b.x0 - 3, b.x0 - 2)];
  return `<g class="c-speed" style="--fd: calc(var(--t) / 2)">${group('w1', w1)}${group('w2', w2)}</g>`;
}

/** Bypass's puffs under the feet, one foot's then the other's. */
function puffs(feet) {
  const [[ax], [bx]] = feet;
  const w1 = [{ x: ax, y: 15, c: 's' }, { x: ax + 1, y: 15, c: 's' }];
  const w2 = [{ x: bx, y: 15, c: 's' }, { x: bx + 1, y: 15, c: 's' }];
  return `<g class="c-puffs" style="--fd: ${PROPS.bypass.fd}">${group('w1', w1)}${group('w2', w2)}</g>`;
}

const foot = ([x, y]) => [
  { x, y, c: 'f' },
  { x: x + 1, y, c: 'f' },
  { x, y: y + 1, c: 'f' },
  { x: x + 1, y: y + 1, c: 'f' },
];

/** **A character's SVG** (`character`, its Mode's `prop`), every pose's parts in it: crew.css shows each pose. */
export function crewSvg(character, prop = 'auto') {
  const ch = CHARACTERS[character] ?? CHARACTERS.generic;
  const pr = prop === 'lead' ? LEAD : (PROPS[prop] ?? PROPS.auto);
  const body = squares(ch);
  const b = boundsOf(body);
  const [sx, sy] = ch.shoulder;
  const eyesUp = body.filter((s) => s.c === 'e');
  const eyesDown = body.filter((s) => s.c === 'E');
  const fx = ch.fx
    ? `<g class="c-cfx" style="--fd: ${ch.fx.fd}">${group('f1', squares(ch.fx.f1))}${group('f2', squares(ch.fx.f2))}</g>`
    : '';
  let held = '';
  if (pr.helmet) {
    held = helmet(b);
  } else {
    const origin = pr.turns ? `transform-origin: ${sx + 0.5}px ${sy + 0.5}px` : '';
    const frames = `${group('f1', squares(pr.f1, sx, sy))}${group('f2', squares(pr.f2, sx, sy))}${group('w1', squares(pr.w1, sx, sy))}${group('w2', squares(pr.w2, sx, sy))}`;
    held =
      group('c-arm', squares(pr.arm, sx, sy)) +
      `<g class="c-prop"${origin ? ` style="${origin}"` : ''}>${rects(squares(pr.art, sx, sy))}${frames ? `<g class="c-pfx" style="--fd: ${pr.fd}">${frames}</g>` : ''}</g>`;
  }
  return (
    `<svg viewBox="0 0 ${GRID_W} ${GRID_H}" shape-rendering="crispEdges" aria-hidden="true" focusable="false">` +
    `<g class="c-all">` +
    (prop === 'auto' ? speedLines(b) : '') +
    (pr.helmet ? puffs(ch.feet) : '') +
    group('c-fl', foot(ch.feet[0])) +
    group('c-fr', foot(ch.feet[1])) +
    `<g class="c-bod">${rects(body.filter((s) => s.c !== 'e' && s.c !== 'E'))}` +
    `<g class="c-eyes">${group('c-eu', eyesUp)}${group('c-el', eyesDown)}</g>` +
    fx +
    (ch.talk ? `<g class="c-talk" style="--fd: 0.5s">${group('w2', ch.talk.map(([x, y]) => ({ x, y, c: 'm' })))}</g>` : '') +
    held +
    (pr.cap ? cap(b) : '') +
    `</g></g></svg>`
  );
}

const esc = (s) => String(s).replace(/&/g, '&amp;').replace(/"/g, '&quot;').replace(/</g, '&lt;').replace(/>/g, '&gt;');

const BADGES = {
  done: '<span class="badge done" aria-hidden="true">✓</span>',
  failed: '<span class="badge failed" aria-hidden="true">×</span>',
  needs: '<span class="bubble" aria-hidden="true">!</span>',
};

const cache = new Map();

/**
 * **An avatar's markup**: the character `character` holding its Mode's
 * `prop`, in `pose` (working, needs, done, stopped, failed, queued), with
 * its badge; `label` its accessible name ("Opus, working, Auto mode");
 * `size` 2 for twice as big (the crew sheet), 0.5 for a line of text (the
 * lead on a Code chat's working line). The same string for the same
 * arguments, so a row redrawn every second never restarts its steps.
 */
export function crewMarkup({ character = 'sonnet', prop = 'auto', pose = 'working', label = '', size = 1, testid = '' } = {}) {
  const id = CHARACTERS[character] ? character : 'generic';
  const p = PROPS[prop] || prop === 'lead' ? prop : 'auto';
  const key = [id, p, pose, label, size, testid].join('|');
  const hit = cache.get(key);
  if (hit) return hit;
  const ch = CHARACTERS[id];
  const classes = ['crew', `ch-${id}`, `pr-${p}`, `po-${pose}`, ch.calm ? 'calm' : '', size === 2 ? 'size-2' : size === 0.5 ? 'size-half' : ''].filter(Boolean).join(' ');
  const html =
    `<span class="${classes}" role="img" aria-label="${esc(label)}" data-character="${id}" data-prop="${p}" data-pose="${esc(pose)}" style="--t0: ${ch.t0}s"` +
    `${testid ? ` data-testid="${esc(testid)}"` : ''}>${crewSvg(id, p)}${BADGES[pose] ?? ''}</span>`;
  if (cache.size > 400) cache.clear();
  cache.set(key, html);
  return html;
}
