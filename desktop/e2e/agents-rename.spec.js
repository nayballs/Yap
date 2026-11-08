// Jobs became Yap Code, each run an agent (7 Nov 2026; desktop/src-tauri/src/agents.rs
// `moved`, config.rs `migrate_once` v8, agents.rs `renamed`): a data folder an older
// Yap left — jobs.json, job_folders.json, jobs/<id>/ and config.json's `jobs…`
// settings — read by today's Yap with nothing lost.
// - The files under their new names (agents.json, workspaces.json, agents/), the old
//   ones gone; the agent, its log and its workspace there.
// - The settings under their new names (`agentsEnabled`…), every value kept, and saved
//   so (no `jobs…` left in config.json).
// - A chat kept then: its report marker (`kind: "jobReport"`, "The job in …") shows as
//   an agent's report card, and its ask-first card (`act: "job"`) as an agent's.
// - Settings → Yap Code: the switch on, the workspace and the agent in History; the old
//   link ("jobs") still lands there.
// Fictional data only, in this run's own folder.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openView, settingsDialog } from './support/fixtures.js';
import { RUNS } from './support/yap.js';
import { toSide } from './support/code-chats.js';

const HERE = path.join(RUNS, 'agents-rename-spec');
const GARDEN = path.join(HERE, 'folders', 'garden-planner');
const NOW = Math.floor(Date.now() / 1000);

const JOBS = {
  jobs: [
    {
      id: 'ab12cd34',
      conversation: 7,
      messageId: 'chat:7:0',
      origin: 'typed',
      folder: 'garden-planner',
      path: GARDEN,
      brief: 'Tidy notes.md.',
      mode: 'auto',
      pay: 'plan',
      status: 'done',
      createdTs: NOW - 600,
      startedTs: NOW - 590,
      endedTs: NOW - 560,
      updatedTs: NOW - 560,
      files: ['notes.md'],
      text: 'Done. I tidied notes.md.',
      turns: 1,
      reported: true,
      reportMessageId: 'chat:7:2',
    },
  ],
  modes: { 7: 'plan' },
  codeChats: { 7: { path: GARDEN, ts: NOW - 600 } },
  bypassConfirmed: false,
};

const FOLDERS = { folders: [{ name: 'garden-planner', path: GARDEN, addedTs: NOW - 900 }] };

const CHATS = {
  conversations: [
    {
      id: 7,
      title: 'Tidy the notes',
      createdTs: NOW - 600,
      updatedTs: NOW - 550,
      messages: [
        { role: 'user', text: 'Tidy notes.md in garden-planner.', ts: NOW - 600 },
        {
          role: 'assistant',
          text: "On it: I've started a job in garden-planner.",
          ts: NOW - 595,
          tools: ['Started a job in garden-planner'],
          confirms: [
            {
              id: 1,
              label: 'garden-planner',
              site: GARDEN,
              does: 'start a job',
              state: 'allowed',
              act: 'job',
              text: 'Tidy notes.md.',
              job: { folder: 'garden-planner', path: GARDEN, mode: 'auto', modeLabel: 'Auto', brief: 'Tidy notes.md.', newFolder: true, web: false, continues: false },
            },
          ],
        },
        { role: 'user', text: "The job in garden-planner: it's done.\nFiles it changed: notes.md (backed up first).", kind: 'jobReport', ts: NOW - 560 },
        { role: 'assistant', text: 'Done. I tidied notes.md, and kept a backup.', ts: NOW - 555 },
      ],
    },
  ],
};

const test = base.extend({
  yapOptions: [
    async ({}, use) => {
      fs.mkdirSync(GARDEN, { recursive: true });
      fs.writeFileSync(path.join(GARDEN, 'notes.md'), '# Garden\n\nTidied.\n');
      await use({
        name: 'agents-rename',
        // As Jobs saved them (v7), none a default. (Not `jobsCleanSpeech`: every
        // test instance's config already says `agentsCleanSpeech`.)
        config: { configVersion: 7, jobsEnabled: true, jobsPay: 'key', jobsModel: 'claude-opus-5-5', jobsEffort: 'max', jobsMode: 'plan', jobsAsk: 'always', jobsSpeak: 'never', jobsMax: 1, jobsAnimate: false, memoryLearnChats: false },
        data: { 'jobs.json': JOBS, 'job_folders.json': FOLDERS, 'chats.json': CHATS },
        prepare: (dir) => {
          const own = path.join(dir, 'jobs', 'ab12cd34');
          fs.mkdirSync(path.join(own, 'backup'), { recursive: true });
          fs.writeFileSync(path.join(own, 'log.jsonl'), `${JSON.stringify({ id: 'ab12cd34', status: 'running', folder: 'garden-planner', step: 'Reading notes.md', ts: NOW - 585 })}\n`);
        },
      });
    },
    { scope: 'worker' },
  ],
});

