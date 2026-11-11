// **Code colours** for the rows of a Code chat's thread (11 Nov 2026, after
// Claude Code's own transcript, whose diffs and commands are coloured; the
// maintainer: "Yes, add them"): keywords, strings, comments, numbers and names
// in an edit's diff and in a command, in Yap's own colours (app.css
// `--yap-syn-*`, light and dark).
//
// - **The highlighter** is highlight.js (BSD-3-Clause), its core and the
//   languages below, in a chunk of its own (`codeColours.hljs.js`) loaded the
//   first time a row is opened (`loadColours`): Chat never waits for it, and
//   a row shows plain text until it's there.
// - **Which language**: a file's by its name (`languageOf`), a command's by
//   how it's written (`shellOf`: PowerShell's cmdlets, else bash). A file Yap
//   has no language for stays plain: nothing is guessed.
// - **No HTML into the page**: highlight.js gives HTML (nested spans, its text
//   escaped). `tokensOf` reads that into `[{ text, kind }]` and the row draws
//   each as a text node in a span, so nothing of a file or a command is ever
//   set as markup.
// - **A diff keeps its sense across lines** (`colourDiff`): each part's lines
//   as they are after the edit are coloured together, and its removed lines
//   with the lines round them as they were before, so a comment or a string
//   that runs over several lines is still one.
//
// Chat's answers use the same highlighter for their fenced code
// (ui/colourCode.js, `fenceLanguage`).
//
// Pure but for `loadColours`. Never used by the phone's page.

/** A file's ending → its highlight.js language. */
const BY_ENDING = {
  js: 'javascript',
  mjs: 'javascript',
  cjs: 'javascript',
  jsx: 'javascript',
  ts: 'typescript',
  mts: 'typescript',
  cts: 'typescript',
  tsx: 'typescript',
  json: 'json',
  jsonc: 'json',
  jsonl: 'json',
  html: 'xml',
  htm: 'xml',
  xml: 'xml',
  svg: 'xml',
  svelte: 'xml',
  vue: 'xml',
  css: 'css',
  scss: 'scss',
  less: 'less',
  md: 'markdown',
  markdown: 'markdown',
  sh: 'bash',
  bash: 'bash',
  zsh: 'bash',
  ps1: 'powershell',
  psm1: 'powershell',
  bat: 'dos',
  cmd: 'dos',
  py: 'python',
  rs: 'rust',
  go: 'go',
  java: 'java',
  kt: 'kotlin',
  kts: 'kotlin',
  swift: 'swift',
  c: 'c',
  h: 'c',
  cpp: 'cpp',
  cc: 'cpp',
  hpp: 'cpp',
  cs: 'csharp',
  lua: 'lua',
  rb: 'ruby',
  php: 'php',
  sql: 'sql',
  yml: 'yaml',
  yaml: 'yaml',
  toml: 'ini',
  ini: 'ini',
  cfg: 'ini',
  diff: 'diff',
  patch: 'diff',
};
/** A whole file name → its language. */
const BY_NAME = { dockerfile: 'dockerfile', makefile: 'makefile' };

/** Every language the rows may ask for (`codeColours.hljs.js` registers exactly these). */
export const LANGUAGES = [...new Set([...Object.values(BY_ENDING), ...Object.values(BY_NAME)])].sort();

/** The most characters coloured at once (a longer part stays plain). */
export const COLOUR_MAX = 60_000;

/** A file's language by its name ("src\\app.ts" → "typescript", "Dockerfile" → "dockerfile"), or `null`: no colours for it. */
export function languageOf(file) {
  const name = String(file || '').split(/[\\/]/).pop().toLowerCase();
  if (!name) return null;
  if (BY_NAME[name]) return BY_NAME[name];
  const dot = name.lastIndexOf('.');
  return dot > 0 ? (BY_ENDING[name.slice(dot + 1)] ?? null) : null;
}

/** A fence's names for a language the highlighter calls something else (its own short names — js, ts, py, rs, sh, yml, html, md… — it knows itself). */
const FENCE = { shell: 'bash', console: 'bash', terminal: 'bash', svelte: 'xml', vue: 'xml', jsonc: 'json', jsonl: 'json', ps: 'powershell', pwsh: 'powershell', bat: 'dos', cmd: 'dos', toml: 'ini', 'c++': 'cpp', 'c#': 'csharp' };

/**
 * The language a rendered code block names (core/markdown.js: a class on its
 * `code`, `lang-js`), as the highlighter is asked for it; `null` for none.
 * Whether the highlighter has it is `colourLines`' to say.
 */
export function fenceLanguage(className) {
  const name = String(className || '').split(/\s+/).find((c) => c.startsWith('lang-'))?.slice(5).toLowerCase();
  if (!name) return null;
  return FENCE[name] ?? name;
}

/** A command's language by how it's written: PowerShell's (a cmdlet, `$env:`, `$_`, `-ErrorAction`), else bash. */
export function shellOf(command) {
  const c = String(command || '');
  return /\b(Get|Set|New|Remove|Select|Where|ForEach|Test|Start|Stop|Write|Out|Invoke|Import|Export|Copy|Move|Add|Clear|Rename|Join|Split|Resolve|Convert|ConvertTo|ConvertFrom|Measure|Sort|Compare)-[A-Z][A-Za-z]+\b|\$env:|\$_\b|\s-ErrorAction\b/.test(c)
    ? 'powershell'
    : 'bash';
}

