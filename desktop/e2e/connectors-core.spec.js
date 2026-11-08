// Connectors (preview), the core through its commands (no Settings page yet:
// wave 5). A program on this PC (support/fake-mcp.js over stdio) is shown
// before it runs, its tools listed with their settings and called, a slow call
// stopped and the program told, its grandchild ended with its job; links
// (fake-mcp.js --http) with a token, a 401, no token, a redirect off the host,
// a slow server and one that never answers; every request in the receipts with
// the connector's purpose, and no secret in any file. Without the preview's
// switch nothing about connectors runs. Fictional data only.
import fs from 'node:fs';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { test, expect } from './support/fixtures.js';
import { RUNS, launchYap } from './support/yap.js';

const FAKE = fileURLToPath(new URL('./support/fake-mcp.js', import.meta.url));
const NODE = process.execPath;
const TOKEN = 'tok-FAKE-7c3e9a1b5d';
const DIR = path.join(RUNS, 'connectors-core-fakes');

test.use({ yapOptions: { name: 'connectors-core', env: { YAP_CONNECTORS: '1', YAP_E2E_CONNECTOR_START_MS: '2000' } } });

const lines = (file) =>
  fs.existsSync(file)
    ? fs
        .readFileSync(file, 'utf8')
        .split('\n')
        .filter(Boolean)
        .map((l) => JSON.parse(l))
    : [];

const alive = (pid) => {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
};

/** An invoke that fails: its message. */
const refused = (p) => p.then((v) => `(no error: ${JSON.stringify(v)})`, (e) => String(e?.message ?? e));

const servers = [];

/** fake-mcp.js --http on a free port, logging to its own file. */
async function fakeHttp(name, args = []) {
  fs.mkdirSync(DIR, { recursive: true });
  const portFile = path.join(DIR, `${name}.port`);
  const logFile = path.join(DIR, `${name}.log`);
  fs.rmSync(portFile, { force: true });
  fs.rmSync(logFile, { force: true });
  const child = spawn(NODE, [FAKE, '--http', '0', '--port-file', portFile, '--log', logFile, ...args], { stdio: 'ignore', windowsHide: true });
  servers.push(child);
  await expect.poll(() => (fs.existsSync(portFile) ? fs.readFileSync(portFile, 'utf8') : ''), { message: `${name} to listen` }).not.toBe('');
  const port = Number(fs.readFileSync(portFile, 'utf8'));
  return { url: `http://127.0.0.1:${port}/mcp`, log: () => lines(logFile).filter((l) => l.kind === 'http') };
}

test.afterAll(() => {
  for (const s of servers) s.kill();
});

/** Hear app event `event` in the main window from now on. */
async function listen(main, event) {
  await main.evaluate(async (ev) => {
    window.__heard = window.__heard || {};
    window.__heard[ev] = [];
    const handler = window.__TAURI_INTERNALS__.transformCallback((e) => window.__heard[ev].push(e.payload));
    await window.__TAURI_INTERNALS__.invoke('plugin:event|listen', { event: ev, target: { kind: 'Any' }, handler });
  }, event);
}
const heard = (main, event) => main.evaluate((ev) => window.__heard?.[ev] ?? [], event);

const read = (yap, file) => {
  const p = path.join(yap.dataDir, file);
  return fs.existsSync(p) ? fs.readFileSync(p, 'utf8') : '';
};

