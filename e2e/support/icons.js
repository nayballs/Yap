// Stand-ins for installed call apps' icons. Test mode reads no installed
// apps (src-tauri/src/app_icons.rs), so a spec that wants an app to look
// installed points YAP_E2E_APP_ICONS at a folder of `<app id>.png`: here, a
// rounded square in the app's colour with a white dot, drawn as a PNG.
import fs from 'node:fs';
import path from 'node:path';
import zlib from 'node:zlib';
import { OUT } from './yap.js';

/** Teams (a letter badge without its icon) and Discord (a mark). */
export const STAND_INS = { teams: [91, 95, 199], discord: [88, 101, 242] };

/**
 * Write `apps`' stand-ins (app id → [r, g, b]) to
 * test-results/app/fixtures/app-icons-<name>/ and return that folder.
 */
export function standInIcons(name, apps = STAND_INS) {
  const dir = path.join(OUT, 'fixtures', `app-icons-${name}`);
  fs.rmSync(dir, { recursive: true, force: true });
  fs.mkdirSync(dir, { recursive: true });
  for (const [app, rgb] of Object.entries(apps)) {
    fs.writeFileSync(path.join(dir, `${app}.png`), iconPng(48, rgb));
  }
  return dir;
}

/** A size × size PNG: a rounded square in `rgb` with a white dot, edges smoothed. */
export function iconPng(size, [r, g, b]) {
  const radius = size * 0.22;
  const dot = size * 0.2;
  const rows = [];
  for (let y = 0; y < size; y++) {
    const row = Buffer.alloc(1 + size * 4); // starts with filter type 0 (none)
    for (let x = 0; x < size; x++) {
      const [px, py] = [x + 0.5, y + 0.5];
      const dx = Math.max(radius - px, px - (size - radius), 0);
      const dy = Math.max(radius - py, py - (size - radius), 0);
      const inside = clamp(radius - Math.hypot(dx, dy) + 0.5);
      const white = clamp(dot - Math.hypot(px - size / 2, py - size / 2) + 0.5);
      const mix = (c) => Math.round(c + (255 - c) * white);
      row.set([mix(r), mix(g), mix(b), Math.round(inside * 255)], 1 + x * 4);
    }
    rows.push(row);
  }
  const header = Buffer.alloc(13);
  header.writeUInt32BE(size, 0);
  header.writeUInt32BE(size, 4);
  header.set([8, 6, 0, 0, 0], 8); // 8-bit RGBA, deflate, no filter, no interlace
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk('IHDR', header),
    chunk('IDAT', zlib.deflateSync(Buffer.concat(rows))),
    chunk('IEND', Buffer.alloc(0)),
  ]);
}

const clamp = (v) => Math.min(1, Math.max(0, v));

const CRC = Array.from({ length: 256 }, (_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});

function crc32(bytes) {
  let c = 0xffffffff;
  for (const byte of bytes) c = CRC[(c ^ byte) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

function chunk(type, data) {
  const out = Buffer.alloc(12 + data.length);
  out.writeUInt32BE(data.length, 0);
  out.write(type, 4, 'ascii');
  data.copy(out, 8);
  out.writeUInt32BE(crc32(out.subarray(4, 8 + data.length)), 8 + data.length);
  return out;
}
