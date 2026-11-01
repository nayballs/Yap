// A word diff, worked out at build time: what was said against what was
// typed, as the app's Home shows it ("AI edited · N changes"). Line breaks
// are words of their own, so a list or an email's frame shows as inserted.

export interface Token {
  /** same: in both; del: said, not typed; ins: typed, not said */
  k: 'same' | 'del' | 'ins';
  text: string;
}

const tokens = (s: string) => s.replace(/\n/g, ' \n ').split(' ').filter(Boolean);

export function wordDiff(said: string, typed: string): Token[] {
  const a = tokens(said);
  const b = tokens(typed);
  // Longest common subsequence, then walk it.
  const lcs = Array.from({ length: a.length + 1 }, () => new Array<number>(b.length + 1).fill(0));
  for (let i = a.length - 1; i >= 0; i--) {
    for (let j = b.length - 1; j >= 0; j--) {
      lcs[i][j] = a[i] === b[j] ? lcs[i + 1][j + 1] + 1 : Math.max(lcs[i + 1][j], lcs[i][j + 1]);
    }
  }
  const out: Token[] = [];
  let i = 0;
  let j = 0;
  while (i < a.length || j < b.length) {
    if (i < a.length && j < b.length && a[i] === b[j]) {
      out.push({ k: 'same', text: a[i] });
      i++;
      j++;
    } else if (i < a.length && (j === b.length || lcs[i + 1][j] >= lcs[i][j + 1])) {
      // Deletions before insertions read better ("Thursday" struck, then "Friday").
      out.push({ k: 'del', text: a[i] });
      i++;
    } else {
      out.push({ k: 'ins', text: b[j] });
      j++;
    }
  }
  return out;
}

/** How many changes the app would count: runs of deleted or inserted words. */
export function changeCount(diff: Token[]): number {
  let n = 0;
  let inRun = false;
  for (const t of diff) {
    const changed = t.k !== 'same';
    if (changed && !inRun) n++;
    inRun = changed;
  }
  return n;
}
