// A stand-in for OpenAI's, Gemini's and Groq's speech endpoints
// (src-tauri/src/voice/tts/openai.rs, gemini.rs, groq.rs, hosted.rs), served
// from 127.0.0.1 so a test run never reaches the internet. Yap is pointed at
// it with the debug-only YAP_OPENAI_TTS_URL, YAP_GEMINI_TTS_URL and
// YAP_GROQ_TTS_URL (`env`: all three name this one server; their paths
// don't clash).
//
// - `POST /v1/audio/speech` (OpenAI): `Authorization: Bearer <OPENAI_KEY>`;
//   `stream_format: "sse"` → server-sent events (`speech.audio.delta` with
//   base64 PCM, then `speech.audio.done` with its usage), else raw PCM;
// - `POST /v1beta/interactions` (Gemini): `x-goog-api-key: <GEMINI_KEY>`;
//   the Interactions API's events (`step.delta` audio, then
//   `interaction.completed` with its usage, then `[DONE]`);
// - `POST /openai/v1/audio/speech` (Groq): `Authorization: Bearer
//   <GROQ_KEY>`; a WAV, and a 400 for an input over 200 characters, as
//   Orpheus answers.
// Another key is each provider's own 401. `fail(provider, status)` answers
// that provider's speech with an error until `fail(provider, null)`.
// `requests` keeps `{ provider, method, path, auth, body }` of each call.
import http from 'node:http';

export const OPENAI_KEY = 'sk-e2e-openai-0123456789abcdef';
export const GEMINI_KEY = 'AIza-e2e-gemini-0123456789abcdef';
export const GROQ_KEY = 'gsk_e2e_groq_0123456789abcdef';

/** PCM for `text`: ~60 ms a word at 24 kHz, a quiet tone. */
function pcmFor(text) {
  const words = Math.max(2, String(text).split(/\s+/).filter(Boolean).length);
  const n = Math.round(24_000 * 0.06 * words);
  const out = Buffer.alloc(n * 2);
  for (let i = 0; i < n; i++) out.writeInt16LE(Math.round(Math.sin(i / 6) * 500), i * 2);
  return out;
}

function wavOf(pcm, rate = 24_000) {
  const h = Buffer.alloc(44);
  h.write('RIFF', 0);
  h.writeUInt32LE(36 + pcm.length, 4);
  h.write('WAVEfmt ', 8);
  h.writeUInt32LE(16, 16);
  h.writeUInt16LE(1, 20);
  h.writeUInt16LE(1, 22);
  h.writeUInt32LE(rate, 24);
  h.writeUInt32LE(rate * 2, 28);
  h.writeUInt16LE(2, 32);
  h.writeUInt16LE(16, 34);
  h.write('data', 36);
  h.writeUInt32LE(pcm.length, 40);
  return Buffer.concat([h, pcm]);
}

const json = (res, status, body) => {
  const text = JSON.stringify(body);
  res.writeHead(status, { 'Content-Type': 'application/json', 'Content-Length': Buffer.byteLength(text) });
  res.end(text);
};
const openAiError = (res, status, code, message) => json(res, status, { error: { message, type: 'invalid_request_error', code } });
const googleError = (res, status, state, message) => json(res, status, { error: { code: status, message, status: state } });

async function inPieces(res, buf, size = 4_801) {
  for (let i = 0; i < buf.length; i += size) {
    if (res.destroyed) return;
    res.write(buf.subarray(i, i + size));
    await new Promise((r) => setTimeout(r, 5));
  }
  res.end();
}

