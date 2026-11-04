// Pairing when the phone has more than one tab of the page (2026-11-03): a
// phone was locked out after Firefox restored its tabs, each with an old
// `#pair=` code in its address, each sending that code instead of the device
// it had — refused, and the refusals counted against the phone. Now a hello
// carries the device and any code the tab has (the PC counts the device
// first), the code leaves the address once the PC answers, a tab refused for
// an old device comes back when another tab pairs, and a code opened in a tab
// already open (only its fragment changes) is said at once.
//
// Yap on the PC here keeps to the real one's rules (link/pairing.rs `judge`,
// session.rs `hello_verdict`): the device first; the live code pairs, once;
// an old code or a wrong secret is refused and is a strike (five lock the
// address out); the real code used already is refused but isn't one.
import { test, expect } from '@playwright/test';
import { servePage, seedDevice, open, snapshot, pickerData, PC, DEVICE, MODELS, CAN, WS_URL, ORIGIN } from './support/harness.js';

const BAD_LINK = "This pairing link isn't Yap's latest. Pair again in Settings → Phone on your PC.";
const USED = 'This pairing link was already used. Pair again in Settings → Phone on your PC.';
const REMOVED = 'This phone was removed in Yap. Pair it again in Settings → Phone.';

/** A PC that judges hellos as Yap's does, shared by every tab of a test. */
function strictPc({ devices = [DEVICE], invite = null } = {}) {
  const pc = {
    devices: [...devices],
    // The live invitation: { token, used }.
    invite,
    strikes: 0,
    hellos: [],
    denied: [],
    conns: [],
    alreadyPaired: 0,
  };
  const send = (ws, msg) => ws.send(JSON.stringify(msg));
  const ready = (ws) => send(ws, { type: 'ready', pc: PC, model: MODELS[0], models: MODELS, can: CAN, picker: pickerData() });
  const refuse = (ws, reason, strike) => {
    if (pc.strikes >= 5) reason = 'Too many tries from this phone. Try again in 9 minutes.';
    else if (strike) pc.strikes++;
    pc.denied.push(reason);
    send(ws, { type: 'denied', reason });
    ws.close();
  };
  pc.judge = (ws, hello, conn) => {
    const d = hello.device;
    const token = hello.pair && hello.pair.token;
    const known = d && pc.devices.find((x) => x.id === d.id && x.secret === d.secret);
    const live = pc.invite && !pc.invite.used && token === pc.invite.token;
    if (known) {
      if (live) {
        pc.invite.used = true;
        pc.alreadyPaired++;
      }
      conn.device = known.id;
      return ready(ws);
    }
    if (!token) return refuse(ws, REMOVED, true);
    if (live) {
      pc.invite.used = true;
      conn.pairing = true;
      return send(ws, { type: 'pair_wait', code: '4821' });
    }
    if (pc.invite && token === pc.invite.token) return refuse(ws, USED, false);
    return refuse(ws, BAD_LINK, true);
  };
  /** Allow on the PC: the waiting tab paired with `device`, then connected. */
  pc.allow = (device) => {
    const conn = pc.conns.find((c) => c.pairing && !c.closed);
    pc.devices.push(device);
    conn.pairing = false;
    conn.device = device.id;
    send(conn.ws, { type: 'paired', device });
    ready(conn.ws);
  };
  pc.attach = (page) =>
    page.routeWebSocket(WS_URL, (ws) => {
      const conn = { ws, page, hellos: [], closed: false };
      pc.conns.push(conn);
      ws.onMessage((m) => {
        if (typeof m !== 'string') return;
        const msg = JSON.parse(m);
        if (msg.type === 'ping') return send(ws, { type: 'pong' });
        if (msg.type === 'chats') return send(ws, { type: 'chats', items: [] });
        if (msg.type !== 'hello') return;
        pc.hellos.push({ page, ...msg });
        conn.hellos.push(msg);
        pc.judge(ws, msg, conn);
      });
      ws.onClose(() => {
        conn.closed = true;
      });
    });
  pc.hellosFrom = (page) => pc.hellos.filter((h) => h.page === page);
  return pc;
}

