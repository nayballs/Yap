// A stand-in for Claude Code in a terminal, for the robot's coding-CLI test
// (terminal.real.js). Like Claude Code it turns on bracketed paste and reads
// the keyboard raw; unlike it, it runs nothing. Every chunk of input is
// appended to the log file given as the first argument (one JSON line each:
// `{ data }`), and echoed so the window shows what arrived. The title
// ("✳ <nonce>", Claude Code's idle title) comes from Windows Terminal's
// --title.
import fs from 'node:fs';
import path from 'node:path';

const log = process.argv[2];
const write = (entry) => fs.appendFileSync(log, `${JSON.stringify(entry)}\n`);
// A classic console window's title (conhost: the window is the console's,
// titled as the program says); Windows Terminal's comes from --title.
if (process.argv[3]) process.title = process.argv[3];

process.stdout.write('\x1b[?2004h'); // bracketed paste on, as Claude Code does
process.stdout.write('Yap robot: a stand-in for a coding CLI. It runs nothing; it shows what arrives.\r\n');
// What a coding CLI showed on screen before ("Read(src/a.ts)"…): banner.txt
// beside the log, when the test wrote one.
const banner = path.join(path.dirname(log), 'banner.txt');
if (fs.existsSync(banner)) process.stdout.write(`${fs.readFileSync(banner, 'utf8').replace(/\r?\n/g, '\r\n')}\r\n`);
process.stdout.write('> ');
if (process.stdin.isTTY) process.stdin.setRawMode(true);
process.stdin.setEncoding('utf8');
process.stdin.resume();
process.stdin.on('data', (data) => {
  write({ data });
  const shown = data
    .replace(/\x1b\[200~/g, '[paste]')
    .replace(/\x1b\[201~/g, '[/paste]')
    .replace(/\r/g, '⏎\r\n');
  process.stdout.write(shown);
  // Ctrl+C (outside a paste) ends it, as a person would expect.
  if (data === '\x03') {
    process.stdout.write('\x1b[?2004l');
    process.exit(0);
  }
});
write({ ready: true });
