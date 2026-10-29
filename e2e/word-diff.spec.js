// src/lib/core/wordDiff.js on its own (no app needed): what AI cleanup changed in
// a dictation, word by word, as Home's "AI edited · N changes" shows it
// (home-ai-edits.spec.js shows it in the app). Capitals and punctuation
// aren't changes; words taken out or put in are, a run of them one change.
import { test, expect } from '@playwright/test';
import { wordDiff, wordKey } from '../src/lib/core/wordDiff.js';

/** The parts as one line: [-out-] and {+in+}. */
const show = ({ parts }) =>
  parts.map((p, i) => (i ? p.sep : '') + (p.type === 'del' ? `[-${p.text}-]` : p.type === 'add' ? `{+${p.text}+}` : p.text)).join('');

test('filler words taken out: one change, the cleaned text around them', () => {
  const d = wordDiff('um so send the report to sam by friday', 'Send the report to Sam by Friday.');
  expect(show(d)).toBe('[-um so-] Send the report to Sam by Friday.');
  expect(d.changes).toBe(1);
  const mid = wordDiff('i think we should uh ship on friday', 'I think we should ship on Friday.');
  expect(show(mid)).toBe('I think we should [-uh-] ship on Friday.');
  expect(mid.changes).toBe(1);
});

test('words swapped: what was said struck, what the AI wrote marked, each run one change', () => {
  const d = wordDiff('we need to fix the login bug before the demo', 'We must fix the sign-in bug before the demo.');
  expect(show(d)).toBe('We [-need to-] {+must+} fix the [-login-] {+sign-in+} bug before the demo.');
  expect(d.changes).toBe(2);
  // Words added at the end.
  const added = wordDiff('can you send it', 'Can you send it, please?');
  expect(show(added)).toBe('Can you send it, {+please?+}');
  expect(added.changes).toBe(1);
});

test('exact: capitals and punctuation are changes too (a note version, as "Fix grammar only" makes them)', () => {
  const d = wordDiff('send it now', 'Send it now.', { exact: true });
  expect(show(d)).toBe('[-send-] {+Send+} it [-now-] {+now.+}');
  expect(d.changes).toBe(2);
  expect(wordDiff('Send it now.', 'Send it now.', { exact: true }).changes).toBe(0);
  // Not exact (Home's AI edits): no change.
  expect(wordDiff('send it now', 'Send it now.').changes).toBe(0);
});

test('capitals and punctuation alone are no change', () => {
  for (const [said, cleaned] of [
    ['send it now', 'Send it now.'],
    ['its done', 'its done!'],
    ['it’s done', "It's done."],
    ['a - b', 'a, b'],
    ['', ''],
  ]) {
    expect(wordDiff(said, cleaned).changes, `${said} → ${cleaned}`).toBe(0);
  }
  // A dash taken out isn't shown; one put in reads as the text.
  expect(show(wordDiff('a - b', 'a, b'))).toBe('a, b');
  expect(show(wordDiff('a b', 'a — b'))).toBe('a — b');
});

test('the cleaned text keeps its line breaks', () => {
  const d = wordDiff('dear sam um thanks for the notes', 'Dear Sam,\nThanks for the notes.');
  expect(d.parts.map((p) => p.type)).toEqual(['same', 'del', 'same']);
  expect(d.parts[2].sep).toBe('\n');
  expect(d.parts[2].text).toBe('Thanks for the notes.');
  expect(show(d)).toBe('Dear Sam, [-um-]\nThanks for the notes.');
  const kept = wordDiff('best sam', 'Best,\nSam');
  expect(show(kept)).toBe('Best,\nSam');
});

test('all of it rewritten, or nothing said: one change', () => {
  expect(wordDiff('', 'Hello there.')).toEqual({ parts: [{ type: 'add', text: 'Hello there.', sep: '' }], changes: 1 });
  expect(wordDiff('hello there', '')).toEqual({ parts: [{ type: 'del', text: 'hello there', sep: '' }], changes: 1 });
  // Too long to align word by word: still one change, out then in.
  const said = Array.from({ length: 600 }, (_, i) => `said${i}`).join(' ');
  const cleaned = Array.from({ length: 600 }, (_, i) => `wrote${i}`).join(' ');
  const big = wordDiff(said, cleaned);
  expect(big.changes).toBe(1);
  expect(big.parts.map((p) => p.type)).toEqual(['del', 'add']);
});

test('a long dictation with one change aligns only the middle', () => {
  const words = Array.from({ length: 2000 }, (_, i) => `w${i}`);
  const said = words.join(' ');
  const cleaned = [...words.slice(0, 1000), 'changed', ...words.slice(1001)].join(' ');
  const d = wordDiff(said, cleaned);
  expect(d.changes).toBe(1);
  expect(d.parts.filter((p) => p.type !== 'same').map((p) => p.text)).toEqual(['w1000', 'changed']);
});

test('a word is compared without its capitals and punctuation', () => {
  expect(wordKey('Friday.')).toBe('friday');
  expect(wordKey('“Sam’s,”')).toBe("sam's");
  expect(wordKey('—')).toBe('');
  expect(wordKey('Zoë')).toBe('zoë');
});