/** `n` more tabs in the test's browser (they share its storage, as a phone's tabs do). */
async function tabs(page, n, pc, { seed = false } = {}) {
  const all = [page];
  for (let i = 0; i < n; i++) all.push(await page.context().newPage());
  for (const tab of all) {
    await servePage(tab);
    if (seed) await seedDevice(tab);
    await pc.attach(tab);
  }
  return all;
}

const connected = (tab) => expect(tab.getByTestId('conn')).toHaveText(`Connected to ${PC}`);

test('a saved device and an old #pair= in the address: the hello carries both, it connects, the code leaves the address', async ({
  page,
}) => {
  const pc = strictPc({ invite: { token: 'tok-new', used: false } });
  await tabs(page, 0, pc, { seed: true });
  await open(page, '#pair=tok-from-this-morning');
  await connected(page);
  expect(pc.hellos).toHaveLength(1);
  const hello = pc.hellos[0];
  expect(hello.device).toEqual(DEVICE);
  expect(hello.pair).toEqual({ token: 'tok-from-this-morning' });
  // Connected as any tab is: never the pairing screen with its code.
  await expect(page.getByTestId('pair-code')).toBeHidden();
  await expect(page.getByTestId('talk')).toBeEnabled();
  // The code left the address: a reload, or a restored tab, never brings it again.
  expect(await page.evaluate(() => location.hash)).toBe('');
  expect(page.url()).toBe(`${ORIGIN}/`);
  expect((await snapshot(page)).pairing).toBe(false);
  // Nothing refused, the invitation still waiting for its phone.
  expect(pc.denied).toEqual([]);
  expect(pc.invite.used).toBe(false);
  await page.reload();
  await connected(page);
  expect(pc.hellos[1].device).toEqual(DEVICE);
  expect(pc.hellos[1].pair).toBeUndefined();
});

test('six tabs restored at once, each with an old code: every one connects, and the phone is never locked out', async ({ page }) => {
  // The code it paired with (used), and older ones from earlier scans.
  const pc = strictPc({ invite: { token: 'tok-3', used: true } });
  const all = await tabs(page, 5, pc, { seed: true });
  const codes = ['tok-3', 'tok-2', 'tok-1', 'tok-0', 'tok-3', 'tok-2'];
  await Promise.all(all.map((tab, i) => open(tab, `#pair=${codes[i]}`)));
  await Promise.all(all.map(connected));
  expect(pc.denied).toEqual([]);
  expect(pc.strikes).toBe(0);
  // One hello each, with the device and its code.
  for (const [i, tab] of all.entries()) {
    const sent = pc.hellosFrom(tab);
    expect(sent, `tab ${i}`).toHaveLength(1);
    expect(sent[0].device).toEqual(DEVICE);
    expect(sent[0].pair).toEqual({ token: codes[i] });
    expect(await tab.evaluate(() => location.hash)).toBe('');
  }
  // Back from the background, they say hello with the device alone.
  for (const tab of all) await tab.reload();
  await Promise.all(all.map(connected));
  expect(pc.hellos.slice(6).every((h) => h.device && !h.pair)).toBe(true);
  expect(pc.denied).toEqual([]);
});

test('a fresh code pairs after old tabs were refused, and the refused tabs come back with the new device', async ({ page }) => {
  // No device on this phone (a new browser, or its data cleared); three tabs
  // with old codes, refused; then the fresh code from Settings.
  const pc = strictPc({ devices: [], invite: { token: 'tok-fresh', used: false } });
  const all = await tabs(page, 3, pc);
  const [old1, old2, old3, fresh] = all;
  await Promise.all([open(old1, '#pair=tok-a'), open(old2, '#pair=tok-b'), open(old3, '#pair=tok-c')]);
  for (const tab of [old1, old2, old3]) {
    await expect(tab.getByTestId('pair-why')).toHaveText(BAD_LINK);
    await expect(tab.getByTestId('conn')).toHaveText('Not paired');
  }
  // One hello each, refused; none tries again by itself.
  await page.waitForTimeout(1500);
  expect(pc.hellos).toHaveLength(3);
  // The fresh code: the pairing code to check, then Allow on the PC.
  await open(fresh, '#pair=tok-fresh');
  await expect(fresh.getByTestId('pair-code')).toHaveText('4821');
  const paired = { id: 'dev-test-2', secret: 'test-secret-2' };
  pc.allow(paired);
  await connected(fresh);
  expect(await fresh.evaluate(() => JSON.parse(localStorage.getItem('yap.link.device')))).toEqual(paired);
  // The tabs refused before come back with it, once each, with no code.
  await Promise.all([old1, old2, old3].map(connected));
  for (const tab of [old1, old2, old3]) {
    const sent = pc.hellosFrom(tab);
    expect(sent).toHaveLength(2);
    expect(sent[1].device).toEqual(paired);
    expect(sent[1].pair).toBeUndefined();
  }
  expect(pc.denied).toEqual([BAD_LINK, BAD_LINK, BAD_LINK]);
});

