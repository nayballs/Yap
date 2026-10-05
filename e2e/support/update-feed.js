// A local stand-in for GitHub's `latest.json`: announces a newer Yap and
// serves a signed dummy "installer". The test instance is pointed at it with
// the debug-only YAP_UPDATE_TEST_* variables (src-tauri/src/updates.rs). The
// signing key is a throwaway made fresh for each run with `tauri signer`.
import { spawnSync } from 'node:child_process';
import crypto from 'node:crypto';
import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';
import { REPO } from './yap.js';

const CLI = path.join(REPO, 'node_modules', '@tauri-apps', 'cli', 'tauri.js');

function tauri(args) {
  // Never let a real signing key from the environment near this.
  const env = Object.fromEntries(
    Object.entries(process.env).filter(([k]) => !k.startsWith('TAURI_SIGNING_'))
  );
  const r = spawnSync(process.execPath, [CLI, ...args], { env, encoding: 'utf8' });
  if (r.status !== 0) throw new Error(`tauri ${args.join(' ')} failed:\n${r.stdout}\n${r.stderr}`);
}

export async function startUpdateFeed(dir, { version, notes }) {
  fs.mkdirSync(dir, { recursive: true });
  const key = path.join(dir, 'updater-test.key');
  tauri(['signer', 'generate', '--ci', '--force', '--password', '', '--write-keys', key]);
  const file = `Yap_${version}_x64-setup.exe`;
  const payload = path.join(dir, file);
  fs.writeFileSync(payload, crypto.randomBytes(256 * 1024));
  tauri(['signer', 'sign', '--private-key-path', key, '--password', '', payload]);

  const requests = [];
  const server = http.createServer((req, res) => {
    requests.push(req.url);
    if (req.url === '/latest.json') {
      res.writeHead(200, { 'content-type': 'application/json' });
      res.end(JSON.stringify(feed));
    } else if (req.url === `/${file}`) {
      res.writeHead(200, {
        'content-type': 'application/octet-stream',
        'content-length': fs.statSync(payload).size,
      });
      fs.createReadStream(payload).pipe(res);
    } else {
      res.writeHead(404).end();
    }
  });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  const base = `http://127.0.0.1:${server.address().port}`;
  const platform = { signature: fs.readFileSync(`${payload}.sig`, 'utf8').trim(), url: `${base}/${file}` };
  const feed = {
    version,
    notes,
    pub_date: new Date().toISOString(),
    platforms: { 'windows-x86_64': platform, 'windows-x86_64-nsis': platform },
  };

  return {
    endpoint: `${base}/latest.json`,
    pubkey: fs.readFileSync(`${key}.pub`, 'utf8').trim(),
    requests,
    close: () => new Promise((resolve) => server.close(resolve)),
  };
}
