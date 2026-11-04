// Stand-ins for Kokoro's download (src-tauri/src/voice/tts/registry.rs): a fake
// model, 54 fake voices, fake dictionaries, and a zip laid out like eSpeak
// NG's unpacked installer (a debug build takes a zip where the real thing is
// an .msi), served from a local server, the model slowly enough to see the
// progress. Yap is pointed at them with the debug-only YAP_TTS_SOURCES file
// (url, size, SHA-256 per file), so a test run never fetches anything from
// the internet. Test mode's Kokoro plays silence once these are installed.
import crypto from 'node:crypto';
import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';
import { makeZip } from './diarization.js';

/** Kokoro v1.0's 54 voices (voice/tts/kokoro.rs `VOICES`). */
export const KOKORO_VOICES = [
  'af_heart', 'af_alloy', 'af_aoede', 'af_bella', 'af_jessica', 'af_kore', 'af_nicole', 'af_nova', 'af_river',
  'af_sarah', 'af_sky', 'am_adam', 'am_echo', 'am_eric', 'am_fenrir', 'am_liam', 'am_michael', 'am_onyx',
  'am_puck', 'am_santa', 'bf_alice', 'bf_emma', 'bf_isabella', 'bf_lily', 'bm_daniel', 'bm_fable', 'bm_george',
  'bm_lewis', 'ef_dora', 'em_alex', 'em_santa', 'ff_siwis', 'hf_alpha', 'hf_beta', 'hm_omega', 'hm_psi',
  'if_sara', 'im_nicola', 'jf_alpha', 'jf_gongitsune', 'jf_nezumi', 'jf_tebukuro', 'jm_kumo', 'pf_dora',
  'pm_alex', 'pm_santa', 'zf_xiaobei', 'zf_xiaoni', 'zf_xiaoxiao', 'zf_xiaoyi', 'zm_yunjian', 'zm_yunxi',
  'zm_yunxia', 'zm_yunyang',
];

const sha256 = (buf) => crypto.createHash('sha256').update(buf).digest('hex');

/**
 * Serve a fake Kokoro + eSpeak NG from 127.0.0.1 and write the sources file
 * into `dir`. `env` is what the Yap instance needs.
 */
export async function startFakeKokoro(dir) {
  fs.mkdirSync(dir, { recursive: true });
  const kokoro = { 'model.onnx': crypto.randomBytes(384 * 1024) };
  for (const v of KOKORO_VOICES) kokoro[`voices/${v}.bin`] = crypto.randomBytes(64);
  for (const f of ['us_gold', 'us_silver', 'gb_gold', 'gb_silver']) kokoro[`lexicon/${f}.json`] = Buffer.from(`{"${f}": "x"}`);
  kokoro['lexicon/LICENSE'] = Buffer.from('Apache License 2.0 (stand-in)');
  const top = 'eSpeak NG';
  const espeak = {
    'espeak-ng.msi': makeZip({
      [`${top}/espeak-ng.exe`]: 'not a real program',
      [`${top}/libespeak-ng.dll`]: 'not a real library',
      [`${top}/espeak-ng-data/phontab`]: 'phontab',
      [`${top}/espeak-ng-data/en_dict`]: 'en',
      [`${top}/espeak-ng-data/ru_dict`]: 'a language Kokoro never reads',
      [`${top}/espeak-ng-data/lang/gmw/en`]: 'en',
    }),
    COPYING: Buffer.from('GNU GENERAL PUBLIC LICENSE Version 3 (stand-in)'),
  };
  const bodies = {};
  const sources = { 'kokoro-v1.0': {}, 'espeak-ng-1.52.0': {} };
  const requests = [];
  const server = http.createServer(async (req, res) => {
    requests.push(req.url);
    const body = bodies[req.url];
    if (!body) {
      res.writeHead(404).end();
      return;
    }
    res.writeHead(200, { 'Content-Length': body.length, 'Content-Type': 'application/octet-stream' });
    if (req.url.endsWith('/model.onnx')) {
      // In 16 pieces, ~2 s in all: long enough to see the progress.
      const piece = Math.ceil(body.length / 16);
      for (let i = 0; i < body.length; i += piece) {
        if (res.destroyed) return;
        res.write(body.subarray(i, i + piece));
        await new Promise((r) => setTimeout(r, 130));
      }
      res.end();
    } else {
      res.end(body);
    }
  });
  await new Promise((r) => server.listen(0, '127.0.0.1', r));
  const base = `http://127.0.0.1:${server.address().port}`;
  for (const [pkg, files] of [['kokoro-v1.0', kokoro], ['espeak-ng-1.52.0', espeak]]) {
    for (const [dest, body] of Object.entries(files)) {
      const url = `/${pkg}/${dest}`;
      bodies[url] = body;
      sources[pkg][dest] = { url: `${base}${url}`, sha256: sha256(body), size: body.length };
    }
  }
  const sourcesFile = path.join(dir, 'tts-sources.json');
  const write = (s) => fs.writeFileSync(sourcesFile, JSON.stringify(s, null, 2));
  write(sources);
  const total = Object.values(sources).flatMap((p) => Object.values(p)).reduce((n, s) => n + s.size, 0);
  return {
    env: { YAP_TTS_SOURCES: sourcesFile },
    requests,
    total,
    modelBytes: kokoro['model.onnx'].length,
    /** Point the model at another URL (the internet, say), or back. */
    setModelUrl(url) {
      const s = structuredClone(sources);
      if (url) s['kokoro-v1.0']['model.onnx'].url = url;
      write(s);
    },
    close: () => new Promise((r) => server.close(r)),
  };
}
