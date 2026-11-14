// One agent per Code chat, its follow-ups as its turns (2026-11-07):
// desktop/src-tauri/src/agents/runner.rs (`follow_up`, `undo_turn`, per-turn
// backups), chat/tools/agents.rs (`decide`: start_agent in a Code chat carries
// the chat's agent on), chat/turn/report.rs (a report when it stops working), and the tab
// (views/chat/AgentTab.svelte + AgentTurn.svelte; core/agents.js `turnsOf`,
// `pillsByAnswer`, `agentTabName`). As Claude Code itself is one session per
// conversation, each follow-up in a Code chat ("could you add a minimap
// button for it as well?") continues the chat's agent with its context.
//
// Agents are driven as agents-ui.spec.js drives them: the scripted fake AI calls
// the agent tools, and Claude Code is this build's own stand-in (`yap.exe
// fake-claude <script>`, switched per turn with `e2e_claude_script`), whose
// `_dump_args` here appends each turn's arguments to a file of this run's
// own (`--session-id`, then `--resume`). No one's real Claude Code, account
// or files: the workspace is in this run's own folder.
//
// - Two requests in a Code chat: one agent, one tab with two turns (its
//   panel's two rows; its timeline the newest open, the earlier folded), two
//   reports, the second turn on `--resume` of the same session; a pill under
//   each answer and a report card each, all opening the same tab at their
//   turn's row. Light and dark.
// - A note while the agent works goes to it now (11 Nov 2026: Claude Code
//   takes a message written mid-work at its next tool call's end): said so,
//   shown in its tab, read at its next step, the same turn, the same process,
//   one report. One it never reads (its turn ended first) waits as its next
//   turn, on `--resume`, and the two are told in one report, never one each
//   (the chat spoke twice in a row with nobody asking in between).
// - Its work (11 Nov 2026, after Claude Code's own transcript): what the
//   agent says as it goes, and a line for each stretch of steps ("Edited
//   notes.md +1") that opens into them (views/chat/AgentWork.svelte,
//   core/agents.js `workOf`, `workSummary`). **Where it shows** (12 Nov 2026,
//   the maintainer: "if you send out an agent I don't think I see the work
//   the agent does in the main chat window, only if you yourself do it"):
//   an agent Chat sent out — every agent of this spec — shows it in its row
//   of the Agent panel, never in the thread; a Code chat that works directly
//   shows its own in the thread (agents-direct.spec.js).
// - A step that came back with a picture (a picture file read: the tool
//   result's own image, as Claude Code hands it over) ends its line with it,
//   small, as Claude Code's transcript does (views/chat/StepPictures.svelte;
//   kept by agents/pictures.rs in the agent's own folder): larger by its
//   step once the line is opened, opened in the viewer or the panel's
//   picture tab, on its step in the Agent tab too. Light and dark.
// - Another tool's call (an MCP server's, a connector's, a skill, a helper)
//   is a step of the work too (11 Nov 2026): a stretch of them alone is a
//   line ("Used homelights · list lights"), several are named when they're
//   all one's and counted otherwise ("Ran a command, used 2 tools"). Yap's
//   own housekeeping still isn't.
// - A stretch opened is a row a step, as done (11 Nov 2026, after Claude
//   Code's transcript: views/chat/StepRows.svelte): "Read notes.md (1–20)",
//   "Edited notes.md +1", "Ran the tests · failed", "Created legend.md +3".
//   An edit's row opens to its path and its diff, each line with its number
//   in the file (Claude Code's own patch says where); a command's to the
//   command, with Copy, and what it printed; a row with nothing more
//   doesn't open. Light and dark.
// - The rows' code is in colour (core/codeColours.js: highlight.js, loaded
//   when a row is first opened; Yap's own colours, light and dark): a
//   diff's lines by the file's language, a command as a shell's, what it
//   printed plain; each token a text node, never markup.
// - Where a line was changed rather than taken out, the part that was
//   replaced is marked in a stronger red and what replaced it in a stronger
//   green (core/diffWords.js), as Claude Code's transcript marks them.
// - A part handed to a helper (Claude Code's own subagent): the helper's
//   steps fold behind its row ("Handed part of it to a helper · Check the
//   docs") as rows of their own, never among the session's own steps, and
//   the stretch's line counts the session's alone (12 Nov 2026).
// - "Start fresh" (`fresh: true`): a second agent, its tab named apart.
// - Undo this turn (refused while a later turn changed the same file:
//   "Undo the later change first"), and Undo all, asked first.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openView, setConfig } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { RUNS } from './support/yap.js';
import { newCodeChat, agentTimeline } from './support/code-chats.js';
import { readPng, writePng } from './support/png.js';
import {
  turnsOf,
  titleOf,
  turnStatusWord,
  canUndoTurn,
  pillsByAnswer,
  agentOfReport,
  turnOfReport,
  agentTabName,
  workedFor,
  turnNow,
  workOf,
  workSummary,
  stepKind,
  stepRow,
  rowSteps,
  stepOpens,
  diffRows,
  timelineOf,
  stepPicture,
  groupPictures,
  shownStepPicture,
  helpersOf,
} from '../src/lib/core/agents.js';
import { LANGUAGES, languageOf, fenceLanguage, shellOf, kindOf, tokensOf, lineTokens, colourLines, colourDiff } from '../src/lib/core/codeColours.js';
import hljs from '../src/lib/core/codeColours.hljs.js';
import { wordChanges, pairChanges, linePieces } from '../src/lib/core/diffWords.js';

const HERE = path.join(RUNS, 'agents-turns-spec');
const SCRIPTS = path.join(HERE, 'scripts');
const GARDEN = path.join(HERE, 'folders', 'garden-planner');
const ARGS = path.join(HERE, 'args.jsonl');

const STARTED = "On it: I've sent an agent to garden-planner.";
const PASSED = "On it: I've passed that to the agent in garden-planner.";
const NOTED = 'Passed that on: the agent in garden-planner takes it in at its next step.';
const DONE1 = 'Done. notes.md has a watering schedule now.';
const DONE2 = 'Done. There is a minimap button for the beds, and notes.md says so.';
const NOTES0 = 'Draft notes\n';
const NOTES1 = 'Draft notes\nWatering: twice a week\n';
const NOTES2 = 'Draft notes\nWatering: twice a week\nMinimap: on\n';
const MINIMAP = 'export const minimap = true;\n';

// ---- the stand-in's scripts ----

const init = (mode = 'auto') => ({
  type: 'system',
  subtype: 'init',
  cwd: '{{cwd}}',
  session_id: '{{session_id}}',
  tools: ['Bash', 'Edit', 'Read', 'Write'],
  mcp_servers: [{ name: 'yap', status: 'connected' }],
  model: 'claude-sonnet-5-5',
  permissionMode: mode,
  apiKeySource: 'none',
  claude_code_version: '2.1.296',
  capabilities: ['interrupt_receipt_v1'],
});
const said = (id, content, stop = 'tool_use') => ({
  type: 'assistant',
  message: { id, type: 'message', role: 'assistant', model: 'claude-sonnet-5-5', content, stop_reason: stop },
  parent_tool_use_id: null,
  session_id: '{{session_id}}',
});
const toolResult = (id, text) => ({
  type: 'user',
  message: { role: 'user', content: [{ type: 'tool_result', tool_use_id: id, content: text }] },
  parent_tool_use_id: null,
  session_id: '{{session_id}}',
});
const result = (text, cost) => ({
  type: 'result',
  subtype: 'success',
  is_error: false,
  duration_ms: 900,
  num_turns: 2,
  result: text,
  session_id: '{{session_id}}',
  total_cost_usd: cost,
  usage: { input_tokens: 4000, output_tokens: 120, cache_creation_input_tokens: 0, cache_read_input_tokens: 0 },
  permission_denials: [],
  terminal_reason: 'completed',
});
/** An edit of `file` (backed up first by the run's hook), leaving `text`. */
const edit = (id, file, text, old_string, new_string) => [
  said(`${id}a`, [{ type: 'tool_use', id, name: 'Edit', input: { file_path: `{{cwd}}\\${file}`, old_string, new_string } }]),
  { _hook: { event: 'PreToolUse', input: { tool_name: 'Edit', tool_use_id: id, tool_input: { file_path: file, old_string, new_string } } } },
  { _write_file: { path: file, text } },
  toolResult(id, `The file ${file} has been updated.`),
];
/** Its first turn: a watering schedule in notes.md. */
const WATERING = [
  { _wait_user: true },
  { _dump_args: ARGS },
  init(),
  said('w1', [{ type: 'text', text: "I'll add the watering schedule to notes.md." }], 'tool_use'),
  ...edit('toolu_w', 'notes.md', NOTES1, 'Draft notes', 'Draft notes\nWatering: twice a week'),
  said('w2', [{ type: 'text', text: DONE1 }], 'end_turn'),
  result(DONE1, 0.03),
  { _wait_user: true },
];
/** Its next: notes.md again, and a new minimap.js. */
const MINIMAP_TURN = [
  { _wait_user: true },
  { _dump_args: ARGS },
  init(),
  said('m1', [{ type: 'text', text: 'Now the minimap button.' }], 'tool_use'),
  ...edit('toolu_m', 'notes.md', NOTES2, 'Watering: twice a week', 'Watering: twice a week\nMinimap: on'),
  said('m2', [{ type: 'tool_use', id: 'toolu_map', name: 'Write', input: { file_path: '{{cwd}}\\minimap.js', content: MINIMAP } }]),
  { _hook: { event: 'PreToolUse', input: { tool_name: 'Write', tool_use_id: 'toolu_map', tool_input: { file_path: 'minimap.js' } } } },
  { _write_file: { path: 'minimap.js', text: MINIMAP } },
  toolResult('toolu_map', 'File created successfully at: minimap.js'),
  said('m3', [{ type: 'text', text: DONE2 }], 'end_turn'),
  result(DONE2, 0.02),
  { _wait_user: true },
];
/**
 * A turn that takes its time (a note comes while it works): the note is with
 * it at its tool call's end, as Claude Code takes one (`{{notes}}`).
 */
const SLOWISH = [
  { _wait_user: true },
  { _dump_args: ARGS },
  init(),
  said('s1', [{ type: 'tool_use', id: 'toolu_read', name: 'Read', input: { file_path: '{{cwd}}\\notes.md' } }]),
  { _sleep_ms: 6000 },
  toolResult('toolu_read', 'Draft notes'),
  result('Read notes.md. Noted: {{notes}}', 0.01),
  { _wait_user: true },
];
/**
 * A turn with no tool call left when the note comes, whose Claude Code ends
 * with its result: the note is never read (it goes back to Yap).
 */
const NEVER_READS = [{ _wait_user: true }, { _dump_args: ARGS }, init(), said('n1', [{ type: 'text', text: 'Reading the notes.' }], 'end_turn'), { _sleep_ms: 6000 }, result('Read notes.md.', 0.01)];
/** A made-up bed plan, as a picture file's bytes: 320 × 200, one green. */
const PLAN = { w: 320, h: 200, colour: [86, 142, 96] };
const PLAN_PNG = writePng(PLAN.w, PLAN.h, PLAN.colour);
const LOOKED = 'Looked at the bed plan: four beds, the herbs nearest the door.';
/**
 * A turn that reads a picture file and lists the folder: the picture comes
 * back in the tool's own result, as Claude Code 2.1.296 hands one to the
 * model (an image block, base64).
 */
const LOOKS_AT_THE_PLAN = [
  { _wait_user: true },
  { _dump_args: ARGS },
  init(),
  said('p1', [{ type: 'text', text: "I'll look at the bed plan." }], 'tool_use'),
  said('p2', [{ type: 'tool_use', id: 'toolu_ls', name: 'Bash', input: { command: 'ls', description: 'List the folder' } }]),
  toolResult('toolu_ls', 'beds.png\nnotes.md'),
  said('p3', [{ type: 'tool_use', id: 'toolu_plan', name: 'Read', input: { file_path: '{{cwd}}\\beds.png' } }]),
  {
    type: 'user',
    message: { role: 'user', content: [{ type: 'tool_result', tool_use_id: 'toolu_plan', content: [{ type: 'image', source: { type: 'base64', media_type: 'image/png', data: PLAN_PNG.toString('base64') } }] }] },
    parent_tool_use_id: null,
    session_id: '{{session_id}}',
  },
  said('p4', [{ type: 'text', text: LOOKED }], 'end_turn'),
  result(LOOKED, 0.01),
  { _wait_user: true },
];
const LIT = 'Both lights are on now, and high tide is at 14:05.';
/**
 * A turn that works with other tools (made-up MCP servers: the lights, the
 * tides): one call alone, then its words, then a stretch of two servers'
 * calls and a command.
 */
const USES_OTHER_TOOLS = [
  { _wait_user: true },
  { _dump_args: ARGS },
  init(),
  said('t1', [{ type: 'tool_use', id: 'toolu_lights', name: 'mcp__homelights__list_lights', input: {} }]),
  toolResult('toolu_lights', 'Porch: on. Shed: off.'),
  said('t2', [{ type: 'text', text: 'The porch light is on. Turning the shed light on too.' }], 'tool_use'),
  said('t3', [{ type: 'tool_use', id: 'toolu_set', name: 'mcp__homelights__set_light', input: { light: 'Shed', on: true } }]),
  toolResult('toolu_set', 'Shed: on.'),
  said('t4', [{ type: 'tool_use', id: 'toolu_tide', name: 'mcp__tides__next_high_tide', input: {} }]),
  toolResult('toolu_tide', '14:05'),
  said('t5', [{ type: 'tool_use', id: 'toolu_ls', name: 'Bash', input: { command: 'ls', description: 'List the folder' } }]),
  toolResult('toolu_ls', 'notes.md'),
  said('t6', [{ type: 'text', text: LIT }], 'end_turn'),
  result(LIT, 0.01),
  { _wait_user: true },
];
const LEGEND = '# Legend\n\nB: a bed\n';
const WATER_JS = '// How much a bed drinks.\nexport function water(bed) {\n  return bed.litres * 2;\n}\n';
const TESTED = 'Added the watering line and a legend. One test still fails: watering is twice a week.';
const TEST_OUTPUT = '> garden-planner@1.0.0 test\n> node --test\n\n✔ beds are sorted (1.2ms)\n✖ watering is twice a week (0.4ms)\n\ntests 2\npass 1\nfail 1';
/**
 * A turn of four steps for its rows: a part of a file read, an edit with
 * Claude Code's own record of it (its patch says where in the file, as
 * 2.1.296 gives one), a command Claude Code described that prints and
 * fails, and a new file.
 */
