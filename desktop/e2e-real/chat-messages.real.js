// Chat apps, for real (pasting/stacked.rs, mentions.rs, compose.rs,
// send_key.rs): a stand-in for Slack — an Edge page titled "… - Slack" with
// a message box whose Enter sends (the message goes into the list above it,
// the box empties) and whose "@" opens a list of the people in the channel,
// picked with Tab, its highlighted entry told to a screen reader
// (aria-activedescendant), as Slack's and Discord's are. Nothing is sent
// anywhere. Yap's steps go into it: stacked messages pasted and sent one by
// one (and stopped when one doesn't send), "at Rochelle" typed and picked,
// plain "@Rochelle" when the list doesn't say what it highlights; and the
// Send shortcut pressing Enter in the test pad.
import path from 'node:path';
import { test, expect, lf } from './support/robot.js';
import { edgeExe, startPage } from './support/targets.js';
import { pickerPage } from './support/pickers.js';

test.skip(!edgeExe(), 'Microsoft Edge is not installed');
test.use({ robotOptions: { name: 'chat-messages' } });

const PEOPLE = ['Rochelle Okafor', 'Priyanka Raman', 'Samantha Jones'];


/** The Slack stand-in (its title "general - Example Ltd - Slack - <nonce>": a chat app, Slack by its tab). */
async function slack(robot, { enterSends = true, describe = true } = {}) {
  const items = PEOPLE.map((p) => ({ label: p, insert: `@${p}` }));
  const aside = `<aside><p>Members</p><ul>${PEOPLE.map((p) => `<li>${p}</li>`).join('')}</ul></aside>`;
  const page = await robot.track(
    await startPage({
      nonce: robot.nonce,
      dir: path.join(robot.dir, `slack-${robot.targets.length}`),
      title: `general - Example Ltd - Slack - ${robot.nonce}`,
      body: pickerPage({ items, boxAttrs: 'aria-label="Message #general"', enterSends, describe, aside }),
    }),
  );
  return {
    ...page,
    sent: () => page.evaluate(() => [...document.querySelectorAll('#sent li')].map((li) => li.textContent)),
    pills: () => page.evaluate(() => [...document.querySelectorAll('#box .pill')].map((p) => p.textContent)),
    box: () => page.evaluate(() => document.getElementById('box').innerText.replace(/ /g, ' ').trim()),
  };
}

test('stacked messages: each pasted and sent in turn, the box empty after each', async ({ robot }) => {
  const chat = await slack(robot);
  await robot.configure({ stackedMessages: true, autoSubmit: true });
  try {
    const r = await robot.dictate(chat, 'Hi Rochelle. New message. Are you free at three?');
    expect(r.error).toBeNull();
    expect(r.kind).toBe('chat app');
    expect(r.steps).toEqual([{ paste: 'Hi Rochelle' }, { key: 'enter' }, { paste: 'Are you free at three?' }, { key: 'enter' }]);
    await expect.poll(() => chat.sent()).toEqual(['Hi Rochelle', 'Are you free at three?']);
    expect(await chat.box()).toBe('');
  } finally {
    await robot.configure({ stackedMessages: false, autoSubmit: false });
  }
});

test("a message that doesn't send stops it: nothing more is sent, the rest is on the clipboard", async ({ robot }) => {
  // Enter adds a line here (as in an app set to send with Ctrl+Enter).
  const chat = await slack(robot, { enterSends: false });
  await robot.configure({ stackedMessages: true, autoSubmit: true });
  try {
    const r = await robot.dictate(chat, 'Hi Rochelle. New message. Are you free at three?');
    expect(r.error).toMatch(/stopped after 0 of 2 messages \(notSent\)/);
    expect(r.leftOnClipboard).toBe(true);
    expect(await chat.sent()).toEqual([]);
    expect(await chat.box()).not.toContain('Are you free');
    expect((await robot.clipboard('check', 'Are you free at three?')).matchesText).toBe(true);
  } finally {
    await robot.configure({ stackedMessages: false, autoSubmit: false });
  }
});

test("the first message not taken: nothing sent, the whole dictation on the clipboard", async ({ robot }) => {
  const chat = await slack(robot);
  await robot.configure({ stackedMessages: true, autoSubmit: true });
  // No app reads the paste (as an app that ignores Ctrl+V).
  await robot.yap.invoke('e2e_paste_fails', { why: 'notRead' });
  try {
    const r = await robot.dictate(chat, 'Hi Rochelle. New message. Are you free at three?');
    expect(r.leftOnClipboard).toBe(true);
    expect(r.leftWhy).toBe('notRead');
    expect(await chat.sent()).toEqual([]);
    // Both messages, not the first alone ("…it's copied, press Ctrl+V").
    expect((await robot.clipboard('check', 'Hi Rochelle\nAre you free at three?')).matchesText).toBe(true);
  } finally {
    await robot.yap.invoke('e2e_paste_fails', { why: null });
    await robot.configure({ stackedMessages: false, autoSubmit: false });
  }
});

test('"at Rochelle": typed, picked from the list with Tab, the text after it in place', async ({ robot }) => {
  const chat = await slack(robot);
  const r = await robot.dictate(chat, 'At Rochelle, can you check the build?');
  expect(r.error).toBeNull();
  // Picked; the space the app put after the mention (as Slack and Discord
  // do) taken back before the comma; then the rest.
  expect(r.steps).toEqual([
    { typed: '@Rochelle', picked: true },
    { key: 'backspace' },
    { paste: ', can you check the build?' },
  ]);
  await expect.poll(() => chat.pills()).toEqual(['@Rochelle Okafor']);
  await expect.poll(() => chat.box()).toBe('@Rochelle Okafor, can you check the build?');
  expect(await chat.sent()).toEqual([]);

  // A word after it: the app's space is the one between them (no second).
  const other = await slack(robot);
  const r2 = await robot.dictate(other, 'At Priyanka can you check the build?');
  expect(r2.steps).toEqual([{ typed: '@Priyanka', picked: true }, { paste: 'can you check the build?' }]);
  await expect.poll(() => other.box()).toBe('@Priyanka Raman can you check the build?');
});

test("a list that doesn't say what it highlights gets no key: plain @Rochelle", async ({ robot }) => {
  const chat = await slack(robot, { describe: false });
  const r = await robot.dictate(chat, 'At Rochelle, can you check the build?');
  expect(r.steps[0]).toEqual({ typed: '@Rochelle', picked: false });
  await expect.poll(() => chat.box()).toBe('@Rochelle, can you check the build?');
  expect(await chat.pills()).toEqual([]);
  expect(await chat.sent()).toEqual([]);
});

test('the Send shortcut presses Enter where you are typing', async ({ robot }) => {
  const { pad } = robot;
  await robot.configure({ sendHotkey: 'kb:130' });
  try {
    await pad.set('box', 'Ready');
    await robot.hookKeys('d:130,w:60,u:130');
    await expect.poll(async () => lf(await pad.text('box'))).toBe('Ready\n');
  } finally {
    await robot.configure({ sendHotkey: '' });
    await pad.set('box', '');
  }
});
