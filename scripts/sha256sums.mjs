// SHA-256 checksums of a release's files, as `sha256sum` writes them, for the
// release's SHA256SUMS.txt asset (.github/workflows/release.yml, nightly.yml;
// docs/architecture/updates-and-release.md):
//
//   node scripts/sha256sums.mjs <out-file> <file>...
//
// One line per file, "<hex>  <name>" with the file's name only (as it's
// published), sorted by name, LF line endings — so on Linux, macOS or Git
// Bash `sha256sum -c SHA256SUMS.txt` checks downloads in the same folder. A
// missing or empty file is an error: a release never lists one it hasn't got.
import { createHash } from 'node:crypto';
import { createReadStream, statSync, writeFileSync } from 'node:fs';
import path from 'node:path';

const [out, ...files] = process.argv.slice(2);
if (!out || files.length === 0) {
  console.error('usage: node scripts/sha256sums.mjs <out-file> <file>...');
  process.exit(2);
}

function sha256(file) {
  return new Promise((resolve, reject) => {
    const hash = createHash('sha256');
    createReadStream(file)
      .on('data', (chunk) => hash.update(chunk))
      .on('error', reject)
      .on('end', () => resolve(hash.digest('hex')));
  });
}

const lines = [];
for (const file of files) {
  const st = statSync(file, { throwIfNoEntry: false });
  if (!st?.isFile() || st.size === 0) {
    console.error(`sha256sums: ${file} is missing or empty`);
    process.exit(1);
  }
  lines.push({ name: path.basename(file), hex: await sha256(file) });
}
const names = new Set(lines.map((l) => l.name));
if (names.size !== lines.length) {
  console.error('sha256sums: two files share a name');
  process.exit(1);
}
lines.sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0));
const text = lines.map((l) => `${l.hex}  ${l.name}\n`).join('');
writeFileSync(out, text);
process.stdout.write(text);