const FOUR_STEPS = [
  { _wait_user: true },
  { _dump_args: ARGS },
  init(),
  said('r1', [{ type: 'tool_use', id: 'toolu_r', name: 'Read', input: { file_path: '{{cwd}}\\notes.md', offset: 1, limit: 20 } }]),
  toolResult('toolu_r', '1\tDraft notes'),
  said('r2', [{ type: 'tool_use', id: 'toolu_e', name: 'Edit', input: { file_path: '{{cwd}}\\notes.md', old_string: 'Draft notes', new_string: 'Draft notes\nWatering: twice a week' } }]),
  { _hook: { event: 'PreToolUse', input: { tool_name: 'Edit', tool_use_id: 'toolu_e', tool_input: { file_path: 'notes.md' } } } },
  { _write_file: { path: 'notes.md', text: NOTES1 } },
  {
    type: 'user',
    message: { role: 'user', content: [{ type: 'tool_result', tool_use_id: 'toolu_e', content: 'The file notes.md has been updated.' }] },
    tool_use_result: { filePath: 'notes.md', structuredPatch: [{ oldStart: 1, oldLines: 1, newStart: 1, newLines: 2, lines: [' Draft notes', '+Watering: twice a week'] }], userModified: false, replaceAll: false },
    parent_tool_use_id: null,
    session_id: '{{session_id}}',
  },
  said('r3', [{ type: 'tool_use', id: 'toolu_t', name: 'Bash', input: { command: 'npm test', description: 'Run the tests' } }]),
  {
    type: 'user',
    message: { role: 'user', content: [{ type: 'tool_result', tool_use_id: 'toolu_t', content: TEST_OUTPUT, is_error: true }] },
    parent_tool_use_id: null,
    session_id: '{{session_id}}',
  },
  said('r3b', [{ type: 'tool_use', id: 'toolu_j', name: 'Write', input: { file_path: '{{cwd}}\\water.js', content: WATER_JS } }]),
  { _hook: { event: 'PreToolUse', input: { tool_name: 'Write', tool_use_id: 'toolu_j', tool_input: { file_path: 'water.js' } } } },
  { _write_file: { path: 'water.js', text: WATER_JS } },
  toolResult('toolu_j', 'File created successfully at: water.js'),
  said('r4', [{ type: 'tool_use', id: 'toolu_w', name: 'Write', input: { file_path: '{{cwd}}\\legend.md', content: LEGEND } }]),
  { _hook: { event: 'PreToolUse', input: { tool_name: 'Write', tool_use_id: 'toolu_w', tool_input: { file_path: 'legend.md' } } } },
  { _write_file: { path: 'legend.md', text: LEGEND } },
  toolResult('toolu_w', 'File created successfully at: legend.md'),
  said('r4b', [{ type: 'tool_use', id: 'toolu_k', name: 'Edit', input: { file_path: '{{cwd}}\\water.js', old_string: 'bed.litres * 2', new_string: 'bed.litres * 3' } }]),
  { _hook: { event: 'PreToolUse', input: { tool_name: 'Edit', tool_use_id: 'toolu_k', tool_input: { file_path: 'water.js' } } } },
  { _write_file: { path: 'water.js', text: WATER_JS.replace('* 2', '* 3') } },
  {
    type: 'user',
    message: { role: 'user', content: [{ type: 'tool_result', tool_use_id: 'toolu_k', content: 'The file water.js has been updated.' }] },
    tool_use_result: { filePath: 'water.js', structuredPatch: [{ oldStart: 2, oldLines: 3, newStart: 2, newLines: 3, lines: [' export function water(bed) {', '-  return bed.litres * 2;', '+  return bed.litres * 3;', ' }'] }], userModified: false, replaceAll: false },
    parent_tool_use_id: null,
    session_id: '{{session_id}}',
  },
  said('r5', [{ type: 'text', text: TESTED }], 'end_turn'),
  result(TESTED, 0.02),
  { _wait_user: true },
];
const HELPED = 'The docs are in order; the folder has two files.';
/** A helper's own message or result: under the call that sent it. */
const under = (call, line) => ({ ...line, parent_tool_use_id: call });
/**
 * A turn that hands part of its work to a helper (Claude Code's `Task`): the helper reads a file and
 * edits one, under that call; then the session runs a command of its own.
 */
const HANDS_PART_OVER = [
  { _wait_user: true },
  { _dump_args: ARGS },
  init(),
  said('h1', [{ type: 'tool_use', id: 'toolu_task', name: 'Task', input: { description: 'Check the notes', prompt: 'Read notes.md and add a line.' } }]),
  under('toolu_task', said('h2', [{ type: 'tool_use', id: 'toolu_hr', name: 'Read', input: { file_path: '{{cwd}}\\notes.md' } }])),
  // (It takes a moment over its first step: its entry in the Agent panel is seen at work.)
  { _sleep_ms: 5000 },
  under('toolu_task', toolResult('toolu_hr', '1\tDraft notes')),
  under('toolu_task', said('h3', [{ type: 'tool_use', id: 'toolu_he', name: 'Edit', input: { file_path: '{{cwd}}\\notes.md', old_string: 'Draft notes', new_string: 'Draft notes\nChecked' } }])),
  { _hook: { event: 'PreToolUse', input: { tool_name: 'Edit', tool_use_id: 'toolu_he', tool_input: { file_path: 'notes.md' } } } },
  { _write_file: { path: 'notes.md', text: 'Draft notes\nChecked\n' } },
  under('toolu_task', toolResult('toolu_he', 'The file notes.md has been updated.')),
  toolResult('toolu_task', 'The notes are in order.'),
  said('h4', [{ type: 'tool_use', id: 'toolu_ls', name: 'Bash', input: { command: 'ls', description: 'List the folder' } }]),
  toolResult('toolu_ls', 'notes.md'),
  said('h5', [{ type: 'text', text: HELPED }], 'end_turn'),
  result(HELPED, 0.02),
  { _wait_user: true },
];
/** Says what it was told. */
const ECHO = [{ _wait_user: true }, { _dump_args: ARGS }, init(), result('Got: {{user_text}}', 0.002), { _wait_user: true }];

function writeScript(name, lines) {
  fs.mkdirSync(SCRIPTS, { recursive: true });
  const file = path.join(SCRIPTS, `${name}.jsonl`);
  fs.writeFileSync(file, lines.map((l) => JSON.stringify(l)).join('\n') + '\n');
  return file;
}

function freshFolder(dir) {
  fs.rmSync(dir, { recursive: true, force: true });
  fs.mkdirSync(dir, { recursive: true });
  fs.writeFileSync(path.join(dir, 'notes.md'), NOTES0);
}

/** Each turn's Claude Code arguments, as the stand-in wrote them. */
const argsSeen = () => (fs.existsSync(ARGS) ? fs.readFileSync(ARGS, 'utf8').split('\n').filter(Boolean).map((l) => JSON.parse(l).args) : []);
const after = (args, flag) => {
  const i = args.indexOf(flag);
  return i >= 0 ? args[i + 1] : undefined;
};
const read = (f) => fs.readFileSync(path.join(GARDEN, f), 'utf8');

// ---- the app ----

const test = base.extend({
  fakeLlm: [
    async ({}, use) => {
      const fake = await startFakeLlm();
      await use(fake);
      await fake.close();
    },
    { scope: 'worker' },
  ],
  yapOptions: [
    async ({}, use) => {
      await use({ name: 'agents-turns', config: { memoryLearnChats: false } });
    },
    { scope: 'worker' },
  ],
});

const chatScope = (fakeLlm) => ({
  chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-chat', apiKey: '', apiKeys: {}, prompt: '', disableThinking: false },
});
const thread = (main) => main.locator('.msgs');
/** The rows of an opened stretch (StepRows.svelte), in `work` (the thread's AgentWork). */
const rowsIn = (work) => work.getByTestId('chat-agent-work-row');
/**
 * **A sent-out agent's work**: in its row of the Agent panel, never the thread. Turn `n`'s row opened (its
 * tab opened by its pill when it's closed, back from the timeline when that's showing), and its work in it.
 */
async function panelWork(main, n = 1) {
  if (!(await tab(main).count())) await thread(main).getByTestId('chat-agent-pill').first().click();
  await expect(tab(main)).toBeVisible();
  const back = tab(main).getByTestId('chat-agent-back');
  if (await back.count()) await back.click();
  const row = rowAt(main, n);
  await expect(row).toBeVisible();
  if ((await row.getAttribute('data-open')) !== 'true') await row.locator('.rhead').click();
  return row.getByTestId('chat-agent-row-steps').getByTestId('chat-agent-work');
}

/** Wait until what `inside` scrolls in has stopped scrolling (a theme change or new lines leave a box following its end for a moment). */
async function scrollStill(inside) {
  let last = null;
  await expect
    .poll(
      async () => {
        const now = await inside.evaluate((el) => {
          let n = el;
          while (n && n.scrollHeight <= n.clientHeight + 1) n = n.parentElement;
          return n ? Math.round(n.scrollTop) : 0;
        });
        const same = now === last;
        last = now;
        return same;
      },
      { intervals: [250], timeout: 10_000, message: 'the box to stop scrolling' }
    )
    .toBe(true);
}
const lastAnswer = (main) => thread(main).locator('.cbubble.assistant').last();
const chipsOfLast = (main) => thread(main).locator('.toolchips').last().locator('.toolchip');
const agents = (yap) => yap.invoke('agent_list');
const script = (yap, lines, name) => yap.invoke('e2e_claude_script', { script: writeScript(name, lines) });
const tab = (main) => main.getByTestId('chat-agent');
const turnsIn = (main) => tab(main).getByTestId('chat-agent-turn');
const turnAt = (main, n) => tab(main).locator(`[data-testid="chat-agent-turn"][data-turn="${n}"]`);
/** Turn `n`'s row in the tab's panel (AgentPanel.svelte). */
const rowAt = (main, n) => tab(main).locator(`[data-testid="chat-agent-row"][data-turn="${n}"]`);
/** The latest tool result the chat model was given. */
const lastTool = (fakeLlm) => JSON.parse(fakeLlm.requests.filter((r) => r.messages.some((m) => m.role === 'tool')).at(-1).messages.filter((m) => m.role === 'tool').at(-1).content);

async function send(main, text) {
  await main.getByRole('textbox', { name: 'Message' }).fill(text);
  await main.getByRole('button', { name: 'Send', exact: true }).click();
}

async function closeTab(main) {
  if (await tab(main).count()) await tab(main).getByRole('button', { name: 'Close the panel' }).click();
}

/** A new code chat in garden-planner. */
async function codeChat(main) {
  await openView(main, 'Chat');
  await closeTab(main);
  await newCodeChat(main, 'garden-planner');
}

async function ended(yap, id, timeout = 30_000) {
  await expect
    .poll(async () => (await agents(yap)).find((j) => j.id === id)?.status, { timeout, message: `agent ${id} to end` })
    .toMatch(/^(done|failed|stopped)$/);
  return (await agents(yap)).find((j) => j.id === id);
}

async function newAgent(yap, before) {
  await expect.poll(async () => (await agents(yap)).length, { timeout: 20_000, message: 'an agent to start' }).toBe(before + 1);
  return (await agents(yap))[0];
}

async function messagesOf(yap, id, n) {
  await expect.poll(async () => (await yap.invoke('chat_get', { id })).messages.length, { timeout: 30_000, message: `${n} messages` }).toBe(n);
  return (await yap.invoke('chat_get', { id })).messages;
}

async function theme(yap, main, which) {
  await setConfig(yap, main, { theme: which });
  await expect.poll(() => main.evaluate(() => document.documentElement.dataset.yapTheme)).toBe(which);
}

/**
 * **A Code chat's agent, two requests in**: a watering schedule, then "could
 * you add a minimap button for it as well?" — both reported. The agent, its
 * conversation, and how many agents there were before.
 */
async function twoTurns(yap, main, fakeLlm) {
  fs.rmSync(ARGS, { force: true });
  await script(yap, WATERING, 'watering');
  const before = (await agents(yap)).length;
  await codeChat(main);
  fakeLlm.scriptChat([
    { tools: [{ name: 'start_agent', args: { brief: 'Add a watering schedule to notes.md.', title: 'Add a watering schedule' } }] },
    { content: STARTED },
    { plain: true, content: 'Done: notes.md has a watering schedule.' },
  ]);
  await send(main, 'Add a watering schedule to notes.md.');
  const first = await ended(yap, (await newAgent(yap, before)).id);
  const conversation = first.conversation;
  await messagesOf(yap, conversation, 4);
  // The follow-up: the same agent, its next turn.
  await script(yap, MINIMAP_TURN, 'minimap');
  fakeLlm.scriptChat([
    { tools: [{ name: 'start_agent', args: { brief: 'Add a minimap button for the beds, and say so in notes.md.', title: 'Add a minimap button' } }] },
    { content: PASSED },
    { plain: true, content: 'Done: there is a minimap button for the beds now.' },
  ]);
  await send(main, 'could you add a minimap button for it as well?');
  await expect(chipsOfLast(main)).toHaveText(['Continued the agent in garden-planner']);
  await expect.poll(async () => (await agents(yap)).find((j) => j.id === first.id).turnList?.length, { timeout: 20_000 }).toBe(2);
  await expect.poll(async () => (await agents(yap)).find((j) => j.id === first.id).turnList[1].status, { timeout: 30_000 }).toBe('done');
  const agent = await ended(yap, first.id);
  await messagesOf(yap, conversation, 8);
  return { agent, conversation, before, first };
}

test.beforeEach(async ({ yap, main, fakeLlm }) => {
  freshFolder(GARDEN);
  await yap.invoke('e2e_claude_approve_auto', { allow: true });
  await setConfig(yap, main, {
    agentsEnabled: true,
    agentsAsk: 'new-or-web',
    agentsSpeak: 'never',
    agentsPay: 'plan',
    agentsMode: 'auto',
    webSearch: false,
    speakReplies: false,
    theme: 'light',
    llmScopes: chatScope(fakeLlm),
  });
  if (!(await yap.invoke('workspaces_list')).some((f) => f.name === 'garden-planner')) {
    await yap.invoke('workspaces_add', { path: GARDEN });
  }
});

// ---------------------------------------------------------------- no app

