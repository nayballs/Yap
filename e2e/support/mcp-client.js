// A minimal MCP client, the way AI apps run Yap's server: spawn `yap.exe mcp`
// with piped stdio and exchange newline-delimited JSON-RPC (see
// src-tauri/src/mcp.rs). Used by mcp.spec.js.
import { spawn } from 'node:child_process';
import readline from 'node:readline';

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/**
 * @param {string} exe  a test build's yap.exe
 * @param {Record<string,string>} env  e.g. { YAP_BRIDGE_FILE }
 */
export function startMcp(exe, env = {}) {
  // Nothing from the developer's shell that could point it elsewhere.
  const base = Object.fromEntries(Object.entries(process.env).filter(([k]) => !/^YAP_/.test(k)));
  const child = spawn(exe, ['mcp'], {
    stdio: ['pipe', 'pipe', 'pipe'],
    env: { ...base, ...env },
    windowsHide: true,
  });
  child.stdin.on('error', () => {});
  const pending = new Map();
  const notJson = [];
  let stderr = '';
  readline.createInterface({ input: child.stdout }).on('line', (line) => {
    let msg;
    try {
      msg = JSON.parse(line);
    } catch {
      notJson.push(line); // stdout must carry protocol messages only
      return;
    }
    const resolve = pending.get(msg.id);
    if (resolve) {
      pending.delete(msg.id);
      resolve(msg);
    }
  });
  child.stderr.on('data', (d) => (stderr += d));

  let nextId = 1;
  const send = (msg) => child.stdin.write(`${JSON.stringify(msg)}\n`);
  const request = (method, params = {}) =>
    new Promise((resolve, reject) => {
      const id = nextId++;
      const timer = setTimeout(() => reject(new Error(`no reply to ${method}; stderr:\n${stderr}`)), 15_000);
      pending.set(id, (msg) => {
        clearTimeout(timer);
        resolve(msg);
      });
      send({ jsonrpc: '2.0', id, method, params });
    });

  return {
    request,
    notJson,
    get stderr() {
      return stderr;
    },
    /** The 2025 handshake every client in use today does. */
    async initialize(protocolVersion = '2025-06-18') {
      const r = await request('initialize', {
        protocolVersion,
        capabilities: {},
        clientInfo: { name: 'yap-e2e', version: '1.0' },
      });
      send({ jsonrpc: '2.0', method: 'notifications/initialized' });
      return r;
    },
    async toolNames() {
      const r = await request('tools/list');
      return r.result.tools.map((t) => t.name);
    },
    /** `{ isError, text }` of one tool call. */
    async tool(name, args = {}) {
      const r = await request('tools/call', { name, arguments: args });
      if (r.error) throw new Error(`${name}: ${JSON.stringify(r.error)}`);
      return { isError: r.result.isError === true, text: r.result.content?.[0]?.text ?? '' };
    },
    /** Hang up like a client (close stdin); resolves with the exit code. */
    async close() {
      if (child.exitCode !== null) return child.exitCode;
      child.stdin.end();
      const code = await Promise.race([
        new Promise((r) => child.once('exit', (c) => r(c))),
        sleep(10_000).then(() => 'timeout'),
      ]);
      if (code === 'timeout') child.kill();
      return code;
    },
  };
}
