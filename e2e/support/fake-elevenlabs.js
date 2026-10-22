// A stand-in for ElevenLabs' API (src-tauri/src/tts/elevenlabs.rs), served
// from 127.0.0.1 so a test run never reaches the internet. Yap is pointed at
// it with the debug-only YAP_ELEVENLABS_URL (`env`).
//
// - `GET /v2/voices`: three voices — two of ElevenLabs' defaults (George,
//   Alice; each with a `preview_url` here, a short WAV) and one of the
//   account's own (cloned, no sample);
// - `GET /v1/models`: Flash v2.5, Multilingual v2 and a speech-to-speech
//   model that can't do text to speech;
// - `GET /v1/user/subscription`: 1,234 of 10,000 characters used (or, with
//   `usage = false`, the key isn't allowed to read it);
// - `POST /v1/text-to-speech/{voice}/stream?output_format=pcm_24000`: raw
//   16-bit PCM at 24 kHz for the sentence (~60 ms a word, in pieces);
// - `GET /samples/{name}.wav`: a voice's sample.
// Every call needs `xi-api-key: <key>` (`KEY`, or `setKey`): another key is a
// 401 `invalid_api_key`, as ElevenLabs answers. `fail(status, code)` answers
// every speech request with that error until `fail(null)`. `requests` keeps
// `{ method, path, query, key, body }` of each call (`key`: the header as
// sent, so a spec can check it never went anywhere else).
import http from 'node:http';

export const KEY = 'sk_e2e_eleven_0123456789abcdef';
export const GEORGE = 'JBFqnCBsd6RMkjVDRZzb';
export const ALICE = 'Xb7hH8MSUJpSbSDYk0k2';
export const OWN = 'e2eOwnVoice0000001';

/** A short mono 16-bit WAV (~0.4 s of a quiet tone), as a voice's sample. */
function sampleWav() {
  const rate = 22_050;
  const n = Math.round(rate * 0.4);
  const data = Buffer.alloc(n * 2);
  for (let i = 0; i < n; i++) data.writeInt16LE(Math.round(Math.sin(i / 8) * 800), i * 2);
  const h = Buffer.alloc(44);
  h.write('RIFF', 0);
  h.writeUInt32LE(36 + data.length, 4);
  h.write('WAVEfmt ', 8);
  h.writeUInt32LE(16, 16);
  h.writeUInt16LE(1, 20);
  h.writeUInt16LE(1, 22);
  h.writeUInt32LE(rate, 24);
  h.writeUInt32LE(rate * 2, 28);
  h.writeUInt16LE(2, 32);
  h.writeUInt16LE(16, 34);
  h.write('data', 36);
  h.writeUInt32LE(data.length, 40);
  return Buffer.concat([h, data]);
}

/** PCM for `text`: ~60 ms a word at 24 kHz, a quiet tone. */
function pcmFor(text) {
  const words = Math.max(2, String(text).split(/\s+/).filter(Boolean).length);
  const n = Math.round(24_000 * 0.06 * words);
  const out = Buffer.alloc(n * 2);
  for (let i = 0; i < n; i++) out.writeInt16LE(Math.round(Math.sin(i / 6) * 500), i * 2);
  return out;
}

const json = (res, status, body) => {
  const text = JSON.stringify(body);
  res.writeHead(status, { 'Content-Type': 'application/json', 'Content-Length': Buffer.byteLength(text) });
  res.end(text);
};
const error = (res, status, type, code, message) => json(res, status, { detail: { type, code, message, status: code } });