/** highlight.js's scope → Yap's colour kind: k keyword, s string, c comment, n number, f a function's or section's name, t a type or built-in, a an attribute, property or variable, r a regular expression or meta, b bold, i italic. */
const KINDS = {
  keyword: 'k',
  'selector-tag': 'k',
  doctag: 'k',
  string: 's',
  char: 's',
  quote: 's',
  addition: 's',
  comment: 'c',
  number: 'n',
  literal: 'n',
  title: 'f',
  section: 'f',
  'selector-id': 'f',
  'selector-class': 'f',
  type: 't',
  built_in: 't',
  attr: 'a',
  attribute: 'a',
  property: 'a',
  name: 'a',
  variable: 'a',
  'template-variable': 'a',
  symbol: 'a',
  bullet: 'a',
  regexp: 'r',
  meta: 'r',
  link: 'r',
  deletion: 'r',
  strong: 'b',
  emphasis: 'i',
};
/** Scopes whose text is plain again whatever is round them (`${x}` in a template string). */
const PLAIN = new Set(['subst']);

/** The colour kind of text inside these spans' classes (outermost first, as highlight.js nests them): the innermost that has one; '' for plain. */
export function kindOf(classes) {
  for (let i = classes.length - 1; i >= 0; i--) {
    const scope = String(classes[i]).split(/\s+/).find((c) => c.startsWith('hljs-'))?.slice(5);
    if (!scope) continue;
    if (PLAIN.has(scope)) return '';
    if (KINDS[scope]) return KINDS[scope];
  }
  return '';
}

const ENTITIES = { '&amp;': '&', '&lt;': '<', '&gt;': '>', '&quot;': '"', '&#x27;': "'", '&#39;': "'" };
const unescaped = (s) => s.replace(/&(amp|lt|gt|quot|#x27|#39);/g, (m) => ENTITIES[m]);

/**
 * **highlight.js's HTML as tokens**: `[{ text, kind }]` in order, neighbours
 * of one kind joined. Its HTML is spans with a class and escaped text and
 * nothing else; anything it didn't expect is kept as text, never dropped.
 */
export function tokensOf(html) {
  const out = [];
  const stack = [];
  const push = (text, kind) => {
    if (!text) return;
    const last = out[out.length - 1];
    if (last && last.kind === kind) last.text += text;
    else out.push({ text, kind });
  };
  for (const m of String(html || '').matchAll(/<span class="([^"]*)">|<\/span>|([^<]+)|(<)/g)) {
    if (m[1] != null) stack.push(m[1]);
    else if (m[2] != null) push(unescaped(m[2]), kindOf(stack));
    else if (m[3] != null) push('<', kindOf(stack));
    else stack.pop();
  }
  return out;
}

/** Tokens as lines: `[[{ text, kind }]]`, a token that runs over a line end cut there (an empty line is `[]`). */
export function lineTokens(tokens) {
  const lines = [[]];
  for (const t of tokens || []) {
    const parts = String(t.text).split('\n');
    parts.forEach((p, i) => {
      if (i) lines.push([]);
      if (p) lines[lines.length - 1].push({ text: p, kind: t.kind });
    });
  }
  return lines;
}

/**
 * **`code` coloured, line by line** (`hljs`: the highlighter, `loadColours`):
 * `[[{ text, kind }]]`, or `null` — no language, one the highlighter wasn't
 * given, too long, or it couldn't (the row stays plain).
 */
export function colourLines(hljs, code, language) {
  const text = String(code ?? '');
  if (!hljs || !language || !hljs.getLanguage?.(language) || text.length > COLOUR_MAX) return null;
  try {
    const lines = lineTokens(tokensOf(hljs.highlight(text, { language, ignoreIllegals: true }).value));
    // (Its text must come back as it went in: else plain.)
    return lines.length === text.split('\n').length ? lines : null;
  } catch {
    return null;
  }
}

/**
 * **A diff's rows with their colours** (`rows`: core/agents.js `diffRows`):
 * the same rows, each with `tokens` when its part could be coloured. Each
 * part (between two gaps) twice: its lines as they are after the edit
 * (unchanged and added) together, and, for its removed lines, as they were
 * before (unchanged and removed).
 */
export function colourDiff(hljs, rows, language) {
  const out = (rows || []).map((r) => ({ ...r }));
  if (!hljs || !language) return out;
  const part = (from, to) => {
    const lines = [];
    for (let i = from; i < to; i++) if (out[i].mark !== 'gap') lines.push(i);
    const after = lines.filter((i) => out[i].mark !== 'del');
    const before = lines.filter((i) => out[i].mark !== 'add');
    const coloured = (which) => colourLines(hljs, which.map((i) => out[i].text).join('\n'), language);
    const a = after.length ? coloured(after) : null;
    if (a) after.forEach((i, n) => (out[i].tokens = a[n]));
    const b = before.some((i) => out[i].mark === 'del') ? coloured(before) : null;
    if (b) before.forEach((i, n) => out[i].mark === 'del' && (out[i].tokens = b[n]));
  };
  let from = 0;
  out.forEach((r, i) => {
    if (r.mark !== 'gap') return;
    part(from, i);
    from = i + 1;
  });
  part(from, out.length);
  return out;
}

let loading = null;

/** The highlighter, loaded once (its own chunk): a promise of it. A failed load is tried again next time. */
export function loadColours() {
  loading ??= import('./codeColours.hljs.js')
    .then((m) => m.default)
    .catch((e) => {
      loading = null;
      throw e;
    });
  return loading;
}
