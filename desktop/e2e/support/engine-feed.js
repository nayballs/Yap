// A stand-in for llama.cpp's GitHub releases (desktop/src-tauri/src/ai/local_llm/
// engine/feed.rs): `/releases` answers like GitHub's releases API (builds as
// pre-releases, each Windows zip with its "sha256:" digest), and
// `/download/<tag>/<name>` serves the zips — a stand-in `llama-server.exe`
// and a DLL that never run. Yap is pointed at it with the debug-only
// YAP_E2E_ENGINE_FEED (a test run refuses any address but 127.0.0.1).
import crypto from 'node:crypto';
import http from 'node:http';
import { engineZip } from './local-llm.js';

const sha256 = (buf) => crypto.createHash('sha256').update(buf).digest('hex');
const DAY = 86_400_000;

/**
 * Serve releases for `builds` ([{ build: 11980, daysAgo: 4 }]), each with
 * Windows CPU and Vulkan zips (and an Ubuntu one Yap must leave alone).
 * `add()` publishes another later.
 */
export async function startEngineFeed(builds) {
  const releases = [];
  const files = new Map();
  const requests = [];

  function add({ build, daysAgo }) {
    const tag = `b${build}`;
    const assets = ['win-cpu-x64.zip', 'win-vulkan-x64.zip', 'ubuntu-x64.tar.gz'].map((kind) => {
      const name = `llama-${tag}-bin-${kind}`;
      const body = engineZip(`stand-in llama-server ${tag} (never run)`);
      files.set(`/download/${tag}/${name}`, body);
      return { name, size: body.length, state: 'uploaded', digest: `sha256:${sha256(body)}`, browser_download_url: `https://example.com/not-used/${name}` };
    });
    releases.push({ tag_name: tag, draft: false, prerelease: true, published_at: new Date(Date.now() - daysAgo * DAY).toISOString(), assets });
    releases.sort((a, b) => Number(b.tag_name.slice(1)) - Number(a.tag_name.slice(1)));
  }
  builds.forEach(add);
  // llama.cpp tags more than builds: never taken for one.
  releases.push({ tag_name: 'v0.8.0', draft: false, prerelease: false, published_at: new Date(Date.now() - 9 * DAY).toISOString(), assets: [] });

  const server = http.createServer((req, res) => {
    requests.push(req.url);
    const url = new URL(req.url, 'http://x');
    if (url.pathname === '/releases') {
      const page = Number(url.searchParams.get('page') || '1');
      res.writeHead(200, { 'content-type': 'application/json' });
      res.end(JSON.stringify(page === 1 ? releases : []));
      return;
    }
    const body = files.get(url.pathname);
    if (!body) {
      res.writeHead(404).end();
      return;
    }
    res.writeHead(200, { 'content-type': 'application/zip', 'content-length': body.length });
    res.end(body);
  });
  await new Promise((r) => server.listen(0, '127.0.0.1', r));
  const base = `http://127.0.0.1:${server.address().port}`;
  return {
    base,
    env: { YAP_E2E_ENGINE_FEED: base },
    requests,
    add,
    downloads: () => requests.filter((r) => r.startsWith('/download/')),
    close: () =>
      new Promise((r) => {
        server.closeAllConnections?.();
        server.close(r);
      }),
  };
}
