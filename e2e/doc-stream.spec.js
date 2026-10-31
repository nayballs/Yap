// The document panel's page as Chat writes it, on its own (no app needed):
// src/lib/views/chat/docStream.js (the text shown, in pieces; the pace a
// section flows in at) and docPieces.js (the page cut at its sections and
// drawn a piece at a time; the writing caret). jsdom stands in for the
// page's DOM.
import { test, expect } from '@playwright/test';
import { JSDOM } from 'jsdom';
import { renderMarkdown } from '../src/lib/core/markdown.js';
import { shownSegments, shownText, revealStep, revealCut, revealFrom, revealOn } from '../src/lib/views/chat/docStream.js';
import { splitSections, drawParts, placeCaret } from '../src/lib/views/chat/docPieces.js';
import { sectionText } from '../src/lib/views/chat/docSections.js';

let dom;
test.beforeAll(() => {
  dom = new JSDOM('');
  globalThis.window = dom.window;
});
test.afterAll(() => {
  delete globalThis.window;
  dom.window.close();
});

const DOC = [
  '# Fell walk',
  '',
  'A day on the hill.',
  '',
  '## Route',
  '',
  '- Up the beck',
  '- Down the ridge',
  '',
  '<!-- yap:section Kit | what to carry -->',
  '',
  '<!-- yap:section Food | where to eat -->',
  '',
].join('\n');

test('the text shown: the title left out, a live section a piece of its own in its place', () => {
  // Nothing live: one piece, the title line and the blank after it gone.
  expect(shownSegments(DOC, 'Fell walk')).toEqual([{ md: DOC.split('\n').slice(2).join('\n'), live: null }]);
  // Kit being written: in its placeholder's place, the rest around it.
  const segs = shownSegments(DOC, 'Fell walk', { Kit: '- Water\n- A ma' });
  expect(segs.map((s) => s.live)).toEqual([null, 'Kit', null]);
  expect(segs[1].md).toBe('## Kit\n\n- Water\n- A ma');
  expect(segs[2].md).toBe('\n<!-- yap:section Food | where to eat -->\n');
  // Joined, the whole text as before (the selection's lines count on it).
  expect(segs.map((s) => s.md).join('\n')).toBe(shownText(DOC, 'Fell walk', { Kit: '- Water\n- A ma' }));
  // Its own heading kept; a rewrite in the section's place; a landed
  // section, found by its own heading, as it goes on flowing in.
  expect(shownSegments(DOC, 'Fell walk', { Kit: '## Kit and kaboodle\n\n- Water' })[1].md).toBe('## Kit and kaboodle\n\n- Water');
  const rewrite = shownSegments(DOC, 'Fell walk', { Route: '- Up the gh' });
  expect(rewrite.map((s) => s.live)).toEqual([null, 'Route', null]);
  expect(rewrite[0].md).toBe('A day on the hill.\n');
  expect(rewrite[1].md).toBe('## Route\n\n- Up the gh');
  const landed = DOC.replace('<!-- yap:section Kit | what to carry -->', '## Kit and kaboodle\n\n- Water\n- A map');
  const flowing = shownSegments(landed, 'Fell walk', { Kit: '## Kit and kaboodle\n\n- Wat' });
  expect(flowing.filter((s) => s.live).map((s) => s.md)).toEqual(['## Kit and kaboodle\n\n- Wat']);
  expect(flowing.map((s) => s.md).join('\n')).not.toContain('A map');
  expect(sectionText(landed, 'Kit', '## Kit and kaboodle\n\n- W')).toBe('## Kit and kaboodle\n\n- Water\n- A map');
  // A section first at the top: no blank line before it.
  expect(shownText('# T\n\n<!-- yap:section A | a -->\n', 'T', { A: 'x' })).toBe('## A\n\nx\n');
  // A new section at the end.
  expect(shownSegments('# T\n\nHello.', 'T', { Tips: 'Go early.' }).map((s) => s.md)).toEqual(['Hello.\n', '## Tips\n\nGo early.']);
});