test('a program is shown before it runs, then listed, called, stopped, and its grandchild ends with it', async ({ yap, main }) => {
  fs.mkdirSync(DIR, { recursive: true });
  const log = path.join(DIR, 'stdio.log');
  const pidFile = path.join(DIR, 'grandchild.pid');
  fs.rmSync(log, { force: true });
  fs.rmSync(pidFile, { force: true });
  await listen(main, 'yap-connector-status');
  const spec = {
    name: 'Home lights',
    command: NODE,
    args: [FAKE, '--log', log, '--stderr', '--spawn-child', '--pid-file', pidFile],
    env: [
      { name: 'HOME_TOKEN', value: TOKEN },
      { name: 'LOG_LEVEL', value: 'debug' },
    ],
  };

  // The whole command line, resolved, with what it can do.
  const preview = await yap.invoke('connector_preview', { spec });
  expect(preview.kind).toBe('local');
  expect(preview.program.resolved.toLowerCase()).toBe(NODE.toLowerCase());
  expect(preview.program.commandLine).toContain('fake-mcp.js');
  expect(preview.program.commandLine).toContain('--spawn-child');
  expect(preview.warnings[0]).toContain('It can read and change anything you can');
  // Not seen first: refused, nothing saved.
  expect(await refused(yap.invoke('connector_add', { spec }))).toContain('look at it again');
  expect(read(yap, 'connectors.json')).toBe('');

  const added = await yap.invoke('connector_add', { spec: { ...spec, approved: preview.program.fingerprint } });
  const id = added.id;
  expect(added.status).toMatchObject({ state: 'connected', line: 'Connected · 9 tools' });
  expect(added.kind).toBe('local');
  expect(added.env).toEqual([{ name: 'HOME_TOKEN', secret: true }, { name: 'LOG_LEVEL', value: 'debug' }]);
  const setting = Object.fromEntries(added.tools.map((t) => [t.name, t.setting]));
  expect(setting).toEqual({
    list_lights: 'allow',
    turn_off: 'ask',
    delete_all: 'ask', // no annotations: it can delete
    slow: 'allow',
    crash: 'allow',
    inject: 'off', // its description reads like orders to the model
    big_schema: 'allow',
    rug_pull: 'allow',
    needs_input: 'off', // its inputs aren't an object
    picture: 'allow',
  });
  expect(added.tools.find((t) => t.name === 'inject').flag).toContain('reads like instructions');
  expect(added.tools.find((t) => t.name === 'needs_input').skipped).toContain("aren't described as an object");
  expect(added.tools[0].wire).toBe('homelights__list_lights');
  // The program got its own variables, and none of Yap's.
  expect(lines(log).find((l) => l.kind === 'env').names).toEqual(['HOME_TOKEN', 'LOG_LEVEL']);

  // A call, shaped as Yap's tools answer.
  expect(await yap.invoke('connector_call_tool', { id, tool: 'list_lights' })).toEqual({
    success: true,
    data: { text: 'Kitchen: on\nHall: off' },
    displayText: 'A connector · list_lights',
  });
  const pic = await yap.invoke('connector_call_tool', { id, tool: 'picture' });
  expect(pic.data).toEqual({ text: 'The floor plan:\n\n[a picture, 2×1]', pictures: 1 });
  // A slow one: stopped after its timeout, and the program told.
  const slow = await yap.invoke('connector_call_tool', { id, tool: 'slow', args: { ms: 20_000 }, timeoutMs: 800 });
  expect(slow).toMatchObject({ success: false, error: 'No answer from A connector in 1 s, so Yap stopped waiting.' });
  await expect.poll(() => lines(log).some((l) => l.kind === 'cancelled-slow'), { message: 'the program told to stop' }).toBe(true);
  // Off: never sent.
  expect(await refused(yap.invoke('connector_call_tool', { id, tool: 'inject' }))).toContain('That tool is off');
  expect(lines(log).some((l) => l.tool === 'inject')).toBe(false);

  // Show details: the server, and what it said on stderr (in memory only).
  const details = await yap.invoke('connector_details', { id });
  expect(details.server).toEqual({ name: 'fake-mcp', version: '1.2.0', protocol: '2025-11-25' });
  // (Node may warn about the environment first.)
  expect(details.stderr.slice(-2)).toEqual(['fake-mcp: starting', 'fake-mcp: listening on stdio']);

  // The secret is in no file Yap keeps (only the test run's stand-in vault).
  for (const f of ['connectors.json', 'connector_tools.json', 'config.json']) {
    expect(read(yap, f), f).not.toContain(TOKEN);
  }
  expect(JSON.parse(read(yap, 'connectors.json')).connectors[0].transport.env[0]).toEqual({ name: 'HOME_TOKEN', secret: true });

  // Its grandchild lives with it, and goes with it: removed, the program
  // is stopped and its job ends everything it started.
  await expect.poll(() => fs.existsSync(pidFile)).toBe(true);
  const grandchild = Number(fs.readFileSync(pidFile, 'utf8'));
  expect(alive(grandchild)).toBe(true);
  await yap.invoke('connector_remove', { id });
  await expect.poll(() => alive(grandchild), { timeout: 15_000, message: 'the grandchild to end with the job' }).toBe(false);
  expect((await yap.invoke('connectors_list')).connectors).toEqual([]);

  // Every status was told.
  const states = async () => (await heard(main, 'yap-connector-status')).filter((s) => s.id === id).map((s) => s.state);
  await expect.poll(async () => (await states()).at(-1), { message: 'the last status told' }).toBe('off');
  expect((await states()).slice(0, 2)).toEqual(['starting', 'connected']);
});

