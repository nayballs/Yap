// Stand-ins for the speaker models' downloads (src-tauri/src/speakers/diarize/
// registry.rs): a fake NeMo-Speech.cpp runtime zip laid out like NVIDIA's
// (bin/*.dll, the CLI, share/licenses/…) and a fake model file, served from
// a local server, slowly enough to see the progress. Yap is pointed at them
// with the debug-only YAP_DIARIZATION_SOURCES file (url, size, SHA-256 per
// package), so a test run never fetches anything from the internet.
import crypto from 'node:crypto';
import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';

const CRC_TABLE = (() => {
  const t = new Uint32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    t[n] = c >>> 0;
  }
  return t;
})();

function crc32(buf) {
  let c = 0xffffffff;
  for (const b of buf) c = CRC_TABLE[(c ^ b) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

/** A zip of `files` ({ 'a/b.dll': Buffer|string }), stored (no compression). */
export function makeZip(files) {
  const locals = [];
  const centrals = [];
  let offset = 0;
  for (const [name, body] of Object.entries(files)) {
    const data = Buffer.isBuffer(body) ? body : Buffer.from(body);
    const nameBuf = Buffer.from(name, 'utf8');
    const crc = crc32(data);
    const local = Buffer.alloc(30);
    local.writeUInt32LE(0x04034b50, 0);
    local.writeUInt16LE(20, 4); // version needed
    local.writeUInt16LE(0x0800, 6); // UTF-8 names
    local.writeUInt16LE(0, 8); // stored
    local.writeUInt32LE(0, 10); // time/date
    local.writeUInt32LE(crc, 14);
    local.writeUInt32LE(data.length, 18);
    local.writeUInt32LE(data.length, 22);
    local.writeUInt16LE(nameBuf.length, 26);
    local.writeUInt16LE(0, 28);
    locals.push(local, nameBuf, data);
    const central = Buffer.alloc(46);
    central.writeUInt32LE(0x02014b50, 0);
    central.writeUInt16LE(20, 4);
    central.writeUInt16LE(20, 6);
    central.writeUInt16LE(0x0800, 8);
    central.writeUInt16LE(0, 10);
    central.writeUInt32LE(0, 12);
    central.writeUInt32LE(crc, 16);
    central.writeUInt32LE(data.length, 20);
    central.writeUInt32LE(data.length, 24);
    central.writeUInt16LE(nameBuf.length, 28);
    central.writeUInt32LE(offset, 42);
    centrals.push(central, nameBuf);
    offset += 30 + nameBuf.length + data.length;
  }
  const centralBuf = Buffer.concat(centrals);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0);
  end.writeUInt16LE(Object.keys(files).length, 8);
  end.writeUInt16LE(Object.keys(files).length, 10);
  end.writeUInt32LE(centralBuf.length, 12);
  end.writeUInt32LE(offset, 16);
  return Buffer.concat([...locals, centralBuf, end]);
}

const sha256 = (buf) => crypto.createHash('sha256').update(buf).digest('hex');

/** Reference turns from an RTTM file: [{ start, end, speaker }] in seconds. */
export function readRttm(file) {
  return fs
    .readFileSync(file, 'utf8')
    .split('\n')
    .map((l) => l.trim().split(/\s+/))
    .filter((p) => p[0] === 'SPEAKER')
    .map((p) => ({ start: Number(p[3]), end: Number(p[3]) + Number(p[4]), speaker: p[7] }));
}

/**
 * Frame-level diarization error (10 ms frames, the best 1:1 speaker mapping,
 * overlap scored, `collar` seconds around reference boundaries left out):
 * the spike's der.py. Segments are { start, end, speaker } in seconds.
 */
export function frameError(ref, hyp, collar = 0.25) {
  const step = 0.01;
  for (const s of [...ref, ...hyp]) {
    if (!Number.isFinite(s.start) || !Number.isFinite(s.end)) throw new Error(`bad segment ${JSON.stringify(s)}`);
  }
  const n = Math.ceil((Math.max(0, ...ref.map((s) => s.end), ...hyp.map((s) => s.end)) + 1) / step);
  const grid = (segs) => {
    const names = [...new Set(segs.map((s) => String(s.speaker)))].sort();
    const m = names.map(() => new Uint8Array(n));
    for (const s of segs) {
      const row = m[names.indexOf(String(s.speaker))];
      for (let i = Math.round(s.start / step); i < Math.min(n, Math.round(s.end / step)); i++) row[i] = 1;
    }
    return m;
  };
  const R = grid(ref);
  const H = grid(hyp);
  const scored = new Uint8Array(n).fill(1);
  const c = Math.round(collar / step);
  for (const s of ref) {
    for (const t of [s.start, s.end]) {
      const i = Math.round(t / step);
      for (let k = Math.max(0, i - c); k < Math.min(n, i + c); k++) scored[k] = 0;
    }
  }
  const co = R.map((r) => H.map((h) => {
    let x = 0;
    for (let i = 0; i < n; i++) if (scored[i] && r[i] && h[i]) x++;
    return x;
  }));
  // Best mapping, by brute force (≤ 8 voices).
  let best = { score: -1, map: [] };
  const used = new Array(H.length).fill(false);
  const map = new Array(R.length).fill(-1);
  (function search(i, score) {
    if (i === R.length) {
      if (score > best.score) best = { score, map: [...map] };
      return;
    }
    search(i + 1, score);
    for (let j = 0; j < H.length; j++) {
      if (used[j] || !co[i][j]) continue;
      used[j] = true;
      map[i] = j;
      search(i + 1, score + co[i][j]);
      map[i] = -1;
      used[j] = false;
    }
  })(0, 0);
  let total = 0;
  let errors = 0;
  for (let i = 0; i < n; i++) {
    if (!scored[i]) continue;
    const refs = R.reduce((a, r) => a + r[i], 0);
    const hyps = H.reduce((a, h) => a + h[i], 0);
    const correct = R.reduce((a, r, k) => a + (r[i] && best.map[k] >= 0 && H[best.map[k]][i] ? 1 : 0), 0);
    total += refs;
    errors += Math.max(refs - hyps, 0) + Math.max(hyps - refs, 0) + (Math.min(refs, hyps) - correct);
  }
  return errors / Math.max(1, total);
}

/** The DLLs Yap needs from the runtime (registry.rs `required`). */
export const RUNTIME_DLLS = ['nemo_speech_asr_c.dll', 'nemo_speech_asr.dll', 'ggml.dll', 'ggml-base.dll', 'ggml-cpu.dll'];
export const MODEL_FILE = 'Nemotron-3-Diarization.q8_0.gguf';

/**
 * Serve a fake model and runtime from 127.0.0.1, and write the sources file
 * pointing at them into `dir`. `env` is what the Yap instance needs.
 */
export async function startFakeSpeakerModels(dir) {
  fs.mkdirSync(dir, { recursive: true });
  const top = 'nemo-speech-0.2.0-windows-x86_64-cpu';
  const zipFiles = {};
  for (const dll of RUNTIME_DLLS) zipFiles[`${top}/bin/${dll}`] = `fake ${dll}`;
  zipFiles[`${top}/bin/nemo-speech.exe`] = 'the CLI, which Yap leaves out';
  zipFiles[`${top}/bin/llama.dll`] = 'not needed for diarization';
  zipFiles[`${top}/share/licenses/nemo-speech/LICENSE`] = 'Apache License 2.0 (stand-in)';
  zipFiles[`${top}/share/licenses/nemo-speech/NOTICE`] = 'NeMo-Speech.cpp (stand-in)';
  zipFiles[`${top}/include/nemo_speech/diar.h`] = '// headers stay out';
  const runtime = makeZip(zipFiles);
  const model = crypto.randomBytes(320 * 1024);
  const bodies = { '/runtime.zip': runtime, '/model.gguf': model };
  const requests = [];

  const server = http.createServer(async (req, res) => {
    requests.push(req.url);
    const body = bodies[req.url];
    if (!body) {
      res.writeHead(404).end();
      return;
    }
    res.writeHead(200, { 'Content-Length': body.length, 'Content-Type': 'application/octet-stream' });
    // In 16 pieces, ~2 s in all: long enough to see the progress.
    const piece = Math.ceil(body.length / 16);
    for (let i = 0; i < body.length; i += piece) {
      if (res.destroyed) return;
      res.write(body.subarray(i, i + piece));
      await new Promise((r) => setTimeout(r, 130));
    }
    res.end();
  });
  await new Promise((r) => server.listen(0, '127.0.0.1', r));
  const base = `http://127.0.0.1:${server.address().port}`;
  const sourcesFile = path.join(dir, 'diarization-sources.json');
  const local = {
    'nemotron-3-diarization': [{ url: `${base}/model.gguf`, sha256: sha256(model), size: model.length }],
    'nemo-speech-0.2.0-cpu': [{ url: `${base}/runtime.zip`, sha256: sha256(runtime), size: runtime.length }],
  };
  const write = (sources) => fs.writeFileSync(sourcesFile, JSON.stringify(sources, null, 2));
  write(local);
  return {
    env: { YAP_DIARIZATION_SOURCES: sourcesFile },
    requests,
    modelBytes: model.length,
    /** Point the model at another URL (the internet, say), or back. */
    setModelUrl(url) {
      write({ ...local, 'nemotron-3-diarization': [{ ...local['nemotron-3-diarization'][0], url: url ?? `${base}/model.gguf` }] });
    },
    close: () => new Promise((r) => server.close(r)),
  };
}