test('the pace: a burst flows in over about a second, never stalling, never all at once', () => {
  // 600 characters arrive at once: frame by frame at 60 a second.
  let have = 0;
  const steps = [];
  let frames = 0;
  while (have < 600 && frames < 600) {
    const next = revealStep(have, 600, 16.7);
    steps.push(next - have);
    have = next;
    frames++;
  }
  expect(have).toBe(600);
  // Caught up in about a second and a half, in many small steps.
  expect(frames).toBeGreaterThan(40);
  expect(frames).toBeLessThan(110);
  expect(Math.max(...steps)).toBeLessThan(40);
  // Little left: at least 90 a second, so a short line never crawls.
  expect(revealStep(0, 5, 16.7)).toBeCloseTo(1.5, 1);
  // A long stall (a hidden window) counts as a quarter of a second at most.
  expect(revealStep(0, 10_000, 60_000)).toBeLessThan(10_000 * 0.9);
  expect(revealStep(5, 5, 16)).toBe(5);
  // Cut: rounded down, never inside an emoji.
  expect(revealCut('ab😀c', 2.9)).toBe(2);
  expect(revealCut('ab😀c', 3)).toBe(4);
  // A heading (and the blank line under it) is shown whole from the start.
  const head = '## Kit\n\n'.length;
  expect(revealFrom('## Kit\n\n- Water')).toBe(head);
  expect(revealFrom('- Water')).toBe(0);
  expect(revealFrom('## Ki')).toBe(0);
  // Text that came on keeps what's shown; shorter (a rewrite began), from its start.
  expect(revealOn(30, '## Kit\n\n- Water and a long list of things')).toBe(30);
  expect(revealOn(undefined, '## Kit\n\n- Water')).toBe(head);
  expect(revealOn(300, '## Kit\n\n- W')).toBe(head);
  // Landed as saved, with the heading it streamed without: the same text shown.
  expect(revealOn(5, '## Kit\n\n- Water', '- Water')).toBe(5 + head);
  expect(revealOn(5, '## Kit\n\n- Water', '## Ki')).toBe(head);
});

test('the page cut at its sections, drawn a piece at a time', () => {
  const md = shownText(DOC, 'Fell walk');
  const parts = splitSections(md);
  expect(parts.map((p) => p.md.split('\n')[0])).toEqual(['A day on the hill.', '## Route', '<!-- yap:section Kit | what to carry -->', '<!-- yap:section Food | where to eat -->']);
  expect(parts.map((p) => p.line)).toEqual([0, 2, 7, 9]);
  // Never inside a code block.
  expect(splitSections('Intro\n\n```\n## not a heading\n```\n\n## Real').map((p) => p.line)).toEqual([0, 6]);
  // Drawn in pieces, the page reads as the whole text would, each block
  // starting on the same line (a piece's last block ends at its own last
  // line, not the blank one after it).
  const doc = dom.window.document;
  const draw = (root, pieces, opts) => drawParts(root, pieces.map((p) => ({ html: renderMarkdown(p.md, { lines: true }), live: p.live ?? null, line: p.line })), opts);
  const whole = doc.createElement('div');
  whole.innerHTML = renderMarkdown(md, { lines: true });
  const root = doc.createElement('div');
  draw(root, parts);
  const flat = (el) => el.innerHTML.replace(/ data-line="[^"]*"/g, '').replace(/>\s+</g, '><');
  expect(flat(root)).toBe(flat(whole));
  const starts = (el) => [...el.children].map((c) => c.getAttribute('data-line').split('-')[0]);
  expect(starts(root)).toEqual(starts(whole));
  // A piece unchanged is kept as it was (the same elements); a changed one
  // is drawn again, and with `compare` it's said new.
  const kept = root.querySelector('h3');
  const changed = md.replace('- Down the ridge', '- Down the long ridge');
  const fresh = draw(root, splitSections(changed), { compare: true });
  expect(root.querySelector('h3')).not.toBe(kept); // the Route piece changed
  expect(fresh.map((el) => el.tagName)).toEqual(['UL']);
  const para = root.querySelector('p');
  draw(root, splitSections(changed));
  expect(root.querySelector('p')).toBe(para);
  // A piece moved down keeps its elements, its lines the page's.
  const moved = draw(root, splitSections(`New first line.\n\n${changed}`), { compare: true });
  expect(root.querySelector('h3').getAttribute('data-line')).toBe('4-5');
  expect(moved.map((el) => el.textContent)).toEqual(['New first line.']);
});

test('the writing caret: one element at the end of the live piece', () => {
  const doc = dom.window.document;
  const root = doc.createElement('div');
  const pieces = [
    { html: renderMarkdown('Intro.'), live: false, line: 0 },
    { html: renderMarkdown('## Kit\n\n| Item | Why |\n|---|---|\n| Water | Thirst |'), live: true, line: 2 },
    { html: renderMarkdown('<!-- yap:section Food | where to eat -->'), live: false, line: 8 },
  ];
  drawParts(root, pieces);
  const caret = placeCaret(root, 'Chat');
  expect(caret.getAttribute('data-who')).toBe('Chat');
  expect(caret.previousSibling.textContent).toBe('Thirst');
  expect(caret.closest('td')).not.toBeNull();
  // Its label is drawn by CSS: no text of its own.
  expect(caret.textContent).toBe('');
  // As the text grows: the same element, moved.
  pieces[1] = { html: renderMarkdown('## Kit\n\n| Item | Why |\n|---|---|\n| Water | Thirst |\n| Map | Way |'), live: true, line: 2 };
  drawParts(root, pieces);
  expect(placeCaret(root, 'Chat')).toBe(caret);
  expect(caret.previousSibling.textContent).toBe('Way');
  expect(root.querySelectorAll('.doccaret')).toHaveLength(1);
  // Nothing live: no caret.
  drawParts(root, pieces.map((p) => ({ ...p, live: false })));
  expect(placeCaret(root, 'Chat')).toBeNull();
  expect(root.querySelectorAll('.doccaret')).toHaveLength(0);
});