test('a link with a token lists and calls, every request in the receipts, its token in no file', async ({ yap }) => {
  const srv = await fakeHttp('token', ['--token', TOKEN]);
  const v = await yap.invoke('connector_add', { spec: { name: 'Fake service', url: srv.url, token: TOKEN } });
  expect(v.status).toMatchObject({ state: 'connected', line: 'Connected · 9 tools' });
  expect(v).toMatchObject({ kind: 'remote', auth: 'bearer', hasSecrets: true, address: srv.url });
  expect((await yap.invoke('connector_call_tool', { id: v.id, tool: 'turn_off', args: { area: 'hall' } })).data.text).toBe('Turned off the hall lights.');
  const said = srv.log();
  expect(said.length).toBeGreaterThan(2);
  expect(said.every((l) => l.auth)).toBe(true);
  expect(said.some((l) => l.rpc === 'tools/call')).toBe(true);

  // Network connections: the connector's purpose and host, nothing more.
  await expect
    .poll(async () => {
      const groups = (await yap.invoke('connections_summary')).groups;
      return groups.find((g) => g.label === 'Connector · A connector')?.hosts.map((h) => [h.host, h.place, h.features]) ?? null;
    }, { timeout: 15_000 })
    .toEqual([['127.0.0.1', 'pc', ['Connectors']]]);
  for (const f of ['connectors.json', 'connector_tools.json', 'config.json', 'connections.json']) {
    expect(read(yap, f), f).not.toContain(TOKEN);
  }

  // Off: stopped, calls refused; on again and Try again: back.
  expect((await yap.invoke('connector_set_enabled', { id: v.id, enabled: false })).status.state).toBe('off');
  expect((await yap.invoke('connector_call_tool', { id: v.id, tool: 'list_lights' }).catch(String)).toString()).toContain('turned off');
  await yap.invoke('connector_set_enabled', { id: v.id, enabled: true });
  const again = await yap.invoke('connector_refresh_tools', { id: v.id });
  expect(again.connector.status.state).toBe('connected');
  expect(again.diff).toEqual({ added: [], changed: ['rug_pull'], askedAgain: ['rug_pull'], removed: [] });
  expect(again.connector.tools.find((t) => t.name === 'rug_pull').setting).toBe('ask');
  // A tool that can delete is never Always allow.
  expect(await refused(yap.invoke('connector_set_tool', { id: v.id, tool: 'delete_all', setting: 'allow' }))).toContain('always asks first');
  await yap.invoke('connector_remove', { id: v.id });
});

test('a token turned down, no token, a redirect off the host and no answer each say why', async ({ yap }) => {
  const strict = await fakeHttp('strict', ['--token', TOKEN]);
  const wrong = await yap.invoke('connector_add', { spec: { name: 'Wrong token', url: strict.url, token: 'nope-FAKE' } });
  expect(wrong.status).toEqual({
    id: wrong.id,
    state: 'failed',
    why: "Couldn't connect: 127.0.0.1 turned the token down (401). Make a new one, then paste it in again.",
    line: "Couldn't connect: 127.0.0.1 turned the token down (401). Make a new one, then paste it in again.",
  });
  const none = await yap.invoke('connector_add', { spec: { name: 'No token', url: strict.url } });
  expect(none.status.state).toBe('needsSignIn');
  expect(none.status.line).toMatch(/^Needs sign-in\. .*use a token if it offers one\.$/);

  const away = await fakeHttp('away', ['--redirect']);
  const redirected = await yap.invoke('connector_add', { spec: { name: 'Away', url: away.url, token: TOKEN } });
  expect(redirected.status.line).toBe("Couldn't connect: 127.0.0.1 sent Yap on to localhost. Yap only talks to the server you added, so it stopped there.");
  // Nothing (the token least of all) ever went to the other host.
  expect(away.log().length).toBeGreaterThan(0);
  expect(away.log().every((l) => l.host.startsWith('127.0.0.1:'))).toBe(true);

  const silent = await fakeHttp('silent', ['--hang']);
  const started = Date.now();
  const hung = await yap.invoke('connector_add', { spec: { name: 'Silent', url: silent.url } });
  expect(hung.status.line).toBe("Couldn't connect: no answer from 127.0.0.1 in 2 s.");
  expect(Date.now() - started).toBeLessThan(10_000);

  const slow = await fakeHttp('slow', ['--slow-ms', '200']);
  expect((await yap.invoke('connector_add', { spec: { name: 'Slow', url: slow.url } })).status.state).toBe('connected');

  // An address on the internet needs https; a metadata address is never one.
  expect(await refused(yap.invoke('connector_preview', { spec: { name: 'Plain', url: 'http://mcp.example.com/mcp' } }))).toContain('https');
  expect(await refused(yap.invoke('connector_preview', { spec: { name: 'Meta', url: 'http://169.254.169.254/' } }))).toContain('metadata');
  for (const c of (await yap.invoke('connectors_list')).connectors) await yap.invoke('connector_remove', { id: c.id });
});

test('without the preview switch nothing about connectors runs', async () => {
  // An instance of its own, without YAP_CONNECTORS (connectorsPreview off).
  const off = await launchYap({ name: 'connectors-off' });
  try {
    expect(await off.invoke('connectors_list')).toEqual({ enabled: false, connectors: [] });
    expect(await refused(off.invoke('connector_add', { spec: { name: 'x', url: 'http://127.0.0.1:9/mcp' } }))).toContain("aren't switched on");
    expect(await refused(off.invoke('connector_preview', { spec: { name: 'x', command: NODE } }))).toContain("aren't switched on");
    expect(fs.existsSync(path.join(off.dataDir, 'connectors.json'))).toBe(false);
  } finally {
    await off.stop();
  }
});