test('core/agents.js: an agent’s turns from its log, their words, the pills a turn each, a report’s turn, tab names apart', () => {
  // An older agent (no turns kept): one, from the agent, its whole log.
  const old = { id: 'a', brief: 'Tidy notes.md. Keep the headings.', status: 'done', files: ['notes.md'], text: 'Done.', messageId: 'chat:5:0' };
  const one = turnsOf(old, [{ step: 'Reading notes.md' }, { step: 'Editing notes.md', file: 'notes.md' }]);
  expect(one.map((t) => [t.n, t.title, t.status, t.events.length])).toEqual([[1, 'Tidy notes.md', 'done', 2]]);
  expect(titleOf('Add a minimap button to the map view. It shows every bed.')).toBe('Add a minimap button to the map view');
  expect(titleOf('could you add a minimap button for it as well?')).toBe('could you add a minimap button for it as well');
  // Turns kept: each event with its turn; one without goes with the turn before it.
  const agent = {
    id: 'b',
    status: 'running',
    turnList: [
      { n: 1, title: 'Add a watering schedule', status: 'done', startedTs: 1, files: ['notes.md'] },
      { n: 2, title: 'Add a minimap button', status: 'running', startedTs: 2, files: [] },
      { n: 3, title: 'Add a legend', status: 'queued', startedTs: 0 },
    ],
  };
  const log = [{ turn: 1, step: 'Editing notes.md' }, { step: 'Got your message' }, { turn: 2, step: 'Writing minimap.js' }, { turn: 3, queued: 'Add a legend' }, { turn: 4, step: 'Starting Claude Code', status: 'running' }];
  const turns = turnsOf(agent, log);
  expect(turns.map((t) => [t.n, t.events.length])).toEqual([
    [1, 2],
    [2, 1],
    [3, 1],
    [4, 1],
  ]);
  expect(turns[3].title, 'a turn this window hasn’t read yet: its own, until it does').toBe('');
  expect(turns.map(turnStatusWord)).toEqual(['Done', 'Running', 'Queued', 'Running']);
  expect(turnStatusWord({ status: 'stopped', startedTs: 0 })).toBe('Not sent yet');
  // Undo this turn: over, its files, not undone, backups made.
  const over = { ...agent, status: 'done' };
  expect(canUndoTurn(over, agent.turnList[0])).toBe(true);
  expect(canUndoTurn(agent, agent.turnList[0]), 'not while it works').toBe(false);
  expect(canUndoTurn(over, agent.turnList[1]), 'no files').toBe(false);
  expect(canUndoTurn(over, { ...agent.turnList[0], undone: true })).toBe(false);
  expect(canUndoTurn({ ...over, noBackups: true }, agent.turnList[0])).toBe(false);
  // A pill a turn, under the answer after its question; one an agent an answer.
  const messages = [{ role: 'user' }, { role: 'assistant' }, { role: 'user', kind: 'agentReport' }, { role: 'assistant' }, { role: 'user' }, { role: 'assistant' }];
  const turned = { id: 'c', conversation: 9, createdTs: 1, turnList: [{ n: 1, messageId: 'chat:9:0', status: 'done', title: 'One' }, { n: 2, messageId: 'chat:9:4', status: 'running', title: 'Two' }] };
  const pills = pillsByAnswer([turned], 9, messages);
  expect([...pills.keys()]).toEqual([1, 5]);
  expect(pills.get(1).map((p) => [p.id, p.turn, p.turnStatus])).toEqual([['c', 1, 'done']]);
  expect(pills.get(5).map((p) => [p.id, p.turn, p.turnStatus, p.turnTitle])).toEqual([['c', 2, 'running', 'Two']]);
  // A report's agent and turn, by the turn's marker.
  const reported = { ...turned, folder: 'garden-planner', reportMessageId: 'chat:9:7', turnList: [{ ...turned.turnList[0], reportMessageId: 'chat:9:2' }, { ...turned.turnList[1], reportMessageId: 'chat:9:7' }] };
  expect(agentOfReport([reported], 'chat:9:2', 9)?.id).toBe('c');
  expect(turnOfReport(reported, 'chat:9:2')?.n).toBe(1);
  expect(turnOfReport(reported, 'chat:9:7')?.n).toBe(2);
  // One report for two turns (the second asked while the first worked): its latest.
  const together = { ...reported, turnList: reported.turnList.map((t) => ({ ...t, reportMessageId: 'chat:9:7' })) };
  expect(turnOfReport(together, 'chat:9:7')?.n).toBe(2);
  expect(agentOfReport([together], 'chat:9:7', 9)?.id).toBe('c');
  expect(turnOfReport(old, 'chat:5:2')).toBeNull();
  // Its work as the thread tells it: what it said, and a line for each
  // stretch of steps between its words.
  const itsLog = [
    { turn: 1, step: 'Starting Claude Code', ts: 1 },
    { turn: 1, said: "I'll look at the notes first.", ts: 2 },
    { turn: 1, step: 'Reading notes.md', ts: 3 },
    { turn: 1, step: 'Running npm test', command: 'npm test', about: 'Run the tests', ts: 4 },
    { turn: 1, step: 'Running npm run lint', command: 'npm run lint', ts: 5 },
    { turn: 1, step: 'Editing notes.md', file: 'notes.md', ts: 6 },
    { turn: 1, diff: { file: 'notes.md', lines: ['+Watering: twice a week'], added: 4, removed: 1, kind: 'edit' }, ts: 7 },
    { turn: 1, said: 'Now the legend.', ts: 8 },
    { turn: 1, step: 'Writing legend.md', file: 'legend.md', ts: 9 },
    { turn: 1, diff: { file: 'legend.md', lines: ['+Legend'], added: 12, removed: 0, kind: 'new' }, ts: 10 },
    { turn: 1, said: 'Done. The legend is in.', ts: 11 },
  ];
  const work = workOf(itsLog, 'Done. The legend is in.');
  // (Housekeeping before its first words isn't a stretch of work; its final words are its report's.)
  expect(work.map((w) => [w.kind, w.text ?? w.steps.length])).toEqual([
    ['said', "I'll look at the notes first."],
    ['work', 4],
    ['said', 'Now the legend.'],
    ['work', 1],
  ]);
  expect(workSummary(work[1])).toEqual([{ text: 'Ran 2 commands' }, { text: 'read a file' }, { text: 'edited notes.md', added: 4, removed: 1 }]);
  // One step: its own line, as done, with what it changed.
  expect(workSummary(work[3])).toEqual([{ text: 'Wrote legend.md', added: 12, removed: 0 }]);
  // The stretch it's on now: the step it's on, as it is.
  expect(workSummary(work[1], true)).toEqual([{ text: 'Editing notes.md' }]);
  // Still working: its words so far all show, and housekeeping alone is what it's on.
  expect(workOf(itsLog).at(-1)).toMatchObject({ kind: 'said', text: 'Done. The legend is in.' });
  expect(workOf([{ step: 'Starting Claude Code' }], '', true).map((w) => workSummary(w, true))).toEqual([[{ text: 'Starting Claude Code' }]]);
  expect(workOf([{ step: 'Starting Claude Code' }])).toEqual([]);
  // Several files: counted, their lines added up; searches and the web in words.
  const many = workOf([
    { step: 'Editing a.js', file: 'a.js' },
    { diff: { file: 'a.js', lines: [], added: 2, removed: 2, kind: 'edit' } },
    { step: 'Editing b.js', file: 'b.js' },
    { diff: { file: 'b.js', lines: [], added: 5, removed: 0, kind: 'edit' } },
    { step: 'Searching the files for "legend"' },
    { step: 'Searching the web for bed spacing' },
  ]);
  expect(workSummary(many[0])).toEqual([{ text: 'Searched the files' }, { text: 'looked on the web' }, { text: 'edited 2 files', added: 7, removed: 2 }]);
  // Yap's browser beside the chat: each action in its own words, as Claude Code's line says them.
  const browsed = ['Opening wiki.example.com/guide beside the chat', 'Finding “Build Order” on the page beside the chat', 'Clicking on the page beside the chat', 'Reading the page beside the chat', 'Finding “Damage” on the page beside the chat', 'Clicking on the page beside the chat', 'Looking at the page beside the chat'].map((step) => ({ step }));
  expect(workSummary({ steps: browsed, edits: [] }).map((p) => p.text)).toEqual(['Opened a page', 'found on page 2 times', 'clicked 2 times', 'read the page', 'captured page']);
  expect(workOf([])).toEqual([]);
  // A picture a tool call came back with is on that call's step, in whichever
  // stretch it is, its address the scheme's (the agent's id, then the picture's).
  const seen = [
    { id: 'ab12', turn: 1, step: 'Reading beds.png', call: 'toolu_a', ts: 1 },
    { id: 'ab12', turn: 1, step: 'Running ls', command: 'ls', call: 'toolu_b', ts: 2 },
    { id: 'ab12', turn: 1, picture: { id: 'img_0123456789abcdef', w: 320, h: 200 }, call: 'toolu_a', ts: 3 },
    { id: 'ab12', turn: 1, said: 'Four beds.', ts: 4 },
    { id: 'ab12', turn: 1, step: 'Using shots · take screenshot', call: 'toolu_c', ts: 5 },
    // (Its picture came after the next words: still its step's.)
    { id: 'ab12', turn: 1, said: 'And the screen.', ts: 6 },
    { id: 'ab12', turn: 1, picture: { id: 'img_fedcba9876543210', w: 1600, h: 900 }, call: 'toolu_c', ts: 7 },
    { id: 'ab12', turn: 1, picture: { id: 'img_fedcba9876543210', w: 1600, h: 900 }, call: 'toolu_c', ts: 7 },
  ];
  expect(stepPicture(seen[2])).toEqual({ id: 'agent/ab12/img_0123456789abcdef', w: 320, h: 200 });
  expect(stepPicture(seen[0])).toBeNull();
  expect(stepPicture({ picture: { id: 'img_0123456789abcdef' } })).toBeNull();
  const looked = workOf(seen);
  expect(looked.map((w) => [w.kind, w.text ?? w.steps.map((x) => x.pictures.length)])).toEqual([
    ['work', [1, 0]],
    ['said', 'Four beds.'],
    ['work', [1]],
    ['said', 'And the screen.'],
  ]);
  expect(groupPictures(looked[0])).toEqual([{ id: 'agent/ab12/img_0123456789abcdef', w: 320, h: 200, caption: 'Read beds.png' }]);
  expect(groupPictures(looked[2]).map((p) => p.caption)).toEqual(['Used shots · take screenshot']);
  expect(workSummary(looked[2])).toEqual([{ text: 'Used shots · take screenshot' }]);
  // Yap's own housekeeping alone isn't a line, unless a step of it came back with a picture.
  expect(workOf([{ id: 'ab12', step: 'Checking a command', call: 'toolu_d' }])).toEqual([]);
  expect(
    workOf([
      { id: 'ab12', step: 'Checking a command', call: 'toolu_d' },
      { id: 'ab12', picture: { id: 'img_4444444444444444', w: 8, h: 8 }, call: 'toolu_d' },
    ]).map((w) => workSummary(w))
  ).toEqual([[{ text: 'Checking a command' }]]);
  expect(workSummary(looked[0])).toEqual([{ text: 'Ran a command' }, { text: 'read a file' }]);
  // Other tools' calls are steps of the work too: an MCP server's or a connector's, a skill, a tool Yap has no
  // words for, a part handed to a helper. Yap's own housekeeping isn't (its permission prompts' tool, a note read…).
  const kindOf = (step) => stepKind({ step });
  expect(['Using homelights · list lights', 'Using a skill · tidy-notes', 'Using Frobnicate', 'Handing part of it to a helper · Check the docs'].map(kindOf)).toEqual(['tool', 'tool', 'tool', 'helper']);
  expect(['Using yap · approve', 'Starting Claude Code', 'Read your note', 'Not allowed: Bash', 'Checking a command', 'Finding a tool'].map(kindOf)).toEqual(['other', 'other', 'other', 'other', 'other', 'other']);
  // (Claude Code looking a tool up before it uses it is plumbing: the call that follows is the step.)
  expect(workSummary(workOf([{ step: 'Finding a tool' }, { step: 'Using homelights · list lights' }])[0])).toEqual([{ text: 'Used homelights · list lights' }]);
  expect(stepKind({ step: 'Using the linter', command: 'npm run lint' })).toBe('command');
  // A stretch of one: its own line, as done. (Before, a turn that only used other tools showed no line at all.)
  const alone = (step) => workOf([{ step }]).map((w) => workSummary(w));
  expect(alone('Using homelights · list lights')).toEqual([[{ text: 'Used homelights · list lights' }]]);
  expect(alone('Using a skill · tidy-notes')).toEqual([[{ text: 'Used a skill · tidy-notes' }]]);
  expect(alone('Handing part of it to a helper · Check the docs')).toEqual([[{ text: 'Handed part of it to a helper · Check the docs' }]]);
  expect(workOf(seen.filter((e) => !e.picture)).map((w) => w.kind)).toEqual(['work', 'said', 'work', 'said']);
  expect(workOf([{ step: 'Using yap · approve' }, { step: 'Starting Claude Code' }, { step: 'Not allowed: Bash' }])).toEqual([]);
  // Several: named when they're all one's, counted otherwise; a helper in words.
  const sameTool = workOf([{ step: 'Running npm test', command: 'npm test' }, { step: 'Using homelights · list lights' }, { step: 'Using homelights · set light' }, { step: 'Reading notes.md' }]);
  expect(workSummary(sameTool[0])).toEqual([{ text: 'Ran a command' }, { text: 'read a file' }, { text: 'used homelights 2 times' }]);
  expect(workSummary(workOf([{ step: 'Running ls', command: 'ls' }, { step: 'Using a skill · tidy-notes' }])[0])).toEqual([{ text: 'Ran a command' }, { text: 'used a skill' }]);
  const mixed = workOf([
    { step: 'Using homelights · list lights' },
    { step: 'Using tides · next high tide' },
    { step: 'Using a skill · tidy-notes' },
    { step: 'Handing part of it to a helper · Check the docs' },
    { step: 'Handing part of it to a helper · Check the tests' },
    { step: 'Editing notes.md', file: 'notes.md' },
  ]);
  expect(workSummary(mixed[0])).toEqual([{ text: 'Used 3 tools' }, { text: 'handed parts of it to 2 helpers' }, { text: 'edited notes.md' }]);
  expect(workSummary(workOf([{ step: 'Reading notes.md' }, { step: 'Handing part of it to a helper · Check the docs' }])[0])).toEqual([{ text: 'Read a file' }, { text: 'handed part of it to a helper' }]);
  // A step as its row says it, with its own diff and what its command printed (each by its call).
  const fourSteps = workOf([
    { id: 'ab12', step: 'Reading src\\app.ts', range: '424–503', call: 'c1' },
    { id: 'ab12', step: 'Editing notes.md', file: 'notes.md', call: 'c2' },
    { id: 'ab12', step: 'Editing notes.md', file: 'notes.md', call: 'c3' },
    { id: 'ab12', diff: { file: 'notes.md', kind: 'edit', lines: ['@@ -4,3 +4,3 @@', ' line four', '-line five', '+line 5', ' line six', '@@ -40,1 +40,2 @@', ' last', '+added'], added: 2, removed: 1 }, call: 'c2' },
    { id: 'ab12', diff: { file: 'notes.md', kind: 'edit', lines: ['-a', '+b'], added: 1, removed: 1 }, call: 'c3' },
    { id: 'ab12', step: 'Checking the edits parse', command: 'node --check notes.js', about: 'Check the edits parse', done: 'Checked the edits parse', call: 'c4' },
    { id: 'ab12', output: { text: 'SyntaxError: nope', failed: true }, call: 'c4' },
    { id: 'ab12', step: 'Running ls', command: 'ls', call: 'c5' },
    { id: 'ab12', step: 'Writing legend.md', file: 'legend.md', call: 'c6' },
    { id: 'ab12', diff: { file: 'legend.md', kind: 'new', lines: ['+# Legend', '+', '+B: a bed'], added: 3, removed: 0 }, call: 'c6' },
    { id: 'ab12', step: 'Using homelights · list lights', call: 'c7' },
  ])[0];
  expect(fourSteps.steps.map((x) => stepRow(x))).toEqual([
    { text: 'Read', name: 'app.ts', tail: '(424–503)' },
    { text: 'Edited', name: 'notes.md', added: 2, removed: 1 },
    { text: 'Edited', name: 'notes.md', added: 1, removed: 1 },
    { text: 'Checked the edits parse', failed: true },
    { text: 'Ran ls' },
    { text: 'Created', name: 'legend.md', added: 3, removed: 0 },
    { text: 'Used homelights · list lights' },
  ]);
  // Plumbing is no row once it's past: only while it's what the agent is on, or when it came back with a picture.
  const plumbed = [{ step: 'Starting Claude Code', pictures: [] }, { step: 'Finding a tool', pictures: [] }, { step: 'Reading notes.md', pictures: [] }, { step: 'Not allowed: Bash', pictures: [] }, { step: 'Finding a tool', pictures: [] }];
  expect(rowSteps(plumbed).map((r) => [r.step.step, r.i, r.now])).toEqual([['Reading notes.md', 2, false], ['Not allowed: Bash', 3, false]]);
  expect(rowSteps(plumbed, true).map((r) => [r.step.step, r.i, r.now])).toEqual([['Reading notes.md', 2, false], ['Not allowed: Bash', 3, false], ['Finding a tool', 4, true]]);
  expect(rowSteps([{ step: 'Finding a tool', pictures: [{ id: 'p' }] }]).length).toBe(1);
  expect(rowSteps(null)).toEqual([]);
  // Which rows open: an edit to its diff, a command to itself; a read and another tool's call have nothing more.
  expect(fourSteps.steps.map(stepOpens)).toEqual([false, true, true, true, true, true, false]);
  // The step it's on now: as it is.
  expect(stepRow(fourSteps.steps[3], true)).toEqual({ text: 'Checking the edits parse' });
  // The stretch's own line says how many failed, as Claude Code's does.
  expect(workSummary(fourSteps).map((p) => p.text)).toEqual(['Ran 2 commands', 'read a file', 'used homelights', 'edited 2 files', '1 failed']);
  // A diff's lines numbered: a removed line by its number before, the others after; a gap between its parts, none before the first.
  expect(diffRows(fourSteps.steps[1].diff).map((x) => [x.mark, x.no, x.text])).toEqual([
    ['ctx', 4, 'line four'],
    ['del', 5, 'line five'],
    ['add', 5, 'line 5'],
    ['ctx', 6, 'line six'],
    ['gap', null, ''],
    ['ctx', 40, 'last'],
    ['add', 41, 'added'],
  ]);
  // No place said (an older log, the stand-in): no numbers; a file written whole: from 1.
  expect(diffRows(fourSteps.steps[2].diff).map((x) => [x.mark, x.no])).toEqual([['del', null], ['add', null]]);
  expect(diffRows(fourSteps.steps[5].diff).map((x) => [x.mark, x.no, x.text])).toEqual([['add', 1, '# Legend'], ['add', 2, ''], ['add', 3, 'B: a bed']]);
  expect(diffRows({ kind: 'edit', lines: ['-a', '@', '+b'] }).map((x) => x.mark)).toEqual(['del', 'gap', 'add']);
  expect(diffRows(null)).toEqual([]);
  // A diff with no call (an older log's): the earliest step that changes that file and has none yet.
  const olderLog = workOf([{ step: 'Editing a.js', file: 'a.js' }, { step: 'Editing a.js', file: 'a.js' }, { diff: { file: 'a.js', kind: 'edit', lines: ['+1'], added: 1, removed: 0 } }, { diff: { file: 'a.js', kind: 'edit', lines: ['+2'], added: 1, removed: 0 } }])[0];
  expect(olderLog.steps.map((x) => x.diff?.lines)).toEqual([['+1'], ['+2']]);
  // A helper's own steps (`under`: the call that handed it the work) fold behind that call's row: never
  // steps of the session's own stretch, never counted in its line; its edit is its row's alone.
  const handed = workOf([
    { id: 'ab12', step: 'Reading notes.md', call: 'c1' },
    { id: 'ab12', step: 'Handing part of it to a helper · Check the docs', call: 't1' },
    { id: 'ab12', step: 'Reading docs\\guide.md', call: 'h1', under: 't1' },
    { id: 'ab12', step: 'Editing docs\\guide.md', file: 'docs\\guide.md', call: 'h2', under: 't1' },
    { id: 'ab12', diff: { file: 'docs\\guide.md', kind: 'edit', lines: ['-a', '+b'], added: 1, removed: 1 }, call: 'h2' },
    { id: 'ab12', step: 'Running ls', command: 'ls', call: 'c2' },
    { id: 'ab12', step: 'Editing notes.md', file: 'notes.md', call: 'c3' },
    { id: 'ab12', diff: { file: 'notes.md', kind: 'edit', lines: ['+x'], added: 1, removed: 0 }, call: 'c3' },
  ]);
  expect(handed).toHaveLength(1);
  expect(handed[0].steps.map((x) => [x.step, x.helped.map((h) => h.step)])).toEqual([
    ['Reading notes.md', []],
    ['Handing part of it to a helper · Check the docs', ['Reading docs\\guide.md', 'Editing docs\\guide.md']],
    ['Running ls', []],
    ['Editing notes.md', []],
  ]);
  expect(workSummary(handed[0])).toEqual([{ text: 'Ran a command' }, { text: 'read a file' }, { text: 'handed part of it to a helper' }, { text: 'edited notes.md', added: 1, removed: 0 }]);
  expect(handed[0].edits).toEqual([{ file: 'notes.md', added: 1, removed: 0, made: null }]);
  expect(handed[0].steps[1].helped[1].diff.lines).toEqual(['-a', '+b']);
  // Its row opens to them; a step of theirs is a row like any other.
  expect(handed[0].steps.map(stepOpens)).toEqual([false, true, true, true]);
  expect(handed[0].steps[1].helped.map((h) => stepRow(h))).toEqual([{ text: 'Read', name: 'guide.md' }, { text: 'Edited', name: 'guide.md', added: 1, removed: 1 }]);
  // The timeline keeps every step in order, a helper's own saying so (it sits in from the edge).
  expect(timelineOf([{ id: 'ab12', step: 'Handing part of it to a helper · Check the docs', call: 't1' }, { id: 'ab12', step: 'Reading x.md', call: 'h1', under: 't1' }]).map((t) => t.under)).toEqual([null, 't1']);
  // **A turn's helpers**, as the Agent panel lists them: an entry for each part handed over — what it was
  // asked, its own steps, working until it's back (while the turn is live), then done with how many steps.
  const handedEvents = [
    { id: 'ab12', step: 'Reading notes.md', call: 'c1' },
    { id: 'ab12', step: 'Handing part of it to a helper · Check the docs', call: 't1' },
    { id: 'ab12', step: 'Reading docs\\guide.md', call: 'h1', under: 't1' },
    { id: 'ab12', step: 'Editing docs\\guide.md', file: 'docs\\guide.md', call: 'h2', under: 't1' },
  ];
  const brief = (list) => list.map((h) => [h.key, h.title, h.status, h.line, h.steps.map((x) => x.step)]);
  expect(brief(helpersOf(handedEvents, true))).toEqual([['t1', 'Check the docs', 'working', 'Editing docs\\guide.md', ['Reading docs\\guide.md', 'Editing docs\\guide.md']]]);
  expect(brief(helpersOf([...handedEvents, { id: 'ab12', helper: 'back', call: 't1' }], true))).toEqual([['t1', 'Check the docs', 'done', '2 steps', ['Reading docs\\guide.md', 'Editing docs\\guide.md']]]);
  expect(helpersOf([...handedEvents, { id: 'ab12', helper: 'failed', call: 't1' }], true).map((h) => [h.status, h.line])).toEqual([['failed', "Didn't finish · 2 steps"]]);
  // The turn over: none still at work, whatever its log says (an older log has no "back").
  expect(helpersOf(handedEvents, false).map((h) => [h.status, h.line])).toEqual([['done', '2 steps']]);
  // Several at once, each its own; one with no words of its own, one that hasn't started.
  const several = helpersOf(
    [
      { id: 'ab12', step: 'Handing part of it to a helper · Check the docs', call: 't1' },
      { id: 'ab12', step: 'Handing part of it to a helper', call: 't2' },
      { id: 'ab12', step: 'Reading a.md', call: 'h1', under: 't2' },
      { id: 'ab12', helper: 'back', call: 't2' },
    ],
    true
  );
  expect(several.map((h) => [h.key, h.title, h.status, h.line])).toEqual([
    ['t1', 'Check the docs', 'working', 'Starting'],
    ['t2', 'A helper', 'done', '1 step'],
  ]);
  expect(helpersOf([{ id: 'ab12', step: 'Reading notes.md', call: 'c1' }], true)).toEqual([]);
  expect(helpersOf(null, true)).toEqual([]);
  // The hand-over's step knows its helper is back; a "back" is never a step of the work.
  const backed = workOf([...handedEvents, { id: 'ab12', helper: 'back', call: 't1' }]);
  expect(backed[0].steps.map((x) => [x.step, x.back])).toEqual([['Reading notes.md', null], ['Handing part of it to a helper · Check the docs', 'back']]);
  expect(timelineOf([...handedEvents, { id: 'ab12', helper: 'back', call: 't1' }])).toHaveLength(4);
  // No such row (an older log, a call Yap never heard of): a step like any other.
  expect(workOf([{ id: 'ab12', step: 'Reading x.md', call: 'h9', under: 'gone' }]).map((w) => w.steps.map((x) => x.step))).toEqual([['Reading x.md']]);
  // The Agent tab's timeline keeps them on their steps too.
  expect(timelineOf(seen).filter((t) => t.kind === 'step').map((t) => t.pictures.length)).toEqual([1, 0, 1]);
  // The same line twice in a row is one step: both calls' pictures are its.
  const twice = workOf([
    { id: 'ab12', step: 'Using shots · take screenshot', call: 'toolu_1' },
    { id: 'ab12', step: 'Using shots · take screenshot', call: 'toolu_2' },
    { id: 'ab12', picture: { id: 'img_1111111111111111', w: 8, h: 8 }, call: 'toolu_1' },
    { id: 'ab12', picture: { id: 'img_2222222222222222', w: 8, h: 8 }, call: 'toolu_2' },
  ]);
  expect(twice[0].steps.map((x) => x.pictures.map((p) => p.id))).toEqual([['agent/ab12/img_1111111111111111', 'agent/ab12/img_2222222222222222']]);
  // One with no call (an older log's): its newest step's.
  expect(groupPictures(workOf([{ id: 'ab12', step: 'Reading beds.png' }, { id: 'ab12', picture: { id: 'img_3333333333333333', w: 8, h: 8 } }])[0]).length).toBe(1);
  // As the panel's picture tab and the viewer take one.
  expect(shownStepPicture(groupPictures(looked[0])[0], 'garden-planner')).toEqual({
    id: 'agent/ab12/img_0123456789abcdef',
    w: 320,
    h: 200,
    caption: 'Read beds.png',
    alt: '',
    imageUrl: '',
    page: { title: 'garden-planner', site: 'garden-planner', url: '' },
    n: 0,
  });
  // Two agent tabs in one folder are named apart: their first requests, else a number.
  const a = { kind: 'agent', agent: { id: 'j1', folder: 'garden-planner', turnList: [{ title: 'Add a watering schedule' }] } };
  const b = { kind: 'agent', agent: { id: 'j2', folder: 'garden-planner', turnList: [{ title: 'Start over on the planner' }] } };
  const c = { kind: 'agent', agent: { id: 'j3', folder: 'recipe-box' } };
  expect(agentTabName(c, [a, c])).toBe('Agent · recipe-box');
  expect(agentTabName(a, [a, b, c])).toBe('Agent · garden-planner · Add a watering schedule');
  expect(agentTabName(b, [a, b, c])).toBe('Agent · garden-planner · Start over on the planner');
  const refOnly = { kind: 'agent', agent: { id: 'j2', folder: 'garden-planner' } };
  expect(agentTabName(refOnly, [a, refOnly], (id) => (id === 'j2' ? b.agent : null))).toBe('Agent · garden-planner · Start over on the planner');
  const twin = { kind: 'agent', agent: { id: 'j4', folder: 'garden-planner', turnList: [{ title: 'Add a watering schedule' }] } };
  expect(agentTabName(twin, [a, twin])).toBe('Agent · garden-planner #2');
  // Two agents from before turns in one Code chat (as the maintainer had): their briefs' first words.
  const older = { kind: 'agent', agent: { id: 'j5', folder: 'garden-planner' } };
  expect(agentTabName(older, [a, older], (id) => (id === 'j5' ? { brief: 'Fix the beds page. Then run the tests.' } : null))).toBe('Agent · garden-planner · Fix the beds page');
  // How long it worked: its turns added up, not the hours between them; the running list, its turn now.
  const worked = { status: 'done', startedTs: 100, endedTs: 7300, turnList: [{ startedTs: 100, endedTs: 160 }, { startedTs: 7200, endedTs: 7300 }] };
  expect(workedFor(worked, 8000)).toBe('2 min');
  expect(workedFor({ ...worked, status: 'running', turnList: [worked.turnList[0], { startedTs: 7200 }] }, 7230)).toBe('1 min');
  expect(workedFor({ status: 'done', startedTs: 100, endedTs: 160 }, 999), 'an agent from before turns').toBe('1 min');
  expect(turnNow(worked)).toEqual({ startedTs: 7200, endedTs: 7300 });
  expect(turnNow({ turnList: [{ startedTs: 5 }, { status: 'queued', startedTs: 0 }] })).toEqual({ startedTs: 5 });
});

