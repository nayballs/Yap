// **File chips** (16 Nov 2026, after the Claude app's: a type badge, the file's name, its size): what a chip says,
// which colour its badge takes, how a long name is cut so its ending shows, and which file has its one chip in a
// turn. One look for every file Chat and Code chats show as a box: a file shared into the conversation
// (SharedFiles.svelte, `share_files`), an agent's report's new files (AgentReportCard.svelte), a question's
// attachments (FileChip.svelte), and the phone's (Yap Link's page imports it as core/fileChip.js: build.rs
// `LINK_SHARED`). Pure — no imports — so the phone can load it and the e2e suite checks it without the app.

/** A file's ending, lower case ('' for none): "report.pdf.exe" → "exe". */
export function endingOf(name) {
  const base = String(name ?? '').split(/[\\/]/).pop() || '';
  const dot = base.lastIndexOf('.');
  return dot > 0 ? base.slice(dot + 1).toLowerCase() : '';
}

/** A kind with no ending to go by (Rust's `kind_of`, an attachment's kind): its badge. */
const KIND_BADGE = { pdf: 'PDF', word: 'DOC', powerpoint: 'PPT', markdown: 'MD', code: 'CODE', sheet: 'XLS', image: 'IMG', text: 'TXT', file: 'FILE' };

/** **The badge**: the file's ending in capitals, at most four letters ("MD", "XLSX", "HTML"), else its kind's. */
export function badgeOf(file) {
  const e = endingOf(file?.name || file?.path);
  if (e && e.length <= 4 && /^[a-z0-9]+$/.test(e)) return e.toUpperCase();
  if (e === 'markdown') return 'MD';
  return KIND_BADGE[file?.kind] || 'TXT';
}

/** **Its size** as the Claude app's chips say it: "572B", "2.1 KB", "14 KB", "1.4 MB". */
export function fileSize(bytes) {
  const b = Math.max(0, Number(bytes) || 0);
  if (b < 1024) return `${b}B`;
  const kb = b / 1024;
  if (kb < 1024) return `${kb < 10 ? kb.toFixed(1) : Math.round(kb)} KB`;
  const mb = kb / 1024;
  if (mb < 1024) return `${mb < 10 ? mb.toFixed(1) : Math.round(mb)} MB`;
  const gb = mb / 1024;
  return `${gb < 10 ? gb.toFixed(1) : Math.round(gb)} GB`;
}

/** Endings by the family their badge's colour says (app.css `--yap-file-*`, light and dark). */
const FAMILIES = {
  doc: ['md', 'markdown', 'mdx', 'docx', 'doc', 'docm', 'odt', 'rtf', 'pages', 'tex'],
  pdf: ['pdf'],
  sheet: ['xlsx', 'xlsm', 'xls', 'xlsb', 'ods', 'csv', 'tsv', 'numbers'],
  slides: ['pptx', 'ppt', 'pptm', 'odp', 'key'],
  picture: ['png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp', 'svg', 'ico', 'heic', 'heif', 'tif', 'tiff', 'avif'],
  code: [
    'js', 'mjs', 'cjs', 'ts', 'mts', 'tsx', 'jsx', 'py', 'rs', 'go', 'java', 'kt', 'c', 'h', 'cpp', 'hpp', 'cc', 'cs', 'rb', 'php', 'sh', 'ps1', 'bat', 'cmd',
    'json', 'jsonc', 'yaml', 'yml', 'toml', 'ini', 'html', 'htm', 'css', 'scss', 'less', 'svelte', 'vue', 'sql', 'xml', 'lua', 'swift', 'dart', 'zig', 'r', 'lock',
  ],
  archive: ['zip', '7z', 'rar', 'tar', 'gz', 'tgz', 'bz2', 'xz'],
  media: ['mp3', 'wav', 'flac', 'm4a', 'ogg', 'opus', 'aac', 'mp4', 'mov', 'mkv', 'webm', 'avi'],
};
const FAMILY_OF = new Map(Object.entries(FAMILIES).flatMap(([family, endings]) => endings.map((e) => [e, family])));
/** A kind with no ending we know: its family. */
const KIND_FAMILY = { markdown: 'doc', word: 'doc', pdf: 'pdf', sheet: 'sheet', powerpoint: 'slides', image: 'picture', code: 'code' };

/** **Its badge's colour**: "doc", "pdf", "sheet", "slides", "picture", "code", "archive", "media" or "text" (the rest). */
export function familyOf(file) {
  return FAMILY_OF.get(endingOf(file?.name || file?.path)) || KIND_FAMILY[file?.kind] || 'text';
}

/**
 * **A name cut in the middle**, so its ending always shows: `{head, tail}` — the chip lets `head` give way with
 * "…" and keeps `tail` (the ending and the few letters before it, at least `keep`) whole. A name no longer than
 * that is all `tail` (`head` ''): nothing to cut.
 */
