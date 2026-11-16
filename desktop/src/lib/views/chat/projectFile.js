// **A big project's file at a part's place** (16 Nov 2026; chat/projects/sources.rs): what a source's pill or its row
// under an answer opens beside the chat, read only, and how the file view draws it (ProjectFileTab.svelte). Pure.
//
// - **Which view** (`partView`): a spreadsheet (a workbook, a CSV) in the sheet view, as Excel draws it, at its sheet
//   and the part's first row (SheetTab's `at`); any other file as Yap read it — the text the model was given — in the
//   project file view, at the part's lines (`from`…`to`), marked.
// - **How the file view draws it** (`blocksOf`): a PDF page by page (each `[Page n]` a page's heading), a
//   presentation slide by slide, a Word document's paragraphs with its heading styles as headings, a text file's
//   paragraphs, code as numbered lines. Markdown is drawn by the document panel's own renderer (core/markdown.js,
//   its blocks' lines kept), so a heading, a table and a list look as they do in Chat's documents.

/** The kinds drawn as Markdown (core/markdown.js), and as numbered lines. */
const MARKDOWN = new Set(['markdown']);
const LINES = new Set(['code', 'sheet']);

/** How kind `kind` is drawn: 'markdown', 'lines', else 'blocks'. */
export function drawnAs(kind) {
  return MARKDOWN.has(kind) ? 'markdown' : LINES.has(kind) ? 'lines' : 'blocks';
}

/**
 * **Where a part's source opens** (`s`: a chats.rs `Source` with its `project` place): `{sheet}` for the sheet view
 * (`{src, name, at: {sheet, row}}`), `{file}` for the project file view (`{project, file, name, kind, from, to, at,
 * place, section}`); `null` for one with no place.
 */
export function partView(s) {
  const p = s?.project;
  if (!p?.project || !p?.file) return null;
  if (p.kind === 'sheet') {
    return { sheet: { src: { from: 'project', project: p.project, file: p.file }, name: s.title || '', at: { sheet: p.sheet || '', row: p.at ?? null } } };
  }
  return {
    file: {
      project: p.project,
      file: p.file,
      name: s.title || '',
      kind: p.kind || 'text',
      from: Number(p.from) || 1,
      to: Math.max(Number(p.to) || 1, Number(p.from) || 1),
      at: p.at ?? null,
      place: s.site || '',
      section: p.section || '',
    },
  };
}

/** A section's own heading: the last of its path ("Leave › Annual leave" → "Annual leave"). */
export const headingOf = (section) => String(section || '').split(' › ').pop().trim();

/** **The place in the view's header**: "Page 12", "Slides 3–5", "Lines 40–52", else its section's heading in quotes. */
export function placeLabel(ref) {
  const place = String(ref?.place || '').trim();
  if (place && !place.startsWith('“') && !/^part \d+$/.test(place)) return place.charAt(0).toUpperCase() + place.slice(1);
  const h = headingOf(ref?.section);
  return h ? `“${h}”` : place;
}

/** What the view says the text is, under its header. */
export function textNote(kind) {
  switch (kind) {
    case 'pdf':
      return 'The PDF’s text as Yap read it on this PC, page by page.';
    case 'powerpoint':
      return 'Each slide’s text and speaker notes, as Yap read them on this PC.';
    case 'word':
      return 'The document’s text as Yap read it on this PC.';
    default:
      return 'As Yap read it on this PC.';
  }
}

const PAGE = /^\[Page (\d+)(, read with OCR)?\]$/;
const SLIDE = /^Slide (\d+)( \(hidden\))?:(.*)$/;
const HEADING = /^(#{1,6})\s+(.+?)\s*#*\s*$/;

/**
 * **A file's text as blocks to read** (`kind`: its reader's), each with its first and last line (from 1, as a part's
 * `lines` count them): `{kind: 'page', n, ocr}`, `{kind: 'slide', n, text, hidden}`, `{kind: 'heading', level,
 * text}`, `{kind: 'para', text}` (its lines as the file has them), `{kind: 'note', text}` (a slide's speaker notes),
 * `{kind: 'line', n, text}` (code). Blank lines end a paragraph.
 */
export function blocksOf(text, kind) {
  const lines = String(text || '').replace(/\r\n?/g, '\n').split('\n');
  const out = [];
  if (LINES.has(kind)) {
    lines.forEach((t, i) => out.push({ kind: 'line', n: i + 1, text: t, from: i + 1, to: i + 1 }));
    if (out.length > 1 && out.at(-1).text === '') out.pop();
    return out;
  }
  let para = null;
  const end = () => {
    if (para) out.push(para);
    para = null;
  };
  // A Word document's paragraphs are a line each: each its own.
  const lineParas = kind === 'word';
  let notes = false;
  lines.forEach((raw, i) => {
    const at = i + 1;
    const t = raw.trimEnd();
    if (!t.trim()) {
      end();
      return;
    }
    let m;
    if (kind === 'pdf' && (m = PAGE.exec(t.trim()))) {
      end();
      out.push({ kind: 'page', n: Number(m[1]), ocr: !!m[2], from: at, to: at });
      return;
    }
    if (kind === 'powerpoint' && (m = SLIDE.exec(t))) {
      end();
      notes = false;
      const title = m[3].trim() === '(no text)' ? '' : m[3].trim();
      out.push({ kind: 'slide', n: Number(m[1]), text: title, hidden: !!m[2], from: at, to: at });
      return;
    }
    if ((kind === 'word' || kind === 'text') && (m = HEADING.exec(t))) {
      end();
      out.push({ kind: 'heading', level: m[1].length, text: m[2], from: at, to: at });
      return;
    }
    if (kind === 'powerpoint' && /^Speaker notes:/i.test(t.trim())) {
      end();
      notes = true;
    }
    const as = notes ? 'note' : 'para';
    if (para && (lineParas || para.kind !== as)) end();
    if (para) {
      para.text += `\n${t}`;
      para.to = at;
    } else {
      para = { kind: as, text: t, from: at, to: at };
    }
  });
  end();
  return out;
}

/** Whether block `b` (its lines) is in the part's lines `from`…`to`. */
export const inPart = (b, from, to) => !!b && b.from <= to && b.to >= from;

/**
 * The lines a rendered Markdown block came from (`data-line="a-b"`: core/markdown.js, 0-based, `b` past its end),
 * as a part's count them (from 1): `{from, to}`, or `null`.
 */
export function blockLines(attr) {
  const m = /^(\d+)-(\d+)$/.exec(String(attr || ''));
  if (!m) return null;
  const a = Number(m[1]);
  const b = Number(m[2]);
  return { from: a + 1, to: Math.max(a + 1, b) };
}