export async function startFakeElevenLabs() {
  const state = { key: KEY, usage: true, fail: null, requests: [] };
  const wav = sampleWav();
  let base = '';
  const voices = () => [
    {
      voice_id: OWN,
      name: 'Imogen (cloned)',
      category: 'cloned',
      labels: { accent: 'british', gender: 'male' },
      preview_url: null,
    },
    {
      voice_id: GEORGE,
      name: 'George',
      category: 'premade',
      labels: { accent: 'british', gender: 'male', age: 'middle_aged' },
      preview_url: `${base}/samples/george.wav`,
    },
    {
      voice_id: ALICE,
      name: 'Alice',
      category: 'premade',
      labels: { accent: 'british', gender: 'female', age: 'middle_aged' },
      preview_url: `${base}/samples/alice.wav`,
    },
  ];

  const server = http.createServer(async (req, res) => {
    const url = new URL(req.url, 'http://x');
    let body = '';
    for await (const chunk of req) body += chunk;
    const key = req.headers['xi-api-key'] ?? null;
    state.requests.push({ method: req.method, path: url.pathname, query: url.search, key, body: body ? JSON.parse(body) : null });

    // A voice's sample: public, no key.
    if (req.method === 'GET' && url.pathname.startsWith('/samples/')) {
      res.writeHead(200, { 'Content-Type': 'audio/wav', 'Content-Length': wav.length });
      res.end(wav);
      return;
    }
    if (key !== state.key) {
      error(res, 401, 'authentication_error', 'invalid_api_key', 'Invalid API key');
      return;
    }
    if (req.method === 'GET' && url.pathname === '/v2/voices') {
      json(res, 200, { voices: voices(), has_more: false, next_page_token: null });
    } else if (req.method === 'GET' && url.pathname === '/v1/models') {
      json(res, 200, [
        { model_id: 'eleven_multilingual_v2', name: 'Eleven Multilingual v2', can_do_text_to_speech: true, languages: [] },
        { model_id: 'eleven_flash_v2_5', name: 'Eleven Flash v2.5', can_do_text_to_speech: true, languages: [] },
        { model_id: 'eleven_multilingual_sts_v2', name: 'Eleven Multilingual v2 STS', can_do_text_to_speech: false },
      ]);
    } else if (req.method === 'GET' && url.pathname === '/v1/user/subscription') {
      if (!state.usage) {
        error(res, 401, 'authorization_error', 'missing_permissions', 'The API key you used is missing the permission user_read');
        return;
      }
      json(res, 200, {
        tier: 'starter',
        character_count: 1234,
        character_limit: 10000,
        next_character_count_reset_unix: Math.floor(Date.UTC(2026, 10, 3, 9) / 1000),
      });
    } else if (req.method === 'POST' && /^\/v1\/text-to-speech\/[^/]+\/stream$/.test(url.pathname)) {
      if (state.fail) {
        const { status, type, code, message } = state.fail;
        error(res, status, type, code, message);
        return;
      }
      const pcm = pcmFor(JSON.parse(body || '{}').text || '');
      res.writeHead(200, { 'Content-Type': 'audio/pcm', 'Transfer-Encoding': 'chunked' });
      // In pieces, as a stream comes (an odd split: a sample across two).
      for (let i = 0; i < pcm.length; i += 4_801) {
        if (res.destroyed) return;
        res.write(pcm.subarray(i, i + 4_801));
        await new Promise((r) => setTimeout(r, 5));
      }
      res.end();
    } else {
      error(res, 404, 'not_found', 'not_found', 'Not found');
    }
  });
  await new Promise((r) => server.listen(0, '127.0.0.1', r));
  base = `http://127.0.0.1:${server.address().port}`;
  const FAILS = {
    401: ['authentication_error', 'invalid_api_key', 'Invalid API key'],
    402: ['payment_required', 'insufficient_credits', 'You have run out of credits'],
    429: ['rate_limit_error', 'concurrent_limit_exceeded', 'Too many concurrent requests'],
  };
  return {
    base,
    env: { YAP_ELEVENLABS_URL: base },
    get requests() {
      return state.requests;
    },
    /** The speech requests so far. */
    speech: () => state.requests.filter((r) => r.method === 'POST'),
    /** Answer every speech request with this HTTP error (401, 402, 429), or not (`null`). */
    fail(status) {
      state.fail = status ? { status, ...Object.fromEntries(['type', 'code', 'message'].map((k, i) => [k, FAILS[status][i]])) } : null;
    },
    /** Whether the key may read its usage. */
    set usage(on) {
      state.usage = on;
    },
    reset() {
      state.fail = null;
      state.usage = true;
      state.key = KEY;
      state.requests.length = 0;
    },
    close: () => new Promise((r) => server.close(r)),
  };
}
