// Stand-ins for Yap's on-device model downloads (src-tauri/src/ai/local_llm.rs):
// a fake GGUF per curated id and a fake llamafile engine, served from
// 127.0.0.1 in pieces, slowly enough to see the progress; `hold()` stops a
// download half-way until `release()` (a screenshot of the bar). Yap is
// pointed at them with the debug-only YAP_LLM_SOURCES file (url + SHA-256 per
// id; "runtime" for the engine), so a test run never downloads a real model
// (and a test run refuses any address but 127.0.0.1 anyway). Nothing here
// ever runs: the engine is random bytes.
import crypto from 'node:crypto';
import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';

const sha256 = (buf) => crypto.createHash('sha256').update(buf).digest('hex');

/** Serve fake on-device models for `ids` and write the sources file into `dir`. */
export async function startFakeLocalModels(dir, ids = ['qwen3.5-4b', 'qwen2.5-1.5b', 'qwen2.5-3b']) {
  fs.mkdirSync(dir, { recursive: true });
  const bodies = { '/llm/runtime': crypto.randomBytes(64 * 1024) };
  for (const id of ids) bodies[`/llm/${id}.gguf`] = crypto.randomBytes(4 * 1024 * 1024);
  const requests = [];
  let gate = null; // { promise, open }

  const server = http.createServer(async (req, res) => {
    requests.push(req.url);
    const body = bodies[req.url];
    if (!body) {
      res.writeHead(404).end();
      return;
    }
    res.writeHead(200, { 'Content-Length': body.length, 'Content-Type': 'application/octet-stream' });
    // In 32 pieces, ~1.5 s in all; a held download stops at half.
    const piece = Math.ceil(body.length / 32);
    for (let i = 0, n = 0; i < body.length; i += piece, n += 1) {
      if (res.destroyed) return;
      if (n === 16 && gate) await gate.promise;
      res.write(body.subarray(i, i + piece));
      await new Promise((r) => setTimeout(r, 45));
    }
    res.end();
  });
  await new Promise((r) => server.listen(0, '127.0.0.1', r));
  const base = `http://127.0.0.1:${server.address().port}`;
  const sources = { runtime: { url: `${base}/llm/runtime`, sha256: sha256(bodies['/llm/runtime']) } };
  for (const id of ids) sources[id] = { url: `${base}/llm/${id}.gguf`, sha256: sha256(bodies[`/llm/${id}.gguf`]) };
  const sourcesFile = path.join(dir, 'llm-sources.json');
  fs.writeFileSync(sourcesFile, JSON.stringify(sources, null, 2));

  return {
    env: { YAP_LLM_SOURCES: sourcesFile },
    /** Each id's { url, sha256 } ("runtime" for the engine). */
    sources,
    requests,
    /** The next download stops half-way until `release()`. */
    hold() {
      let open;
      const promise = new Promise((r) => (open = r));
      gate = { promise, open };
    },
    release() {
      gate?.open();
      gate = null;
    },
    close: () =>
      new Promise((r) => {
        gate?.open();
        server.closeAllConnections?.();
        server.close(r);
      }),
  };
}

/**
 * Yap's on-device model "installed" in `dataDir` for a test run: an empty
 * llamafile engine and an empty model file. Nothing here runs: a spec that
 * needs the model to answer points YAP_E2E_LOCAL_LLM at a fake AI
 * (support/fake-llm.js), which then stands in for the started sidecar
 * (ai/local_llm.rs `e2e_stand_in`), up `YAP_E2E_LOCAL_LLM_LOAD_MS` after
 * it's asked to start.
 */
export function installStandInModel(dataDir, file = 'qwen2.5-1.5b-instruct-q4_k_m.gguf') {
  const llm = path.join(dataDir, 'llm');
  fs.mkdirSync(llm, { recursive: true });
  fs.writeFileSync(path.join(llm, 'llamafile.exe'), '');
  fs.writeFileSync(path.join(llm, file), '');
}