test('a data folder from Jobs: its files, settings and chats are Yap Code’s, nothing lost', async ({ yap, main, shot }) => {
  // The files under their new names; the old ones gone.
  const has = (f) => fs.existsSync(path.join(yap.dataDir, f));
  expect([has('agents.json'), has('workspaces.json'), has('agents')]).toEqual([true, true, true]);
  expect([has('jobs.json'), has('job_folders.json'), has('jobs')]).toEqual([false, false, false]);
  expect(has(path.join('agents', 'ab12cd34', 'log.jsonl'))).toBe(true);

  // The agent, its log and its workspace.
  const agents = await yap.invoke('agent_list');
  expect(agents).toHaveLength(1);
  expect(agents[0]).toMatchObject({ id: 'ab12cd34', folder: 'garden-planner', status: 'done', text: 'Done. I tidied notes.md.' });
  const got = await yap.invoke('agent_get', { id: 'ab12cd34' });
  expect(got.log.map((e) => e.step)).toContain('Reading notes.md');
  expect((await yap.invoke('workspaces_list')).map((w) => w.name)).toEqual(['garden-planner']);
  expect(await yap.invoke('code_chat_workspace_get', { conversation: 7 })).toMatchObject({ name: 'garden-planner' });
  expect((await yap.invoke('agent_mode_get', { conversation: 7 })).mode).toBe('plan');

  // The settings under their new names, every value kept, and saved so.
  const cfg = await yap.invoke('get_config');
  expect(cfg).toMatchObject({ agentsEnabled: true, agentsPay: 'key', agentsModel: 'claude-opus-5-5', agentsEffort: 'max', agentsMode: 'plan', agentsAsk: 'always', agentsSpeak: 'never', agentsMax: 1, agentsAnimate: false });
  const saved = yap.readJson('config.json');
  expect(saved.configVersion).toBeGreaterThanOrEqual(8);
  expect(Object.keys(saved).filter((k) => k.startsWith('jobs'))).toEqual([]);
  expect(saved.agentsEnabled).toBe(true);

  // The chat kept then: its marker an agent's report, its card an agent's.
  const conv = await yap.invoke('chat_get', { id: 7 });
  expect(conv.messages.map((m) => m.kind ?? '')).toEqual(['', '', 'agentReport', '']);
  expect(conv.messages[1].confirms[0]).toMatchObject({ act: 'agent', agent: { folder: 'garden-planner', brief: 'Tidy notes.md.' } });
  await openView(main, 'Chat');
  await toSide(main, 'Code');
  await main.getByText('Tidy the notes').first().click();
  const report = main.getByTestId('chat-agent-report');
  await expect(report).toBeVisible();
  await expect(report.locator('.jtitle')).toHaveText('Agent done · garden-planner');
  const card = main.getByTestId('chat-agent-confirm');
  await expect(card).toBeVisible();
  await expect(card.locator('.jq')).toHaveText('Yap wants to send an agent to ‘garden-planner’ (Auto)');
  await shot(main, '01-a-chat-from-jobs');

  // Settings → Yap Code, by the old link too: the switch on, the workspace, the agent.
  await yap.invoke('plugin:event|emit', { event: 'yap-open-settings', payload: 'jobs' });
  const dialog = settingsDialog(main);
  await expect(dialog.getByRole('heading', { name: 'Yap Code', level: 1 })).toBeVisible();
  await expect(dialog.getByTestId('yap-code-workspace')).toHaveAttribute('data-name', 'garden-planner');
  await expect(dialog.getByTestId('yap-code-history-row')).toHaveCount(1);
  await shot(main, '02-settings-yap-code');
});
