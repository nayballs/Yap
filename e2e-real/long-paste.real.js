// Long pastes into terminals (pasting/slices.rs): a stand-in coding CLI —
// raw keyboard, bracketed paste on, as Claude Code; it runs nothing and logs
// every byte that arrives — in Windows Terminal and in a classic console
// window (conhost).
//
// - Claude Code (its title "✳ …"): a long dictation goes in slices, each
//   one a paste Claude Code shows as text (≤ 800 characters, ≤ 2 line
//   breaks), all of it there, in order, nothing sent.
// - A classic console window (conhost) gets its own Edit → Paste, not
//   Ctrl+V: a program reading the keyboard raw gets Ctrl+V as a ^V there.
// - Windows Terminal stops a paste over 5 KiB with a warning nothing
//   arrives through until it's answered (measured 2026-10-25: 4,004
//   characters arrived whole, 16,019 not at all): Yap pastes into it in
//   slices of at most 4,000. Measured here from 1,000 to 64,000 characters
//   (a plain program, no coding CLI) in both, written to
//   test-results/real/long-paste.json.
import fs from 'node:fs';
import path from 'node:path';
import { test, expect } from './support/robot.js';
import { hasWindowsTerminal, OUT } from './support/targets.js';

test.skip(!hasWindowsTerminal(), 'Windows Terminal (wt.exe) is not installed');

const SENTENCES = [
  'Refactor the payment module so that retries back off.',
  'Keep the public interface exactly as it is today.',
  'Add a test for the case where the card is declined twice.',
  'Log the retry count, never the card number.',
  'Then update the changelog and the docs for the next release.',
];

/** About `n` characters of sentences (one line). */
function sentences(n) {
  let out = '';
  for (let i = 0; out.length < n; i++) out += (out ? ' ' : '') + SENTENCES[i % SENTENCES.length];
  return out;
}

/** What arrived as bracketed pastes (their text, line breaks as "\n"), and what arrived outside them. */
function pastes(received) {
  const inside = [...received.matchAll(/\x1b\[200~([\s\S]*?)\x1b\[201~/g)].map((m) => m[1].replace(/\r\n?/g, '\n'));
  const outside = received.replace(/\x1b\[200~[\s\S]*?\x1b\[201~/g, '');
  return { inside, outside };
}

/**
 * A console window that conhost never started (no window, the program
 * never ran, twice: support/targets.js `startConsoleCli`) is the harness's
 * failure, not a paste's: said, and that part skipped.
 */
const conhostDidntStart = (e) => /console window never showed up.*didn't start/.test(e.message);

const lf = (s) => s.replace(/\r\n?/g, '\n');
/** What arrived, without the bracketed paste markers, line breaks as "\n". */
const plain = (s) => lf(s).replace(/\x1b\[20[01]~/g, '');

test('Claude Code in Windows Terminal: a long dictation in slices it shows as text, all of it, nothing sent', async ({ robot }) => {
  const cli = await robot.claudeCode();
  // A list (its lines kept: Claude Code in Windows Terminal) and a long paragraph.
  const list = ['Fix these:', '- the login bug', '- the slow search', '- the flaky test', '- the docs'].join('\n');
  const text = `${list}\n${sentences(2_400)}`;
  const r = await robot.dictate(cli, text);
  expect(r.kind).toBe('terminal (Claude Code in Windows Terminal)');
  expect(r.submit).toBeNull();
  await expect
    .poll(() => pastes(cli.received()).inside.join(''), { timeout: 20_000, message: 'every slice arrived' })
    .toBe(lf(r.text));
  const { inside, outside } = pastes(cli.received());
  expect(inside.length).toBeGreaterThanOrEqual(4);
  for (const p of inside) {
    expect(p.length, p).toBeLessThanOrEqual(800);
    expect((p.match(/\n/g) || []).length, p).toBeLessThanOrEqual(2);
  }
  // Nothing outside the pastes: no Enter, nothing sent.
  expect(outside).toBe('');
  // The list's lines kept, the slices cut after a line break or a space.
  expect(inside[0].startsWith('Fix these:\n- the login bug\n')).toBe(true);
});

test('Claude Code in a console window: the console pastes it, one line, in slices, all of it', async ({ robot }) => {
  let cli;
  try {
    cli = await robot.consoleCli();
  } catch (e) {
    test.skip(conhostDidntStart(e), `conhost started nothing: ${e.message}`);
    throw e;
  }
  const text = sentences(2_000);
  const r = await robot.dictate(cli, text);
  expect(r.kind).toBe('terminal (Claude Code in a console)');
  expect(r.paste).toBe("the console's Paste");
  await expect.poll(() => plain(cli.received()), { timeout: 20_000 }).toBe(r.text);
  const { inside } = pastes(cli.received());
  // In bracketed paste (the console's own paste wraps it, as the program
  // asked), each slice one Claude Code shows as text; no ^V, no Enter.
  expect(inside.length).toBeGreaterThanOrEqual(3);
  for (const p of inside) expect(p.length).toBeLessThanOrEqual(800);
  expect(cli.received()).not.toMatch(/[\r\n\x16]/);
});

test('a long paste arrives whole: Windows Terminal and conhost, 1,000 to 64,000 characters', async ({ robot }) => {
  test.setTimeout(240_000);
  const results = [];
  for (const [label, open] of [
    ['Windows Terminal', () => robot.claudeCode('raw-wt', { title: `Yap robot raw ${robot.nonce}` })],
    ['conhost', () => robot.consoleCli('raw-conhost', { title: `Yap robot raw ${robot.nonce}` })],
  ]) {
    // One window each, the sizes one after another into it.
    let cli;
    try {
      cli = await open();
    } catch (e) {
      if (!conhostDidntStart(e)) throw e;
      console.warn(`long paste: ${label} skipped — conhost started nothing`);
      test.info().annotations.push({ type: 'skipped part', description: `${label}: ${e.message}` });
      continue;
    }
    for (const size of [1_000, 4_000, 16_000, 64_000]) {
      const had = plain(cli.received()).length;
      const text = sentences(size);
      const started = Date.now();
      const r = await robot.dictate(cli, text);
      expect(r.kind).toMatch(/^terminal \((Windows Terminal|a console)\)$/);
      // Wait for it all (or for nothing more to come).
      let got = '';
      let still = 0;
      while (still < 6 && Date.now() - started < 60_000) {
        await new Promise((res) => setTimeout(res, 500));
        const now = plain(cli.received()).slice(had);
        still = now === got ? still + 1 : 0;
        got = now;
        if (got.length >= r.text.length) break;
      }
      const intact = got === r.text;
      results.push({ terminal: label, chars: r.text.length, arrived: got.length, intact, ms: Date.now() - started });
      console.log(`long paste: ${label}, ${r.text.length} chars → ${got.length} arrived${intact ? ', intact' : ', NOT intact'}`);
    }
    await cli.close();
  }
  fs.mkdirSync(OUT, { recursive: true });
  fs.writeFileSync(path.join(OUT, 'long-paste.json'), JSON.stringify(results, null, 2));
  // Every size arrives whole in both.
  for (const r of results) expect(r.intact, JSON.stringify(r)).toBe(true);
});