export async function startFakeCloudTts() {
  const state = { fail: {}, requests: [] };
  const server = http.createServer(async (req, res) => {
    const url = new URL(req.url, 'http://x');
    let raw = '';
    for await (const chunk of req) raw += chunk;
    let body = null;
    try {
      body = raw ? JSON.parse(raw) : null;
    } catch {
      body = null;
    }
    const provider =
      url.pathname === '/v1/audio/speech' ? 'openai' : url.pathname === '/v1beta/interactions' ? 'gemini' : url.pathname === '/openai/v1/audio/speech' ? 'groq' : '';
    const auth = provider === 'gemini' ? req.headers['x-goog-api-key'] ?? null : req.headers.authorization ?? null;
    state.requests.push({ provider, method: req.method, path: url.pathname, query: url.search, auth, body });
    if (req.method !== 'POST' || !provider) {
      json(res, 404, { error: { message: 'Not found' } });
      return;
    }
    const fail = state.fail[provider];

    if (provider === 'openai') {
      if (auth !== `Bearer ${OPENAI_KEY}`) return openAiError(res, 401, 'invalid_api_key', 'Incorrect API key provided');
      if (fail === 429) return openAiError(res, 429, 'insufficient_quota', 'You exceeded your current quota');
      if (fail) return openAiError(res, fail, 'server_error', 'Something went wrong');
      const pcm = pcmFor(body?.input || '');
      if (body?.stream_format === 'sse') {
        res.writeHead(200, { 'Content-Type': 'text/event-stream', 'Transfer-Encoding': 'chunked' });
        let text = '';
        for (let i = 0; i < pcm.length; i += 4_800) {
          text += `data: ${JSON.stringify({ type: 'speech.audio.delta', audio: pcm.subarray(i, i + 4_800).toString('base64') })}\n\n`;
        }
        text += `data: ${JSON.stringify({ type: 'speech.audio.done', usage: { input_tokens: 12, output_tokens: 90, total_tokens: 102 } })}\n\n`;
        await inPieces(res, Buffer.from(text), 3_001);
      } else {
        res.writeHead(200, { 'Content-Type': 'audio/pcm', 'Transfer-Encoding': 'chunked' });
        await inPieces(res, pcm);
      }
      return;
    }

    if (provider === 'gemini') {
      if (auth !== GEMINI_KEY) return googleError(res, 400, 'INVALID_ARGUMENT', 'API key not valid. Please pass a valid API key.');
      if (fail === 429) return googleError(res, 429, 'RESOURCE_EXHAUSTED', 'Resource has been exhausted');
      if (fail) return googleError(res, fail, 'INTERNAL', 'Internal error');
      const text = body?.input?.[0]?.content?.[0]?.text || '';
      const pcm = pcmFor(text);
      res.writeHead(200, { 'Content-Type': 'text/event-stream', 'Transfer-Encoding': 'chunked' });
      const ev = (name, data) => `event: ${name}\ndata: ${JSON.stringify({ event_type: name, ...data })}\n\n`;
      let out = ev('interaction.created', { interaction: { id: 'e2e', status: 'in_progress' } });
      out += ev('step.start', { index: 0, step: { type: 'model_output' } });
      for (let i = 0; i < pcm.length; i += 4_800) {
        out += ev('step.delta', { index: 0, delta: { type: 'audio', data: pcm.subarray(i, i + 4_800).toString('base64') } });
      }
      out += ev('step.stop', { index: 0 });
      out += ev('interaction.completed', {
        interaction: { id: 'e2e', status: 'completed', usage: { total_tokens: 110, total_input_tokens: 10, total_output_tokens: 100 } },
      });
      out += 'event: done\ndata: [DONE]\n\n';
      await inPieces(res, Buffer.from(out), 2_049);
      return;
    }

    // Groq
    if (auth !== `Bearer ${GROQ_KEY}`) return openAiError(res, 401, 'invalid_api_key', 'Invalid API Key');
    if (fail === 429) return openAiError(res, 429, 'rate_limit_exceeded', 'Rate limit reached');
    if (fail) return openAiError(res, fail, 'server_error', 'Something went wrong');
    const input = String(body?.input || '');
    if ([...input].length > 200) return openAiError(res, 400, 'invalid_request_error', 'Input must be at most 200 characters');
    res.writeHead(200, { 'Content-Type': 'audio/wav', 'Transfer-Encoding': 'chunked' });
    await inPieces(res, wavOf(pcmFor(input)), 1_003);
  });
  await new Promise((r) => server.listen(0, '127.0.0.1', r));
  const base = `http://127.0.0.1:${server.address().port}`;
  return {
    base,
    env: { YAP_OPENAI_TTS_URL: base, YAP_GEMINI_TTS_URL: base, YAP_GROQ_TTS_URL: base },
    get requests() {
      return state.requests;
    },
    /** The speech requests to `provider` so far. */
    speech: (provider) => state.requests.filter((r) => r.method === 'POST' && (!provider || r.provider === provider)),
    /** Answer `provider`'s speech with this HTTP error (429, 500…), or not (`null`). */
    fail(provider, status) {
      state.fail[provider] = status || null;
    },
    reset() {
      state.fail = {};
      state.requests.length = 0;
    },
    close: () => new Promise((r) => server.close(r)),
  };
}