export function nameParts(name, keep = 10) {
  const n = String(name ?? '');
  const dot = n.lastIndexOf('.');
  const ending = dot > 0 ? n.length - dot : 0;
  const k = Math.min(n.length, Math.max(keep, ending + 4));
  if (n.length <= k) return { head: '', tail: n };
  // (The kept end starts at a word where one is near: "…chat-turns.md", not "…t-turns.md".)
  let cut = n.length - k;
  for (let i = cut - 1; i >= Math.max(1, cut - 8); i--) {
    if (/[-_ .]/.test(n[i])) {
      cut = i + 1;
      break;
    }
  }
  return { head: n.slice(0, cut), tail: n.slice(cut) };
}

/** A path as compared (Rust's `agents::workspaces::norm`): `\`, no trailing one, lower case. */
export function normPath(p) {
  return String(p ?? '')
    .trim()
    .replace(/^"+|"+$/g, '')
    .replace(/\//g, '\\')
    .replace(/\\+$/, '')
    .toLowerCase();
}

/** Every file a message's shares hold (`files`, else as they come: `liveFiles`). */
const sharesOf = (m) => (m?.files?.length ? m.files : m?.liveFiles || []);

/** A user message that starts a turn: the person's own (an agent's report marker is a `user` message too, never theirs). */
const startsTurn = (m) => m?.role === 'user' && m.kind !== 'agentReport';

/**
 * **One chip per file per turn** (16 Nov 2026: the same file showed as a card in the work lines and again under
 * the answer): for each agent's report in `messages` (Chat's thread), the files its card leaves to a chip
 * elsewhere in the turn — a file an answer in the same turn shares (that's its one chip, under the answer), or
 * one an earlier report of the turn shows already. `Map<index, Set<normPath>>`, only reports with some. A turn runs
 * from the person's message to their next.
 */
export function chippedElsewhere(messages) {
  const list = Array.isArray(messages) ? messages : [];
  const out = new Map();
  let start = 0;
  for (let i = 0; i <= list.length; i++) {
    if (i < list.length && !(i > start && startsTurn(list[i]))) continue;
    // The turn [start, i): its answers' files first, then each report in order.
    const answered = new Set();
    for (let k = start; k < i; k++) {
      if (list[k]?.role !== 'assistant') continue;
      for (const s of sharesOf(list[k])) for (const it of s?.items || []) if (it?.path) answered.add(normPath(it.path));
    }
    const shown = new Set();
    for (let k = start; k < i; k++) {
      const m = list[k];
      if (m?.kind !== 'agentReport') continue;
      const here = new Set();
      for (const s of sharesOf(m)) {
        for (const it of s?.items || []) {
          if (!it?.path) continue;
          const p = normPath(it.path);
          if (answered.has(p) || shown.has(p)) here.add(p);
          else shown.add(p);
        }
      }
      if (here.size) out.set(k, here);
    }
    start = i;
  }
  return out;
}

/**
 * **A share's files, each once** (`skip`: paths shown as a chip elsewhere in the turn, `seen`: those already
 * drawn by an earlier share of the same message — both normalised; `seen` grows). A file with no path (a picture
 * from the web) is always itself.
 */
export function onceEach(items, { skip = null, seen = null } = {}) {
  const out = [];
  for (const it of items || []) {
    if (!it) continue;
    const p = it.path ? normPath(it.path) : '';
    if (p && (skip?.has(p) || seen?.has(p))) continue;
    if (p) seen?.add(p);
    out.push(it);
  }
  return out;
}

/**
 * **A report's "Changed …" line, its files as links** (AgentReportCard): `changed` (core/agents.js
 * `reportChanged`: names and paths joined with ", ") split into its parts, each a file the card's `items` hold
 * (its path ending in the part, as written: "notes.md", "data\beds.csv", a full path) given as `{text, item}`, the
 * rest as `{text}` — commas kept as text. `extra`: files to name at its end as links too (one the line doesn't
 * name, whose chip is elsewhere in the turn). Pure.
 */
export function changedParts(changed, items, extra = []) {
  const text = String(changed ?? '');
  const files = (items || []).filter((it) => it?.path);
  const parts = [];
  const used = new Set();
  const pieces = text ? text.split(', ') : [];
  pieces.forEach((piece, i) => {
    if (i) parts.push({ text: ', ' });
    const m = /^(.*?)(\s+and \d+ more)?$/.exec(piece);
    const name = (m?.[1] ?? piece).trim();
    const want = normPath(name);
    const item = want ? files.find((it) => !used.has(it) && (normPath(it.path) === want || normPath(it.path).endsWith(`\\${want}`))) : null;
    if (item) {
      used.add(item);
      parts.push({ text: name, item });
      if (m?.[2]) parts.push({ text: m[2] });
    } else parts.push({ text: piece });
  });
  for (const it of extra || []) {
    if (!it?.path || used.has(it)) continue;
    if (parts.length) parts.push({ text: ', ' });
    parts.push({ text: it.name || it.path, item: it });
    used.add(it);
  }
  return parts;
}