test('a tab still holding a removed device gets the new one another tab paired, at its next hello', async ({ page }) => {
  // Paired again in another tab (the old device removed on the PC) while
  // this one slept: its next hello says the new device, not the old one.
  const removed = { id: 'dev-old', secret: 'old-secret' };
  const fresh = { id: 'dev-new', secret: 'new-secret' };
  const pc = strictPc({ devices: [removed] });
  const [tab] = await tabs(page, 0, pc);
  await seedDevice(tab, removed);
  await open(tab);
  await connected(tab);
  // On the PC: the old device removed, the new one paired in another tab,
  // which kept it in the storage every tab shares (a sleeping tab can miss
  // the storage event: written here, in this tab, it gets none).
  pc.devices = [fresh];
  await tab.evaluate((d) => localStorage.setItem('yap.link.device', JSON.stringify(d)), fresh);
  // This tab's connection drops (the phone slept); it reconnects with the new one.
  pc.conns.find((c) => c.page === tab).ws.close();
  await expect.poll(() => pc.hellosFrom(tab).length, { timeout: 8000 }).toBe(2);
  expect(pc.hellosFrom(tab)[1].device).toEqual(fresh);
  await connected(tab);
  expect(pc.denied).toEqual([]);
});

test('a code opened in the tab already open (only the fragment changes): the hello goes again with it', async ({ page }) => {
  const pc = strictPc({ invite: { token: 'tok-live', used: false } });
  await tabs(page, 0, pc, { seed: true });
  await open(page);
  await connected(page);
  // The link pasted into this tab's address bar: no reload, just the fragment.
  await page.evaluate(() => (location.hash = '#pair=tok-live'));
  await expect.poll(() => pc.hellos.length, { timeout: 8000 }).toBe(2);
  expect(pc.hellos[1].device).toEqual(DEVICE);
  expect(pc.hellos[1].pair).toEqual({ token: 'tok-live' });
  await connected(page);
  // Paired already: the code spent (the PC tells Settings), nothing refused.
  expect(pc.alreadyPaired).toBe(1);
  expect(pc.invite.used).toBe(true);
  expect(pc.denied).toEqual([]);
  expect(await page.evaluate(() => location.hash)).toBe('');
});

test('"Too many tries": the PC\'s words, with when to try again, and no hammering', async ({ page }) => {
  await servePage(page);
  await seedDevice(page);
  const reason = 'Too many tries from this phone. Try again in 4 minutes.';
  let hellos = 0;
  await page.routeWebSocket(WS_URL, (ws) => {
    ws.onMessage((m) => {
      if (typeof m === 'string' && JSON.parse(m).type === 'hello') {
        hellos++;
        ws.send(JSON.stringify({ type: 'denied', reason }));
        ws.close();
      }
    });
  });
  await open(page, '#pair=tok-old');
  await expect(page.getByTestId('pair-why')).toHaveText(reason);
  await page.waitForTimeout(2000);
  expect(hellos).toBe(1);
  // Try again: once, on purpose, with the device (the old code is gone).
  await page.getByRole('button', { name: 'Try again' }).click();
  await expect.poll(() => hellos).toBe(2);
  expect(await page.evaluate(() => location.hash)).toBe('');
});