// ---------------------------------------------------------------- one agent, its turns

test('core/codeColours.js: a file’s language, a command’s shell, the highlighter’s HTML as tokens, a diff coloured across its lines', () => {
  // The highlighter's chunk has exactly the languages the rows may ask for.
  expect(hljs.listLanguages().sort()).toEqual(LANGUAGES);
  // A file's language by its name; one Yap has no language for stays plain (nothing is guessed).
  expect(['src\\lib\\core\\agents.js', 'app.TSX', 'StepRows.svelte', 'beds/plan.json', 'Bar.lua', 'steps.rs', 'Dockerfile', 'notes.md', 'deploy.ps1'].map(languageOf)).toEqual([
    'javascript',
    'typescript',
    'xml',
    'json',
    'lua',
    'rust',
    'dockerfile',
    'markdown',
    'powershell',
  ]);
  expect(['notes.txt', '.gitignore', 'README', 'beds.png', '', null].map(languageOf)).toEqual([null, null, null, null, null, null]);
  // A rendered code block's language (core/markdown.js: a class on its code), as the highlighter is asked for
  // it: its own short names as they are, a few it calls something else; whether it has one is colourLines' to say.
  expect(['lang-js', 'md lang-svelte', 'lang-shell', 'lang-c++', 'lang-PS', 'lang-klingon', 'md-code', '', null].map(fenceLanguage)).toEqual(['js', 'xml', 'bash', 'cpp', 'powershell', 'klingon', null, null, null]);
  expect(colourLines(hljs, 'let a = 1;', fenceLanguage('lang-js'))[0].map((t) => t.kind)).toEqual(['k', '', 'n', '']);
  expect(colourLines(hljs, 'x', fenceLanguage('lang-klingon'))).toBeNull();
  // A command's shell by how it's written.
  expect(['npm test', 'git log --oneline -5', 'Get-ChildItem src | Select-Object Name', 'echo $env:PATH', 'ls | ForEach-Object { $_ }'].map(shellOf)).toEqual(['bash', 'bash', 'powershell', 'powershell', 'powershell']);
  // The highlighter's HTML read into tokens: its text unescaped, never markup; the innermost span that has a colour wins.
  expect(tokensOf('a &lt; b <span class="hljs-string">&quot;x&#x27;&quot;</span> &amp; <span class="hljs-tag">&lt;<span class="hljs-name">b</span>&gt;</span>')).toEqual([
    { text: 'a < b ', kind: '' },
    { text: '"x\'"', kind: 's' },
    { text: ' & <', kind: '' },
    { text: 'b', kind: 'a' },
    { text: '>', kind: '' },
  ]);
  expect(tokensOf('1 < 2')).toEqual([{ text: '1 < 2', kind: '' }]);
  expect([kindOf(['hljs-title function_']), kindOf(['hljs-string', 'hljs-subst']), kindOf(['language-javascript']), kindOf([])]).toEqual(['f', '', '', '']);
  // …as lines: a token that runs over a line end is cut there.
  expect(lineTokens([{ text: 'a\n\nb', kind: 'c' }, { text: 'c', kind: '' }])).toEqual([[{ text: 'a', kind: 'c' }], [], [{ text: 'b', kind: 'c' }, { text: 'c', kind: '' }]]);
  // Code coloured line by line.
  const js = colourLines(hljs, "export function water(bed) {\n  return bed.litres * 2; // twice\n}", 'javascript');
  expect(js.map((l) => l.filter((t) => t.kind).map((t) => [t.kind, t.text]))).toEqual([
    [['k', 'export'], ['k', 'function'], ['f', 'water']],
    [['k', 'return'], ['a', 'litres'], ['n', '2'], ['c', '// twice']],
    [],
  ]);
  expect(js.map((l) => l.map((t) => t.text).join(''))).toEqual(['export function water(bed) {', '  return bed.litres * 2; // twice', '}']);
  expect(colourLines(hljs, 'npm test -- --watch "beds spec"', 'bash').flat().filter((t) => t.kind)).toEqual([
    { text: 'test', kind: 't' },
    { text: '"beds spec"', kind: 's' },
  ]);
  // No language, one the highlighter wasn't given, or no highlighter yet: plain.
  expect([colourLines(hljs, 'x', null), colourLines(hljs, 'x', 'klingon'), colourLines(null, 'x', 'javascript')]).toEqual([null, null, null]);
  // A diff keeps its sense across lines: a comment that starts above the change is still one on the added
  // lines, and the removed line is coloured as it was before.
  const rows = [
    { mark: 'ctx', text: '/* a comment that', no: 1 },
    { mark: 'del', text: '   ends here */ const old = 1;', no: 2 },
    { mark: 'add', text: '   goes on', no: 2 },
    { mark: 'add', text: '   and ends */ const now = "two";', no: 3 },
    { mark: 'gap', text: '', no: null },
    { mark: 'ctx', text: 'return now;', no: 40 },
  ];
  const coloured = colourDiff(hljs, rows, 'javascript');
  expect(coloured.map((r) => (r.tokens || []).map((t) => t.kind || '-').join(''))).toEqual(['c', 'c-k-n-', 'c', 'c-k-s-', '', 'k-']);
  expect(coloured.map((r) => [r.mark, r.no, r.text])).toEqual(rows.map((r) => [r.mark, r.no, r.text]));
  // No language: the same rows, plain.
  expect(colourDiff(hljs, rows, null).every((r) => r.tokens === undefined)).toBe(true);
  expect(colourDiff(null, rows, 'javascript').every((r) => r.tokens === undefined)).toBe(true);
});

