// A stand-in for meeting search's embedding model (src-tauri/src/search/
// embedder.rs: EmbeddingGemma on llamafile's `--embedding` server), which a
// test run never starts: Yap is pointed here with the debug-only
// YAP_E2E_EMBED_URL. `POST /v1/embeddings {input: [...]}` answers as
// llama.cpp does — `{data: [{index, embedding}]}` — with 768-dimension
// vectors made from each text's words, hashed into the dimensions after a
// few words are put in a shared sense ("teeth" and "dental" are one, "staff"
// and "hiring" another), so a search by meaning finds what a keyword search
// can't, deterministically. The prompts' own words ("title: … | text: …",
// "task: search result | query: …") are left out. `requests` keeps how
// many texts each request carried (never the texts' order of arrival).
import http from 'node:http';

const DIMS = 768;

// Words that mean the same thing here: the first is what each becomes.
const SENSES = [
  ['dental', 'teeth', 'tooth', 'dentist', 'cleaning'],
  ['hiring', 'staff', 'hire', 'recruit', 'freeze'],
  ['money', 'cost', 'price', 'quote', 'pound', 'budget', 'spend'],
  ['launch', 'ship', 'release', 'beta'],
  ['offsite', 'trip', 'away'],
];
const STOP = new Set(['the', 'a', 'an', 'and', 'or', 'of', 'to', 'in', 'on', 'at', 'for', 'is', 'was', 'what', 'when', 'did', 'do', 'does', 'we', 'i', 'you', 'it', 'my', 'our', 'with', 'by', 'that', 'this', 'title', 'text', 'task', 'search', 'result', 'query', 'none']);

function sense(word) {
  for (const group of SENSES) if (group.includes(word)) return group[0];
  return word;
}

function hash(word) {
  let h = 2166136261;
  for (const ch of word) h = Math.imul(h ^ ch.codePointAt(0), 16777619) >>> 0;
  return h;
}

/** A text's vector: its words' senses, hashed, unit length. */
export function vectorOf(text) {
  const v = new Array(DIMS).fill(0);
  const words = String(text)
    .toLowerCase()
    .split(/[^a-z0-9]+/)
    .filter((w) => w.length > 1 && !STOP.has(w))
    .map((w) => sense(w.replace(/s$/, '')) || w);
  for (const w of words) {
    const h = hash(w);
    v[h % DIMS] += 1;
    v[(h >>> 11) % DIMS] += 0.5;
  }
  const n = Math.hypot(...v) || 1;
  return v.map((x) => x / n);
}

export async function startFakeEmbed() {
  const requests = [];
  let failing = 0;
  const server = http.createServer((req, res) => {
    let raw = '';
    req.on('data', (c) => (raw += c));
    req.on('end', () => {
      if (req.method !== 'POST' || !req.url.endsWith('/embeddings')) {
        res.writeHead(404).end();
        return;
      }
      if (failing > 0) {
        failing -= 1;
        res.writeHead(500, { 'content-type': 'application/json' }).end(JSON.stringify({ error: { message: 'fake embedding failure' } }));
        return;
      }
      let body;
      try {
        body = JSON.parse(raw);
      } catch {
        res.writeHead(400).end();
        return;
      }
      const input = Array.isArray(body.input) ? body.input : [body.input];
      requests.push({ texts: input.length, query: input.some((t) => String(t).startsWith('task: search result')) });
      res.writeHead(200, { 'content-type': 'application/json' });
      res.end(JSON.stringify({ object: 'list', model: body.model, data: input.map((t, index) => ({ object: 'embedding', index, embedding: vectorOf(t) })) }));
    });
  });
  await new Promise((r) => server.listen(0, '127.0.0.1', r));
  return {
    url: `http://127.0.0.1:${server.address().port}/v1`,
    requests,
    /** The next `n` requests answer HTTP 500. */
    failNext: (n = 1) => (failing = n),
    close: () =>
      new Promise((r) => {
        server.closeAllConnections?.();
        server.close(r);
      }),
  };
}
