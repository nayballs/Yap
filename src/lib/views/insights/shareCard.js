// Insights as a picture to share (InsightsView's Share): the numbers only —
// words, speed, time saved, the streaks, dictations and fixes — never a
// word said, an app or a name. Drawn on a canvas in the page in the
// theme's own colours (the dark feature card Home's hero uses), then
// copied or saved through Rust as Snip to Chat copies a picture
// (dictation/stats/share.rs → snip/copy_save.rs).

const W = 1200;
const H = 630;

/** A --yap-* token's colour as the page has it now. */
function token(name, fallback) {
  const v = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  return v || fallback;
}

const fmt = (n) => Math.round(Number(n) || 0).toLocaleString();

/** "3 h 20 min", "45 min". */
function minutesText(min) {
  const m = Math.round(Number(min) || 0);
  if (m < 60) return `${m} min`;
  return `${Math.floor(m / 60)} h ${m % 60} min`;
}

/** The figures on the card, in order: [number, label]. Pure. */
export function cardFigures(stats) {
  const s = stats || {};
  const weeks = s.weekStreak ?? 0;
  return [
    [s.wpm == null ? '–' : fmt(s.wpm), 'words a minute'],
    [minutesText(s.timeSaved?.minutes ?? s.timeSavedMinutes), 'saved against typing'],
    [fmt(weeks), weeks === 1 ? 'week in a row' : 'weeks in a row'],
    [fmt(s.fixes ? s.fixes.cleanupWords + s.fixes.dictionaryFixes : 0), 'words fixed'],
  ];
}

/** The stats card as a PNG's bytes. */
export async function statsCardPng(stats) {
  await document.fonts?.ready;
  const canvas = document.createElement('canvas');
  canvas.width = W;
  canvas.height = H;
  const g = canvas.getContext('2d');
  const bg = token('--yap-feature', '#26231c');
  const fg = token('--yap-feature-fg', '#f7f5f0');
  const accent = token('--yap-primary', '#c2690a');
  const display = token('--yap-font-display', 'Georgia, serif');
  const body = getComputedStyle(document.body).fontFamily || 'sans-serif';

  g.fillStyle = bg;
  g.fillRect(0, 0, W, H);
  // A band of the accent along the top.
  g.fillStyle = accent;
  g.fillRect(0, 0, W, 10);

  g.fillStyle = fg;
  g.textBaseline = 'alphabetic';
  g.font = `600 30px ${body}`;
  g.fillText('Yap', 72, 96);
  g.globalAlpha = 0.65;
  g.font = `500 24px ${body}`;
  g.fillText('My dictation', 136, 96);
  g.globalAlpha = 1;

  g.font = `550 150px ${display}`;
  g.fillText(fmt(stats?.totalWords), 66, 270);
  g.globalAlpha = 0.75;
  g.font = `500 30px ${body}`;
  const count = stats?.totalTranscriptions ?? 0;
  g.fillText(`words dictated, in ${fmt(count)} ${count === 1 ? 'dictation' : 'dictations'}`, 72, 322);
  g.globalAlpha = 1;

  // Four figures in a row.
  const figures = cardFigures(stats);
  const colW = (W - 144) / figures.length;
  figures.forEach(([num, label], i) => {
    const x = 72 + i * colW;
    g.fillStyle = accent;
    g.fillRect(x, 392, 44, 4);
    g.fillStyle = fg;
    g.font = `550 56px ${display}`;
    g.fillText(num, x, 470);
    g.globalAlpha = 0.7;
    g.font = `500 22px ${body}`;
    g.fillText(label, x, 506);
    g.globalAlpha = 1;
  });

  g.globalAlpha = 0.5;
  g.font = `500 20px ${body}`;
  g.fillText('Dictated on my own PC with Yap', 72, 578);
  g.globalAlpha = 1;

  const blob = await new Promise((resolve, reject) => canvas.toBlob((b) => (b ? resolve(b) : reject(new Error('The picture could not be made'))), 'image/png'));
  return new Uint8Array(await blob.arrayBuffer());
}