test('core/diffWords.js: the part of a line that was replaced, which lines go together, a line as pieces', () => {
  const show = (t, r) => (r ? `${t.slice(0, r[0])}[${t.slice(r[0], r[1])}]${t.slice(r[1])}` : t);
  const marked = (a, b) => {
    const c = wordChanges(a, b);
    return c ? [show(a, c.before), show(b, c.after)] : null;
  };
  // What two lines share at their start and their end is the same; what lies between was replaced.
  expect(marked('  return bed.litres * 2;', '  return bed.litres * 3;')).toEqual(['  return bed.litres * [2];', '  return bed.litres * [3];']);
  // Whole words: a mark never stops in the middle of a name.
  expect(marked('const beds1 = sort(all);', 'const beds2 = sort(all);')).toEqual(['const [beds1] = sort(all);', 'const [beds2] = sort(all);']);
  // Text only added, or only taken out: the other line has no mark.
  expect(marked('water(bed)', 'water(bed, days)')).toEqual(['water(bed)', 'water(bed[, days])']);
  expect(marked('const total = count + 1;', 'const total = count;')).toEqual(['const total = count[ + 1];', 'const total = count;']);
  // A bracket opened inside both stretches closes inside them.
  expect(marked('rows.map((e) => [e.className.trim(), e.id])', "rows.map((e) => [['a', 'b'].find((m) => e.has(m)), e.id])")).toEqual([
    'rows.map((e) => [[e.className.trim()], e.id])',
    "rows.map((e) => [[['a', 'b'].find((m) => e.has(m))], e.id])",
  ]);
  // Lines with little in common are a line removed and a line added: no mark. The same line, or an empty one: none.
  expect([marked('return a;', 'throw new Error("no");'), marked('same', 'same'), marked('', 'new line')]).toEqual([null, null, null]);
  // Which lines go together: in a run of removed lines followed at once by added ones. A different number of
  // each: every removed line with the most alike added line after the last one taken.
  const texts = (rows) => rows.map((r) => show(r.text, r.changed?.[0]));
  expect(
    texts(
      pairChanges([
        { mark: 'ctx', text: 'function water(bed) {' },
        { mark: 'del', text: '  const days = 2;' },
        { mark: 'del', text: '  return bed.litres * days;' },
        { mark: 'add', text: '  // how often' },
        { mark: 'add', text: '  const days = 3;' },
        { mark: 'add', text: '  return bed.litres * days * 2;' },
        { mark: 'ctx', text: '}' },
        { mark: 'gap', text: '' },
        // (Not one run: an unchanged line between them.)
        { mark: 'del', text: 'const a = 1;' },
        { mark: 'ctx', text: 'between' },
        { mark: 'add', text: 'const a = 2;' },
      ])
    )
  ).toEqual(['function water(bed) {', '  const days = [2];', '  return bed.litres * days;', '  // how often', '  const days = [3];', '  return bed.litres * days[ * 2];', '}', '', 'const a = 1;', 'between', 'const a = 2;']);
  // The same number of each: each with the one at its place.
  expect(texts(pairChanges([{ mark: 'del', text: 'let a = 1;' }, { mark: 'del', text: 'let b = 2;' }, { mark: 'add', text: 'let a = 10;' }, { mark: 'add', text: 'let b = 20;' }]))).toEqual([
    'let a = [1];',
    'let b = [2];',
    'let a = [10];',
    'let b = [20];',
  ]);
  expect(pairChanges(null)).toEqual([]);
  // A line as pieces to draw: its coloured tokens cut where the marked stretch starts and ends.
  const tokens = [{ text: 'return', kind: 'k' }, { text: ' bed.', kind: '' }, { text: 'litres', kind: 'a' }, { text: ' * ', kind: '' }, { text: '31', kind: 'n' }, { text: ';', kind: '' }];
  expect(linePieces({ text: 'return bed.litres * 31;', tokens, changed: [[18, 22]] }).map((p) => [p.text, p.kind, p.changed])).toEqual([
    ['return', 'k', false],
    [' bed.', '', false],
    ['litres', 'a', false],
    [' ', '', false],
    ['* ', '', true],
    ['31', 'n', true],
    [';', '', false],
  ]);
  // No colours yet: its plain text, cut the same way. No mark: the tokens as they are. An empty line: nothing.
  expect(linePieces({ text: 'abcdef', changed: [[2, 4]] }).map((p) => [p.text, p.changed])).toEqual([['ab', false], ['cd', true], ['ef', false]]);
  expect(linePieces({ text: 'plain' })).toEqual([{ text: 'plain', kind: '', changed: false }]);
  expect(linePieces({ text: '' })).toEqual([]);
  // The pieces always spell the line.
  expect(linePieces({ text: 'return bed.litres * 31;', tokens, changed: [[18, 22]] }).map((p) => p.text).join('')).toBe('return bed.litres * 31;');
});

test('two requests in a Code chat: one agent, one tab with two turns, two reports, the second on --resume', async ({ yap, main, fakeLlm, shot }) => {
  const { agent, conversation, before, first } = await twoTurns(yap, main, fakeLlm);
  // One agent, its two turns, one session.
  expect((await agents(yap)).length).toBe(before + 1);
  expect(agent.session).toBe(first.session);
  expect(agent.turnList.map((t) => [t.n, t.title, t.status])).toEqual([
    [1, 'Add a watering schedule', 'done'],
    [2, 'Add a minimap button', 'done'],
  ]);
  expect(agent.turnList.map((t) => t.messageId)).toEqual([`chat:${conversation}:0`, `chat:${conversation}:4`]);
  expect(agent.turnList[1].request).toBe('Add a minimap button for the beds, and say so in notes.md.');
  expect(agent.turnList[1].files).toEqual(['notes.md', 'minimap.js']);
  expect(agent.turnList[0].cost).toBeCloseTo(0.03, 5);
  expect(agent.turnList[1].cost).toBeCloseTo(0.02, 5);
  expect(agent.cost).toBeCloseTo(0.05, 5);
  expect(agent.text).toBe(DONE2);
  expect(read('notes.md')).toBe(NOTES2);
  // The follow-up's tool result: the same agent, its next turn.
  expect(lastTool(fakeLlm)).toMatchObject({ success: true, data: { id: agent.id, turn: 2, say: PASSED } });
  // The second turn resumed the first's session (Claude Code's own `--resume`).
  const args = argsSeen();
  expect(args).toHaveLength(2);
  expect(after(args[0], '--session-id')).toBe(agent.session);
  expect(args[0]).not.toContain('--resume');
  expect(after(args[1], '--resume')).toBe(agent.session);
  expect(args[1]).not.toContain('--session-id');
  // Each turn's own backups.
  const backup = path.join(yap.dataDir, 'agents', agent.id, 'backup');
  const manifest = (n) => JSON.parse(fs.readFileSync(path.join(backup, String(n), 'manifest.json'), 'utf8')).files.map((f) => [path.basename(f.original), f.state]);
  expect(manifest(1)).toEqual([['notes.md', 'copied']]);
  expect(manifest(2)).toEqual([
    ['notes.md', 'copied'],
    ['minimap.js', 'created'],
  ]);
  // Two reports, one a turn.
  const messages = await messagesOf(yap, conversation, 8);
  expect(messages.map((m) => [m.role, m.kind ?? ''])).toEqual([
    ['user', ''],
    ['assistant', ''],
    ['user', 'agentReport'],
    ['assistant', ''],
    ['user', ''],
    ['assistant', ''],
    ['user', 'agentReport'],
    ['assistant', ''],
  ]);
  expect(messages[2].text).toContain('Files it changed: notes.md (backed up first).');
  expect(messages[6].text).toContain('Files it changed: notes.md, minimap.js (backed up first).');
  expect(messages[6].text).toContain(DONE2);
  expect(agent.turnList.map((t) => t.reportMessageId)).toEqual([`chat:${conversation}:2`, `chat:${conversation}:6`]);

  // One tab: the panel shows the agent alone (no row of tabs), its two turns.
  await expect(tab(main)).toBeVisible();
  await expect(main.getByTestId('chat-panel-tabs')).toHaveCount(0);
  await expect(tab(main)).toHaveAttribute('data-turns', '2');
  // Its panel: a row a turn, the newest first, both finished.
  await expect(tab(main).getByTestId('chat-agent-row')).toHaveCount(2);
  await expect(tab(main).getByTestId('chat-agent-row').first()).toHaveAttribute('data-turn', '2');
  await expect(tab(main).locator('[data-testid="chat-agent-group"][data-group="finished"] [data-testid="chat-agent-group-head"]')).toHaveText('Finished · 2');
  await expect(rowAt(main, 1).getByTestId('chat-agent-row-title')).toHaveText('Add a watering schedule');
  await expect(rowAt(main, 2).getByTestId('chat-agent-row-title')).toHaveText('Add a minimap button');
  // Its whole timeline: a section a turn.
  await agentTimeline(main);
  await expect(turnsIn(main)).toHaveCount(2);
  await expect(turnAt(main, 1)).toHaveAttribute('data-open', 'false');
  await expect(turnAt(main, 2)).toHaveAttribute('data-open', 'true');
  await expect(turnAt(main, 1).locator('.ttitle')).toHaveText('Add a watering schedule');
  await expect(turnAt(main, 2).locator('.ttitle')).toHaveText('Add a minimap button');
  await expect(turnAt(main, 1).getByTestId('chat-agent-turn-facts')).toContainText('Done');
  await expect(turnAt(main, 1).getByTestId('chat-agent-turn-facts')).toContainText('≈ $0.03');
  await expect(turnAt(main, 2).getByTestId('chat-agent-turn-facts')).toContainText('≈ $0.02');
  await expect(main.getByTestId('chat-agent-cost')).toHaveText('Your Claude plan · ≈ $0.05 on the API in all');
  await expect(main.getByTestId('chat-agent-turn-count')).toHaveText('· 2 turns');
  // The newest open: its steps, files, words; the earlier folded.
  await expect(main.getByTestId('chat-agent-text')).toHaveCount(1);
  await expect(turnAt(main, 2).getByTestId('chat-agent-text')).toContainText(DONE2);
  await expect(turnAt(main, 2).getByTestId('chat-agent-files').locator('.fname')).toHaveText(['notes.md', 'minimap.js']);
  await expect(turnAt(main, 2).getByTestId('chat-agent-said')).toHaveText(['Now the minimap button.']);
  await expect(tab(main).getByRole('button', { name: 'Undo all' })).toBeVisible();
  await expect(turnAt(main, 2).getByTestId('chat-agent-undo-turn')).toBeVisible();
  // The earlier one opens to its own.
  await turnAt(main, 1).getByTestId('chat-agent-turn-head').click();
  await expect(turnAt(main, 1)).toHaveAttribute('data-open', 'true');
  await expect(turnAt(main, 1).getByTestId('chat-agent-text')).toContainText(DONE1);
  await expect(turnAt(main, 1).getByTestId('chat-agent-said')).toHaveText(["I'll add the watering schedule to notes.md."]);
  await expect(turnAt(main, 1).getByTestId('chat-agent-files').locator('.fname')).toHaveText(['notes.md']);
  await main.locator('.agentbody').evaluate((el) => el.scrollTo(0, 0));
  await shot(main, '01-agent-tab-two-turns-light');
  await theme(yap, main, 'dark');
  await shot(main, '02-agent-tab-two-turns-dark');
  await theme(yap, main, 'light');
  await turnAt(main, 1).getByTestId('chat-agent-turn-head').click();
  await expect(turnAt(main, 1)).toHaveAttribute('data-open', 'false');

  // A pill under each answer, a report card each: all the same tab, at their turn.
  const pills = thread(main).getByTestId('chat-agent-pill');
  await expect(pills).toHaveCount(2);
  await expect(pills.nth(0)).toHaveAttribute('data-turn', '1');
  await expect(pills.nth(1)).toHaveAttribute('data-turn', '2');
  await expect(pills.nth(1).locator('.jturn')).toHaveText('· Add a minimap button');
  await closeTab(main);
  await pills.nth(0).click();
  await expect(tab(main)).toBeVisible();
  await expect(rowAt(main, 1)).toHaveAttribute('data-open', 'true');
  // Turn 2's row was opened earlier (the timeline's Open): remembered while the window stays open.
  await expect(rowAt(main, 2)).toHaveAttribute('data-open', 'true');
  await expect(main.getByTestId('chat-panel-tabs')).toHaveCount(0);
  const reports = thread(main).getByTestId('chat-agent-report');
  await expect(reports).toHaveCount(2);
  await expect(reports.nth(1)).toHaveAttribute('data-turn', '2');
  await expect(reports.nth(1).locator('.jtitle')).toHaveText('Agent done · garden-planner');
  // Its work is never in the thread (Chat sent this agent out: it works in its own panel, as a subagent
  // does in Claude Code). Each turn's is in its row there: what it said as it went, its last words too
  // (no report follows them there), and a line for each stretch of steps, which opens into them.
  await expect(thread(main).getByTestId('chat-agent-work')).toHaveCount(0);
  const turnOne = await panelWork(main, 1);
  await expect(turnOne).toHaveAttribute('data-turn', '1');
  await expect(turnOne.getByTestId('chat-agent-work-said')).toHaveText(["I'll add the watering schedule to notes.md.", DONE1]);
  await expect(turnOne.getByTestId('chat-agent-work-steps').locator('.wtext')).toHaveText([/^Edited notes\.md\s*\+1$/]);
  const second = await panelWork(main, 2);
  await expect(second.getByTestId('chat-agent-work-said')).toHaveText(['Now the minimap button.', DONE2]);
  await expect(second.getByTestId('chat-agent-work-steps').locator('.wtext')).toHaveText([/^Edited 2 files\s*\+2$/]);
  await expect(rowsIn(second)).toHaveCount(0);
  await second.getByRole('button', { name: /Edited 2 files/ }).click();
  await expect(rowsIn(second).locator('.rtext')).toHaveText(['Edited notes.md', 'Created minimap.js']);
  await shot(main, '10-its-work-in-its-panel-row');
  await second.getByRole('button', { name: /Edited 2 files/ }).click();
  await expect(rowsIn(second)).toHaveCount(0);
  await expect(reports.nth(1).locator('.jline')).toHaveText('Changed notes.md, minimap.js');
  await closeTab(main);
  await reports.nth(1).getByRole('button', { name: 'Open the agent in garden-planner' }).click();
  await expect(rowAt(main, 2)).toHaveAttribute('data-open', 'true');
  // Its Open: the timeline at that turn.
  await agentTimeline(main, 2);
  await expect(turnAt(main, 2)).toHaveAttribute('data-open', 'true');
  await expect(main.getByTestId('chat-panel-tabs')).toHaveCount(0);
  // The chat's prompt said so: its agent, a follow-up continues it.
  const asked = fakeLlm.requests.filter((r) => r.tools.includes('start_agent') && JSON.stringify(r.messages).includes('as well?'))[0];
  expect(JSON.stringify(asked.messages)).toContain(`This chat's agent there (id ${agent.id}) is done, after 1 turn: the user's next request there continues it.`);
});

test('a note while the agent works goes to it now: said so, read at its next step, the same turn, one report', async ({ yap, main, fakeLlm, shot }) => {
  fs.rmSync(ARGS, { force: true });
  await script(yap, SLOWISH, 'slowish');
  const before = (await agents(yap)).length;
  await codeChat(main);
  fakeLlm.scriptChat([
    { tools: [{ name: 'start_agent', args: { brief: 'Read notes.md.', title: 'Read the notes' } }] },
    { content: STARTED },
  ]);
  await send(main, 'Read notes.md.');
  const agent = await newAgent(yap, before);
  await expect.poll(async () => (await agents(yap)).find((j) => j.id === agent.id).status, { timeout: 20_000 }).toBe('running');
  fakeLlm.scriptChat([
    { tools: [{ name: 'start_agent', args: { brief: 'Keep the headings as they are.', title: 'Keep the headings' } }] },
    { content: NOTED },
    { plain: true, content: 'Done. It read notes.md and kept the headings.' },
  ]);
  await send(main, 'Keep the headings as they are.');
  await expect(chipsOfLast(main)).toHaveText(['Told the agent in garden-planner']);
  // Handed over: the turn working's, never a turn of its own.
  expect(lastTool(fakeLlm)).toMatchObject({ success: true, data: { id: agent.id, turn: 1, say: NOTED } });
  expect(lastTool(fakeLlm).data.note).toContain('it takes them into the work in hand at its next step');
  await expect(tab(main).locator('[data-testid="chat-agent-group"][data-group="queued"]')).toHaveCount(0);
  await expect(tab(main).getByTestId('chat-agent-row')).toHaveCount(1);
  await expect(rowAt(main, 1)).toHaveAttribute('data-state', 'running');
  await expect(rowAt(main, 1).getByTestId('chat-agent-row-line')).toHaveText('Your note is with the agent: it reads it at its next step');
  // …and in its row's work, live: the line it's on (never in the thread: Chat sent this agent out).
  await expect(thread(main).getByTestId('chat-agent-work')).toHaveCount(0);
  const working = await panelWork(main, 1);
  await expect(working).toHaveAttribute('data-live', 'true');
  await expect(working.locator('[data-busy="true"] .wtext')).toHaveText('Your note is with the agent: it reads it at its next step');
  await shot(main, '03-agent-tab-a-note-with-the-agent');
  // Its tool call ends: it reads the note, and answers once for both.
  const done = await ended(yap, agent.id, 40_000);
  expect(done.turnList.map((t) => [t.n, t.status])).toEqual([[1, 'done']]);
  expect(done.turnList[0].request).toBe('Read notes.md.\n\nKeep the headings as they are.');
  expect(done.turnList[0].text).toContain('Read notes.md. Noted: Keep the headings as they are.');
  // One Claude Code process, started so it says what it has read.
  const args = argsSeen();
  expect(args).toHaveLength(1);
  expect(args[0]).toContain('--replay-user-messages');
  expect((await agents(yap)).length).toBe(before + 1);
  // One report, of one turn.
  const messages = await messagesOf(yap, done.conversation, 6);
  expect(messages.map((m) => m.kind || m.role)).toEqual(['user', 'assistant', 'user', 'assistant', 'agentReport', 'assistant']);
  expect(messages[4].text).not.toContain('This report covers');
  expect(messages[4].text).toContain('Its own words: Read notes.md. Noted: Keep the headings as they are.');
  await expect(thread(main).getByTestId('chat-agent-report')).toHaveCount(1);
  await expect(lastAnswer(main)).toHaveText('Done. It read notes.md and kept the headings.');
  // Its tab: the note read, in the turn's steps.
  const itsWork = await panelWork(main, 1);
  await expect(itsWork).toHaveAttribute('data-live', 'false');
  await expect(itsWork.locator('.wtext')).toHaveText(['Read notes.md']);
  await expect(thread(main).getByTestId('chat-agent-work')).toHaveCount(0);
  await agentTimeline(main, 1);
  await expect(turnAt(main, 1)).toContainText('Read your note');
  await shot(main, '06-a-note-read-one-report');
});

test('a note the agent never reads waits as its next turn, and both are told in one report', async ({ yap, main, fakeLlm, shot }) => {
  fs.rmSync(ARGS, { force: true });
  await script(yap, NEVER_READS, 'never-reads');
  const before = (await agents(yap)).length;
  await codeChat(main);
  fakeLlm.scriptChat([
    { tools: [{ name: 'start_agent', args: { brief: 'Read notes.md.', title: 'Read the notes' } }] },
    { content: STARTED },
  ]);
  await send(main, 'Read notes.md.');
  const agent = await newAgent(yap, before);
  await expect.poll(async () => (await agents(yap)).find((j) => j.id === agent.id).status, { timeout: 20_000 }).toBe('running');
  // Its next turn will say what it's told.
  await script(yap, ECHO, 'echo-next');
  fakeLlm.scriptChat([
    { tools: [{ name: 'start_agent', args: { brief: 'Add a legend to the minimap.', title: 'Add a legend' } }] },
    { content: NOTED },
    { plain: true, content: 'Done. It read notes.md, and the legend is in.' },
  ]);
  await send(main, 'Add a legend to the minimap too.');
  await expect(chipsOfLast(main)).toHaveText(['Told the agent in garden-planner']);
  expect(lastTool(fakeLlm)).toMatchObject({ success: true, data: { id: agent.id, turn: 1, say: NOTED } });
  // The turn ends with the note unread: it goes as the next turn, on --resume.
  const done = await (async () => {
    await expect.poll(async () => (await agents(yap)).find((j) => j.id === agent.id).turnList?.[1]?.status, { timeout: 40_000 }).toBe('done');
    return ended(yap, agent.id);
  })();
  expect(done.turnList.map((t) => [t.n, t.status, t.title])).toEqual([
    [1, 'done', 'Read the notes'],
    [2, 'done', 'Add a legend'],
  ]);
  expect(done.turnList[0].request).toBe('Read notes.md.');
  expect(done.turnList[1].text).toContain('Got: Add a legend to the minimap.');
  const args = argsSeen();
  expect(args).toHaveLength(2);
  expect(after(args[1], '--resume')).toBe(done.session);
  expect((await agents(yap)).length).toBe(before + 1);
  // One report once it stops working, for both requests: one card, one answer.
  const reports = () => thread(main).getByTestId('chat-agent-report');
  const messages = await messagesOf(yap, done.conversation, 6);
  expect(messages.map((m) => m.kind || m.role)).toEqual(['user', 'assistant', 'user', 'assistant', 'agentReport', 'assistant']);
  const marker = messages[4].text;
  expect(marker).toContain('This report covers 2 requests it worked through one after the other');
  expect(marker).toContain('Request 1, "Read the notes". Its own words: Read notes.md.');
  expect(marker).toContain('Request 2, "Add a legend". Its own words: Got: Add a legend to the minimap.');
  expect(messages[5].text).toBe('Done. It read notes.md, and the legend is in.');
  // The chat's model was told to answer for both in one message.
  const told = fakeLlm.requests.filter((r) => JSON.stringify(r.messages).includes('This report covers 2 requests')).at(-1);
  expect(JSON.stringify(told.messages)).toContain('This one report covers 2 requests');
  expect(told.tools).toEqual([]);
  // Both turns are that report's: its card reads as the latest, and opens there.
  const after2 = (await agents(yap)).find((j) => j.id === agent.id);
  const markerId = `chat:${done.conversation}:4`;
  expect(after2.turnList.map((t) => [t.reportMessageId, !!t.reportHeld])).toEqual([
    [markerId, false],
    [markerId, false],
  ]);
  await expect(reports()).toHaveCount(1);
  await expect(reports()).toHaveAttribute('data-turn', '2');
  await expect(reports().locator('.jtitle')).toHaveText('Agent done · garden-planner');
  await expect(lastAnswer(main)).toHaveText('Done. It read notes.md, and the legend is in.');
  await shot(main, '07-one-report-for-both-requests');
  await agentTimeline(main, 2);
  await expect(turnAt(main, 2)).toHaveAttribute('data-open', 'true');
  await expect(turnAt(main, 1)).toHaveAttribute('data-open', 'false');
});

test('a question sent just before an agent’s report is answered: its card still shows, and that answer tells what the agent did', async ({ yap, main, fakeLlm, shot }) => {
  // 11 Nov 2026, a Yap Lab run on the real Claude Code: a question sent in
  // the seconds between an agent finishing and its report. The report's own
  // answer gives way to it (as to any newer question) — but its card never
  // showed (the view skipped reading the chat again while its own question
  // was answered), and nobody said what the agent had done.
  fs.rmSync(ARGS, { force: true });
  // (An agent that takes a few seconds: Chat's own answer is in before it ends.)
  await script(yap, SLOWISH, 'slowish-overtaken');
  const before = (await agents(yap)).length;
  await codeChat(main);
  fakeLlm.scriptChat([
    { tools: [{ name: 'start_agent', args: { brief: 'Read notes.md.', title: 'Read the notes' } }] },
    { content: STARTED },
    // The report's own answer: held back (below), and never said.
    { plain: true, content: 'Never said: a question came first.' },
    { content: 'The agent read notes.md and changed nothing. The footer is fine as it is.' },
  ]);
  await send(main, 'Read notes.md.');
  const agent = await newAgent(yap, before);
  await expect(lastAnswer(main)).toHaveText(STARTED);
  // Every answer takes a while from here: the report's is still on its way
  // when the next question is sent.
  fakeLlm.delay('other', 4000);
  try {
    const done = await ended(yap, agent.id, 40_000);
    await expect.poll(async () => (await yap.invoke('chat_get', { id: done.conversation })).messages.some((m) => m.kind === 'agentReport'), { timeout: 20_000, message: 'the report’s marker in the chat' }).toBe(true);
    await send(main, 'And is the footer fine?');
    // Answered: the report's card is in the thread, above the question, and
    // the answer is the question's — which told the report first.
    await expect(lastAnswer(main)).toHaveText('The agent read notes.md and changed nothing. The footer is fine as it is.', { timeout: 30_000 });
    const messages = await messagesOf(yap, done.conversation, 5);
    expect(messages.map((m) => m.kind || m.role)).toEqual(['user', 'assistant', 'agentReport', 'user', 'assistant']);
    const reports = thread(main).getByTestId('chat-agent-report');
    await expect(reports).toHaveCount(1);
    await expect(reports.locator('.jtitle')).toHaveText('Agent done · garden-planner');
    const order = await thread(main).locator('[data-testid="chat-agent-report"], .cbubble.user').evaluateAll((els) => els.map((e) => (e.dataset.testid === 'chat-agent-report' ? 'report' : e.textContent.trim())));
    expect(order).toEqual(['Read notes.md.', 'report', 'And is the footer fine?']);
    const asked = fakeLlm.requests.filter((r) => JSON.stringify(r.messages).includes('And is the footer fine?')).at(-1);
    expect(JSON.stringify(asked.messages)).toContain("The user hasn't been told about it yet");
    expect(JSON.stringify(asked.messages)).not.toContain('Never said');
    // The pill of its turn is under the first answer again (the thread's ids are the saved ones).
    await expect(thread(main).getByTestId('chat-agent-pill')).toHaveCount(1);
    await shot(main, '08-a-question-overtook-the-report');
    // Once answered, the report reads as any other: nothing to tell twice.
    fakeLlm.delay('other', 0);
    fakeLlm.scriptChat([{ content: 'Yes.' }]);
    await send(main, 'Thanks. Anything else?');
    await expect(lastAnswer(main)).toHaveText('Yes.');
    const later = fakeLlm.requests.filter((r) => JSON.stringify(r.messages).includes('Anything else?')).at(-1);
    expect(JSON.stringify(later.messages)).not.toContain("hasn't been told about it yet");
    expect(JSON.stringify(later.messages)).toContain('Report from the agent you started (not from the person): ');
  } finally {
    fakeLlm.delay('other', 0);
  }
});

test('asked how it is getting on just as the agent finishes: that answer tells the result, and the report adds its card alone', async ({ yap, main, fakeLlm, shot }) => {
  // 11 Nov 2026, a Yap Lab run on the real Claude Code: "Stop the agent." as
  // it finished. Chat's model looked (agent_status), its answer told the
  // whole result — and then the report said it all again.
  fs.rmSync(ARGS, { force: true });
  await script(yap, SLOWISH, 'slowish-asked');
  const before = (await agents(yap)).length;
  const reportsAsked = () => fakeLlm.requests.filter((r) => JSON.stringify(r.messages).includes('An agent you sent earlier')).length;
  const askedBefore = reportsAsked();
  await codeChat(main);
  fakeLlm.scriptChat([
    { tools: [{ name: 'start_agent', args: { brief: 'Read notes.md.', title: 'Read the notes' } }] },
    { content: STARTED },
  ]);
  await send(main, 'Read notes.md.');
  const agent = await newAgent(yap, before);
  await expect(lastAnswer(main)).toHaveText(STARTED);
  await expect.poll(async () => (await agents(yap)).find((j) => j.id === agent.id).status, { timeout: 20_000 }).toBe('running');
  // The model takes its time: when it looks, the agent has ended, and its
  // report is waiting for this answer.
  fakeLlm.delay('other', 7000);
  try {
    fakeLlm.scriptChat([{ tools: [{ name: 'agent_status', args: {} }] }, { content: 'It has just finished: it read notes.md and changed nothing.' }]);
    await send(main, 'How is it getting on?');
    await expect(lastAnswer(main)).toHaveText('It has just finished: it read notes.md and changed nothing.', { timeout: 40_000 });
    // It was told the agent had just finished, and to say so itself.
    const looked = lastTool(fakeLlm);
    expect(looked.data.agents[0]).toMatchObject({ id: agent.id, status: 'done', just_finished: true });
    expect(looked.data.note).toContain('Yap shows its card under your answer and says nothing more.');
    // The report: its card, under that answer, and no second answer.
    const done = await ended(yap, agent.id);
    const messages = await messagesOf(yap, done.conversation, 5);
    expect(messages.map((m) => m.kind || m.role)).toEqual(['user', 'assistant', 'user', 'assistant', 'agentReport']);
    expect(messages[4].text.endsWith('(You told the user this in your answer just before it.)')).toBe(true);
    await expect(thread(main).getByTestId('chat-agent-report')).toHaveCount(1);
    await expect(thread(main).getByTestId('chat-agent-report').locator('.jtitle')).toHaveText('Agent done · garden-planner');
    await expect(lastAnswer(main)).toHaveText('It has just finished: it read notes.md and changed nothing.');
    await shot(main, '09-the-answer-told-it-the-card-alone');
    // The next question: nothing more was said meanwhile, the report's own
    // model was never asked, and that card isn't news to tell.
    fakeLlm.delay('other', 0);
    fakeLlm.scriptChat([{ content: 'Yes.' }]);
    await send(main, 'Thanks. All good?');
    await expect(lastAnswer(main)).toHaveText('Yes.');
    const after = await messagesOf(yap, done.conversation, 7);
    expect(after.map((m) => m.kind || m.role)).toEqual(['user', 'assistant', 'user', 'assistant', 'agentReport', 'user', 'assistant']);
    expect(reportsAsked()).toBe(askedBefore);
    const next = fakeLlm.requests.filter((r) => JSON.stringify(r.messages).includes('All good?')).at(-1);
    expect(JSON.stringify(next.messages)).toContain('(You told the user this in your answer just before it.)');
    expect(JSON.stringify(next.messages)).not.toContain("hasn't been told about it yet");
  } finally {
    fakeLlm.delay('other', 0);
  }
});

test('a step that read a picture ends its line with it: small on the line, larger by its step, on its step in the timeline, opened in the picture tab', async ({ yap, main, fakeLlm, shot }) => {
  fs.writeFileSync(path.join(GARDEN, 'beds.png'), PLAN_PNG);
  await script(yap, LOOKS_AT_THE_PLAN, 'plan');
  const before = (await agents(yap)).length;
  await codeChat(main);
  fakeLlm.scriptChat([
    { tools: [{ name: 'start_agent', args: { brief: 'Look at beds.png and say how the beds are laid out.', title: 'Look at the bed plan' } }] },
    { content: STARTED },
    { plain: true, content: 'It looked: four beds, the herbs nearest the door.' },
  ]);
  await send(main, 'Have a look at beds.png and tell me how the beds are laid out.');
  const agent = await ended(yap, (await newAgent(yap, before)).id);
  await messagesOf(yap, agent.conversation, 4);
  await closeTab(main);

  // Its log: the step with its call, then the picture that call came back with.
  const log = (await yap.invoke('agent_get', { id: agent.id })).log;
  const read = log.find((e) => e.step === 'Reading beds.png');
  expect(read.call).toBe('toolu_plan');
  const pictures = log.filter((e) => e.picture);
  expect(pictures.map((e) => [e.call, e.picture.w, e.picture.h, e.turn, e.step ?? null])).toEqual([['toolu_plan', PLAN.w, PLAN.h, 1, null]]);
  // Kept in the agent's own folder (gone with its log), never in chat-images.
  const kept = path.join(yap.dataDir, 'agents', agent.id, 'pictures', `${pictures[0].picture.id}.png`);
  const png = readPng(fs.readFileSync(kept));
  expect([png.width, png.height]).toEqual([PLAN.w, PLAN.h]);
  const chatImages = path.join(yap.dataDir, 'chat-images');
  expect(fs.existsSync(chatImages) ? fs.readdirSync(chatImages).filter((n) => n.includes(pictures[0].picture.id)) : []).toEqual([]);

  // In its row's work (never the thread): one quiet line for the stretch, its picture at its end.
  await expect(thread(main).getByTestId('chat-agent-work')).toHaveCount(0);
  const work = await panelWork(main, 1);
  await expect(work).toHaveCount(1);
  await expect(work.locator('.wtext')).toHaveText(['Ran a command, read a file']);
  const small = work.getByTestId('agent-step-picture');
  await expect(small).toHaveCount(1);
  await expect(small).toHaveAttribute('title', 'Read beds.png: open the picture');
  const loaded = (img) => img.evaluate((el) => ({ w: el.naturalWidth, h: el.naturalHeight, done: el.complete, src: el.getAttribute('src') }));
  await expect.poll(async () => (await loaded(small.locator('img'))).w, { message: 'the small picture to load' }).toBeGreaterThan(0);
  const ratio = await main.evaluate(() => window.devicePixelRatio);
  const tile = await loaded(small.locator('img'));
  // A tile filled with its middle, Rust's at exactly its device pixels; by the agent's id and the picture's, nothing else.
  expect([tile.w, tile.h]).toEqual([Math.round(30 * ratio), Math.round(20 * ratio)]);
  expect(decodeURIComponent(tile.src)).toContain(`/agent/${agent.id}/${pictures[0].picture.id}?w=${tile.w}&h=${tile.h}&fit=cover`);
  const box = await small.boundingBox();
  expect([Math.round(box.width), Math.round(box.height)]).toEqual([30, 20]);
  // On the line, after its words.
  const line = await work.locator('.wline').boundingBox();
  expect(box.x).toBeGreaterThan(line.x + line.width - 1);
  expect(Math.abs(box.y + box.height / 2 - (line.y + line.height / 2))).toBeLessThan(3);
  await shot(main, '20-a-picture-at-the-end-of-its-line');
  await theme(yap, main, 'dark');
  await shot(main, '21-a-picture-at-the-end-of-its-line-dark');
  await theme(yap, main, 'light');

  // The line opened: a row a step, the small picture at the end of the row that read it
  // (the line's own gives way); that row opened: the picture larger.
  await work.getByRole('button', { name: /Ran a command, read a file/ }).click();
  await expect(rowsIn(work).locator('.rtext')).toHaveText(['Listed the folder', 'Read beds.png']);
  await expect(work.locator('.wrow').getByTestId('agent-step-picture')).toHaveCount(0);
  await expect(rowsIn(work).nth(0).getByTestId('agent-step-picture')).toHaveCount(0);
  await expect(rowsIn(work).nth(1).getByTestId('agent-step-picture')).toHaveCount(1);
  await rowsIn(work).nth(1).locator('button.rhead').click();
  const byStep = rowsIn(work).nth(1).getByTestId('chat-agent-work-step-picture');
  await expect(byStep).toHaveCount(1);
  await expect(rowsIn(work).nth(1).getByTestId('agent-step-picture')).toHaveCount(0);
  await expect.poll(async () => (await loaded(byStep.locator('img'))).w, { message: 'the step’s picture to load' }).toBeGreaterThan(0);
  const frame = await byStep.boundingBox();
  expect(frame.width).toBeLessThanOrEqual(281);
  expect(frame.height).toBeLessThanOrEqual(181);
  expect(Math.abs(frame.width / frame.height - PLAN.w / PLAN.h)).toBeLessThan(0.03);
  await shot(main, '23-by-its-step-once-opened');
  await work.getByRole('button', { name: /Ran a command, read a file/ }).click();
  await expect(rowsIn(work)).toHaveCount(0);
  await expect(work.getByTestId('agent-step-picture')).toHaveCount(1);

  // Copy image takes the same address (ids only).
  await yap.invoke('chat_image_copy', { id: `agent/${agent.id}/${pictures[0].picture.id}` });
  await expect(yap.invoke('chat_image_copy', { id: `agent/${agent.id}/../log.jsonl` })).rejects.toThrow(/isn't there any more/);

  // In its timeline: on its step.
  await agentTimeline(main, 1);
  const inTab = tab(main).getByTestId('agent-step-picture');
  await expect(inTab).toHaveCount(1);
  await expect.poll(async () => (await loaded(inTab.locator('img'))).w).toBeGreaterThan(0);
  await shot(main, '24-on-its-step-in-the-tab');
  // A click on the one in its row's work opens the panel's picture tab, captioned with its step.
  await (await panelWork(main, 1)).getByTestId('agent-step-picture').click();
  await expect(main.getByTestId('chat-picture-stage')).toBeVisible();
  await expect(main.getByTestId('chat-picture-caption')).toHaveText('Read beds.png');
  await expect(main.getByTestId('chat-picture-source')).toContainText('garden-planner');
  await shot(main, '25-in-the-picture-tab');
});

test('another tool’s call is a line of the agent’s work: one alone as itself, several counted, each with its icon in the timeline', async ({ yap, main, fakeLlm, shot }) => {
  await script(yap, USES_OTHER_TOOLS, 'tools');
  const before = (await agents(yap)).length;
  await codeChat(main);
  fakeLlm.scriptChat([
    { tools: [{ name: 'start_agent', args: { brief: 'See which lights are on, turn the shed light on, and say when high tide is.', title: 'Check the lights and the tide' } }] },
    { content: STARTED },
    { plain: true, content: 'Both lights are on, and high tide is at 14:05.' },
  ]);
  await send(main, 'See which lights are on, turn the shed light on, and tell me when high tide is.');
  const agent = await ended(yap, (await newAgent(yap, before)).id);
  await messagesOf(yap, agent.conversation, 4);

  // In its row's work: the lone call is a line of its own (it used to show nothing), then its words, then the
  // stretch of three as one line, then its last words.
  const work = await panelWork(main, 1);
  await expect(work).toHaveCount(1);
  await expect(work.locator('.wtext')).toHaveText(['Used homelights · list lights', 'Ran a command, used 2 tools']);
  await expect(work.getByTestId('chat-agent-work-said')).toHaveText(['The porch light is on. Turning the shed light on too.', LIT]);
  await shot(main, '30-other-tools-calls-as-lines');
  await theme(yap, main, 'dark');
  await shot(main, '31-other-tools-calls-as-lines-dark');
  await theme(yap, main, 'light');
  await work.getByRole('button', { name: /Ran a command, used 2 tools/ }).click();
  await expect(rowsIn(work).locator('.rtext')).toHaveText(['Used homelights · set light', 'Used tides · next high tide', 'Listed the folder']);
  await shot(main, '32-the-stretch-opened');

  // In its timeline: each on its step, with a tool's icon.
  await agentTimeline(main, 1);
  await expect(tab(main).locator('.step.k-tool .stext')).toHaveText(['Using homelights · list lights', 'Using homelights · set light', 'Using tides · next high tide']);
  await expect(tab(main).locator('.step.k-tool .sicon svg')).toHaveCount(3);
  await expect(tab(main).locator('.step.k-command .stext')).toHaveText(['Listing the folder']);
});

test('a stretch opened is a row a step: an edit opens to its numbered diff, a command to itself and what it printed', async ({ yap, main, fakeLlm, shot }) => {
  await script(yap, FOUR_STEPS, 'rows');
  const before = (await agents(yap)).length;
  await codeChat(main);
  fakeLlm.scriptChat([
    { tools: [{ name: 'start_agent', args: { brief: 'Add a watering line to notes.md, run the tests and write a legend.', title: 'Watering line and a legend' } }] },
    { content: STARTED },
    { plain: true, content: 'It added the watering line and a legend; one test still fails.' },
  ]);
  await send(main, 'Add a watering line to notes.md, run the tests and write a legend.');
  const agent = await ended(yap, (await newAgent(yap, before)).id);
  await messagesOf(yap, agent.conversation, 4);
  await closeTab(main);

  // Its log: the edit's diff with its place in the file and its call, the command as done, what it printed.
  const log = (await yap.invoke('agent_get', { id: agent.id })).log;
  const edited = log.find((e) => e.diff?.file === 'notes.md');
  expect([edited.call, edited.diff.lines]).toEqual(['toolu_e', ['@@ -1,1 +1,2 @@', ' Draft notes', '+Watering: twice a week']]);
  expect(log.find((e) => e.command === 'npm test')).toMatchObject({ step: 'Running the tests', done: 'Ran the tests', call: 'toolu_t' });
  expect(log.find((e) => e.output)).toMatchObject({ call: 'toolu_t', output: { text: TEST_OUTPUT, failed: true } });
  expect(log.find((e) => e.step === 'Reading notes.md').range).toBe('1–20');

  // The stretch's line, then its rows: each step as done, a file's name set apart, its lines added.
  // (In its row of the Agent panel: Chat sent this agent out.)
  await expect(thread(main).getByTestId('chat-agent-work')).toHaveCount(0);
  const work = await panelWork(main, 1);
  await expect(work.locator('.wtext')).toHaveText([/^Ran a command, read a file, edited 3 files\s*\+9\s*−1, 1 failed$/]);
  await work.getByRole('button', { name: /Ran a command, read a file/ }).click();
  const rows = rowsIn(work);
  await expect(rows.locator('.rtext')).toHaveText(['Read notes.md (1–20)', 'Edited notes.md', 'Ran the tests', 'Created water.js', 'Created legend.md', 'Edited water.js']);
  await expect(rows.locator('.rname')).toHaveText(['notes.md', 'notes.md', 'water.js', 'legend.md', 'water.js']);
  await expect(rows.nth(1).locator('.plus')).toHaveText('+1');
  await expect(rows.nth(4).locator('.plus')).toHaveText('+3');
  await expect(rows.nth(2).locator('.rfailed')).toHaveText('failed');
  // A row with nothing more to show doesn't open; the others do.
  await expect(rows.nth(0).getByRole('button')).toHaveCount(0);
  await expect(rows.locator('button.rhead')).toHaveCount(5);
  // The highlighter isn't loaded until a row is opened.
  await expect(work.getByTestId('chat-agent-work-rows')).toHaveAttribute('data-colours', 'false');
  // No command or diff until its row is opened.
  await expect(work.getByTestId('chat-agent-work-command')).toHaveCount(0);
  await expect(work.getByTestId('chat-agent-work-diff')).toHaveCount(0);
  await shot(main, '40-a-row-a-step');

  // The edit: its path, and its lines with their numbers in the file.
  await rows.nth(1).getByRole('button', { name: /Edited notes\.md/ }).click();
  await expect(rows.nth(1)).toHaveAttribute('data-open', 'true');
  await expect(rows.nth(1).locator('.rpath')).toHaveText('notes.md');
  const lines = rows.nth(1).getByTestId('chat-agent-work-diff').locator('.dl');
  await expect(lines).toHaveCount(2);
  expect(await lines.evaluateAll((els) => els.map((e) => [['ctx', 'add', 'del'].find((m) => e.classList.contains(m)), e.querySelector('.dno').textContent, e.querySelector('.dt').textContent]))).toEqual([
    ['ctx', '1', 'Draft notes'],
    ['add', '2', 'Watering: twice a week'],
  ]);
  // The command: itself, and what it printed.
  await rows.nth(2).getByRole('button', { name: /Ran the tests/ }).click();
  await expect(rows.nth(2).getByTestId('chat-agent-work-command').locator('.ccmd')).toHaveText('$ npm test');
  const printed = rows.nth(2).getByTestId('chat-agent-work-output');
  await expect(printed).toContainText('✖ watering is twice a week (0.4ms)');
  await expect(printed).toHaveClass(/failed/);
  expect(await printed.locator('pre').first().evaluate((el) => el.textContent)).toBe(TEST_OUTPUT);
  await expect(rows.nth(2).getByRole('button', { name: 'Copy the command' })).toBeVisible();
  // The new file: every line added, numbered from 1.
  // Its code in colour: the highlighter loaded with the first row opened; a script's diff by its language,
  // each token a text node in a span (never markup), the line's own text unchanged.
  await expect(work.getByTestId('chat-agent-work-rows')).toHaveAttribute('data-colours', 'true');
  await rows.nth(3).getByRole('button', { name: /Created water\.js/ }).click();
  const jsDiff = rows.nth(3).getByTestId('chat-agent-work-diff');
  await expect(jsDiff.locator('.dl')).toHaveCount(4);
  await expect(jsDiff.locator('.syn-k')).toHaveText(['export', 'function', 'return']);
  await expect(jsDiff.locator('.syn-c')).toHaveText(['// How much a bed drinks.']);
  await expect(jsDiff.locator('.syn-f')).toHaveText(['water']);
  await expect(jsDiff.locator('.syn-n')).toHaveText(['2']);
  expect(await jsDiff.locator('.dl .dt').evaluateAll((els) => els.map((e) => e.textContent))).toEqual(WATER_JS.trimEnd().split('\n'));
  expect(await jsDiff.locator('.dt *').evaluateAll((els) => els.every((e) => e.tagName === 'SPAN' && e.children.length === 0))).toBe(true);
  // The colours are Yap's own, and not the text's: a keyword's, a comment's and plain text's all differ.
  const colourOf = (loc) => loc.first().evaluate((el) => getComputedStyle(el).color);
  const light = { keyword: await colourOf(jsDiff.locator('.syn-k')), comment: await colourOf(jsDiff.locator('.syn-c')), plain: await colourOf(jsDiff.locator('.dl .dt')) };
  expect(new Set(Object.values(light)).size).toBe(3);
  // The command as a shell's; what it printed stays plain; a Markdown file's heading too.
  await expect(rows.nth(2).getByTestId('chat-agent-work-command')).toHaveAttribute('data-coloured', 'true');
  await expect(rows.nth(2).getByTestId('chat-agent-work-command').locator('.ccmd')).toHaveText('$ npm test');
  await expect(rows.nth(2).getByTestId('chat-agent-work-output').locator('[class*="syn-"]')).toHaveCount(0);
  await rows.nth(4).getByRole('button', { name: /Created legend\.md/ }).click();
  await expect(rows.nth(4).getByTestId('chat-agent-work-diff').locator('.syn-f')).toHaveText(['# Legend']);
  const made = rows.nth(4).getByTestId('chat-agent-work-diff').locator('.dl');
  expect(await made.evaluateAll((els) => els.map((e) => [e.querySelector('.dno').textContent, e.querySelector('.dt').textContent.trim()]))).toEqual([
    ['1', '# Legend'],
    ['2', ''],
    ['3', 'B: a bed'],
  ]);
  // A line changed rather than taken out: the part that was replaced is marked on the removed line and what
  // replaced it on the added one, stronger than the line's own tint, over the code's own colours.
  await rows.nth(5).getByRole('button', { name: /Edited water\.js/ }).click();
  const changed = rows.nth(5).getByTestId('chat-agent-work-diff');
  await expect(changed.locator('.dl')).toHaveCount(4);
  await expect(changed.locator('.dl.del .chg')).toHaveText(['2']);
  await expect(changed.locator('.dl.add .chg')).toHaveText(['3']);
  await expect(changed.locator('.dl.ctx .chg')).toHaveCount(0);
  expect(await changed.locator('.dl .dt').evaluateAll((els) => els.map((e) => e.textContent))).toEqual(['export function water(bed) {', '  return bed.litres * 2;', '  return bed.litres * 3;', '}']);
  expect(await changed.locator('.dl .dno').evaluateAll((els) => els.map((e) => e.textContent))).toEqual(['2', '3', '3', '4']);
  const tint = (loc) => loc.first().evaluate((el) => getComputedStyle(el).backgroundColor);
  expect(await tint(changed.locator('.dl.del .chg'))).not.toBe(await tint(changed.locator('.dl.del')));
  expect(await tint(changed.locator('.dl.add .chg'))).not.toBe(await tint(changed.locator('.dl.add')));
  expect(await tint(changed.locator('.dl.del .chg'))).not.toBe(await tint(changed.locator('.dl.add .chg')));
  await expect(changed.locator('.dl.add .chg')).toHaveClass(/syn-n/);
  await shot(main, '41-rows-opened');
  await theme(yap, main, 'dark');
  // The dark theme has its own colours for the same tokens.
  expect(await colourOf(jsDiff.locator('.syn-k'))).not.toBe(light.keyword);
  await shot(main, '42-rows-opened-dark');
  await theme(yap, main, 'light');
  // A long line would wrap under itself: the diff's box never scrolls sideways.
  expect(await jsDiff.locator('.dl').first().evaluate((el) => getComputedStyle(el).whiteSpace)).toBe('pre-wrap');
  expect(await jsDiff.evaluate((el) => el.scrollWidth <= el.clientWidth + 1)).toBe(true);
  // Scrolled up to a row, its box stays there (it never jumps back to its end under the pointer).
  await scrollStill(work);
  const head = rows.nth(2).locator('button.rhead');
  await head.scrollIntoViewIfNeeded();
  await main.waitForTimeout(500);
  expect((await head.boundingBox()).y).toBeGreaterThanOrEqual(0);
  // A row closes again; the stretch closed takes its rows with it.
  await head.click();
  await expect(rows.nth(2).getByTestId('chat-agent-work-command')).toHaveCount(0);
  await work.getByRole('button', { name: /Ran a command, read a file/ }).click();
  await expect(rowsIn(work)).toHaveCount(0);
  // The timeline's Files changed still reads the same diff (its place line is no line of its own there).
  await agentTimeline(main, 1);
  await tab(main).getByTestId('chat-agent-files').getByRole('button', { name: /notes\.md/ }).click();
  await expect(tab(main).getByTestId('chat-agent-diff').first().locator('.dl')).toHaveText([/Draft notes/, /Watering: twice a week/]);
});

test('a part handed to a helper: the helper’s steps fold behind its row, never among the session’s own', async ({ yap, main, fakeLlm, shot }) => {
  await script(yap, HANDS_PART_OVER, 'helper');
  const before = (await agents(yap)).length;
  await codeChat(main);
  fakeLlm.scriptChat([
    { tools: [{ name: 'start_agent', args: { brief: 'Check the notes and list the folder.', title: 'Check the notes' } }] },
    { content: STARTED },
    { plain: true, content: 'The notes are in order.' },
  ]);
  await send(main, 'Check the notes and list the folder.');
  const started = await newAgent(yap, before);

  // **In the Agent panel, the helper is an entry of its own** under the turn's row, as Claude Code's own
  // app lists a session's subagents: what it was asked, and the step it's on while it works.
  await thread(main).getByTestId('chat-agent-pill').first().click();
  await expect(tab(main)).toBeVisible();
  const entry = rowAt(main, 1).getByTestId('chat-agent-helper');
  await expect(entry).toHaveCount(1, { timeout: 20_000 });
  await expect(entry).toHaveAttribute('data-status', 'working');
  await expect(entry.getByTestId('chat-agent-helper-title')).toHaveText('Check the notes');
  await expect(entry.getByTestId('chat-agent-helper-line')).toHaveText('Reading notes.md');
  await expect(entry.locator('.hspin')).toHaveCount(1);
  await shot(main, '49-a-helper-at-work-in-the-panel');
  const agent = await ended(yap, started.id);
  await messagesOf(yap, agent.conversation, 4);
  // Back: done, with how many steps it took; it opens to them, an edit of its to its diff.
  await expect(entry).toHaveAttribute('data-status', 'done');
  await expect(entry.getByTestId('chat-agent-helper-line')).toHaveText('2 steps');
  await expect(entry.locator('.hspin')).toHaveCount(0);
  await expect(entry.getByTestId('chat-agent-helper-steps')).toHaveCount(0);
  await entry.locator('button.hhead').click();
  const own = entry.getByTestId('chat-agent-helper-steps');
  await expect(own.getByTestId('chat-agent-work-row').locator('.rtext')).toHaveText(['Read notes.md', 'Edited notes.md']);
  await own.getByRole('button', { name: /Edited notes\.md/ }).click();
  await expect(own.getByTestId('chat-agent-work-diff').locator('.dl.add .dt')).toHaveText(['Checked']);
  await shot(main, '49b-a-helper-done-opened');
  await entry.locator('button.hhead').click();
  await expect(entry.getByTestId('chat-agent-helper-steps')).toHaveCount(0);
  expect((await yap.invoke('agent_get', { id: agent.id })).log.filter((e) => e.helper).map((e) => [e.helper, e.call])).toEqual([['back', 'toolu_task']]);

  // Its log: the helper's own steps say the call they were made under; the session's own don't.
  const log = (await yap.invoke('agent_get', { id: agent.id })).log;
  expect(log.filter((e) => e.step && e.call).map((e) => [e.step, e.under ?? null])).toEqual([
    ['Handing part of it to a helper · Check the notes', null],
    ['Reading notes.md', 'toolu_task'],
    ['Editing notes.md', 'toolu_task'],
    ['Listing the folder', null],
  ]);

  // Its work: the stretch's line counts the session's own steps (the helper's edit isn't its own), and its
  // rows are the hand-over and the command — the helper's steps aren't among them.
  const work = await panelWork(main, 1);
  await expect(work.locator('.wtext')).toHaveText(['Ran a command, handed part of it to a helper']);
  await work.getByRole('button', { name: /Ran a command, handed part of it to a helper/ }).click();
  const rows = work.getByTestId('chat-agent-work-rows').first().locator('> li');
  await expect(rows.locator('> .rline .rtext')).toHaveText(['Handed part of it to a helper · Check the notes', 'Listed the folder']);
  await expect(work.getByTestId('chat-agent-work-helper')).toHaveCount(0);
  await shot(main, '50-a-helper-folded');
  // The hand-over's row opens to the helper's own steps, rows of their own: an edit of its still opens to its diff.
  await rows.nth(0).locator('> .rline button.rhead').click();
  const helper = rows.nth(0).getByTestId('chat-agent-work-helper');
  await expect(helper.getByTestId('chat-agent-work-row').locator('.rtext')).toHaveText(['Read notes.md', 'Edited notes.md']);
  await helper.getByRole('button', { name: /Edited notes\.md/ }).click();
  await expect(helper.getByTestId('chat-agent-work-diff').locator('.dl.add .dt')).toHaveText(['Checked']);
  await shot(main, '51-the-helpers-own-steps');
  await theme(yap, main, 'dark');
  await shot(main, '52-the-helpers-own-steps-dark');
  await theme(yap, main, 'light');
  // The file it changed is still the agent's (its report, Undo).
  expect((await agents(yap)).find((j) => j.id === agent.id).files).toEqual(['notes.md']);
  // Its timeline lists every step in order, the helper's own in from the edge.
  await agentTimeline(main, 1);
  await expect(tab(main).locator('.step .stext')).toContainText(['Handing part of it to a helper · Check the notes', 'Reading notes.md', 'Editing notes.md', 'Listing the folder']);
  await expect(tab(main).locator('.step[data-helper="true"] .stext')).toContainText(['Reading notes.md', 'Editing notes.md']);
  await expect(tab(main).locator('.step[data-helper="true"]')).toHaveCount(2);
});

test('“start fresh”: a second agent in the workspace, its tab named apart from the first', async ({ yap, main, fakeLlm }) => {
  const { agent, conversation } = await twoTurns(yap, main, fakeLlm);
  const before = (await agents(yap)).length;
  await script(yap, ECHO, 'echo-fresh');
  fakeLlm.scriptChat([
    { tools: [{ name: 'start_agent', args: { brief: 'Start over on the planner: list what it has.', title: 'Start over on the planner', fresh: true } }] },
    { content: STARTED },
    { plain: true, content: 'It listed what the planner has.' },
  ]);
  await send(main, 'Start a new agent: start over on the planner and list what it has.');
  await expect(chipsOfLast(main)).toHaveText(['Sent an agent to garden-planner']);
  const fresh = await ended(yap, (await newAgent(yap, before)).id);
  expect(fresh.id).not.toBe(agent.id);
  expect(fresh.session).not.toBe(agent.session);
  expect(fresh).toMatchObject({ conversation, folder: 'garden-planner', turns: 1 });
  expect(after(argsSeen().at(-1), '--session-id')).toBe(fresh.session);
  await messagesOf(yap, conversation, 12);
  // Both agents' tabs in the row, never two that read the same.
  await thread(main).getByTestId('chat-agent-pill').first().click();
  // (Once the agents list has the fresh agent's turns: a moment after its tab opens.)
  const names = () => main.getByTestId('chat-panel-tab').evaluateAll((els) => els.filter((e) => e.dataset.key?.startsWith('agent:')).map((e) => e.getAttribute('title')));
  await expect.poll(names).toEqual(['Agent · garden-planner · Add a watering schedule', 'Agent · garden-planner · Start over on the planner']);
  // The fresh agent is the chat's agent from now on: its newest in the workspace.
  await closeTab(main);
  await script(yap, ECHO, 'echo-after-fresh');
  fakeLlm.scriptChat([
    { tools: [{ name: 'start_agent', args: { brief: 'Also list the seeds.', title: 'List the seeds' } }] },
    { content: PASSED },
    { plain: true, content: 'It listed the seeds.' },
  ]);
  await send(main, 'Also list the seeds.');
  await expect(chipsOfLast(main)).toHaveText(['Continued the agent in garden-planner']);
  await expect.poll(async () => (await agents(yap)).find((j) => j.id === fresh.id).turnList?.length, { timeout: 20_000 }).toBe(2);
  expect((await agents(yap)).length).toBe(before + 1);
  await ended(yap, fresh.id);
  await messagesOf(yap, conversation, 16);
});

test('Undo this turn: refused while a later turn changed the same file, then each turn back; Undo all, asked first', async ({ yap, main, fakeLlm, shot }) => {
  const { agent } = await twoTurns(yap, main, fakeLlm);
  await expect(tab(main)).toBeVisible();
  await agentTimeline(main);
  // The earlier turn first: refused, and why.
  await turnAt(main, 1).getByTestId('chat-agent-turn-head').click();
  await turnAt(main, 1).getByTestId('chat-agent-undo-turn').click();
  await expect(turnAt(main, 1).getByTestId('chat-agent-turn-refused')).toHaveText('Undo the later change first: “Add a minimap button” changed notes.md too.');
  expect(read('notes.md')).toBe(NOTES2);
  await shot(main, '04-undo-the-later-change-first');
  // The later turn: back to before it.
  await turnAt(main, 2).getByTestId('chat-agent-undo-turn').click();
  await expect(turnAt(main, 2).getByTestId('chat-agent-undone')).toHaveText('Undone: notes.md put back; minimap.js deleted (the agent made it).');
  expect(read('notes.md')).toBe(NOTES1);
  expect(fs.existsSync(path.join(GARDEN, 'minimap.js'))).toBe(false);
  await expect(turnAt(main, 2).getByTestId('chat-agent-undo-turn')).toHaveCount(0);
  // Now the earlier one goes.
  await turnAt(main, 1).getByTestId('chat-agent-undo-turn').click();
  await expect(turnAt(main, 1).getByTestId('chat-agent-undone')).toHaveText('Undone: notes.md put back.');
  expect(read('notes.md')).toBe(NOTES0);
  await expect.poll(async () => (await agents(yap)).find((j) => j.id === agent.id)).toMatchObject({ undone: true });
  const undone = (await agents(yap)).find((j) => j.id === agent.id);
  expect(undone.turnList.map((t) => t.undone)).toEqual([true, true]);
  await expect(tab(main).getByRole('button', { name: 'Undo all' })).toHaveCount(0);

  // Undo all: asked first, then back to before the first turn in one go.
  freshFolder(GARDEN);
  const again = await twoTurns(yap, main, fakeLlm);
  expect(read('notes.md')).toBe(NOTES2);
  await tab(main).getByRole('button', { name: 'Undo all' }).click();
  const confirm = main.getByRole('alertdialog', { name: 'Undo every turn of this agent?' });
  await expect(confirm).toBeVisible();
  await shot(main, '05-undo-all-asked-first');
  await confirm.getByRole('button', { name: 'Undo all' }).click();
  await expect(main.getByTestId('chat-agent-undone-all')).toHaveText('Undone: notes.md put back; minimap.js deleted (the agent made it).');
  expect(read('notes.md')).toBe(NOTES0);
  expect(fs.existsSync(path.join(GARDEN, 'minimap.js'))).toBe(false);
  await expect.poll(async () => (await agents(yap)).find((j) => j.id === again.agent.id)).toMatchObject({ undone: true });
});
