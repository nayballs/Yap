// A stand-in for an OpenAI-compatible chat server (Groq, OpenAI, a local
// llamafile…): deterministic replies, and a record of every request so a
// spec can check what Yap sent. Point a test instance's Note Formatting
// scope at `base` (see meetings.spec.js).
//
// It knows Yap's meeting calls by their system prompts (src-tauri/src/llm.rs):
// a rolling DIGEST of one part of a meeting, the final ACTION PLAN, and the
// meeting notepad's helpers: the meeting TITLE, "What did I miss?" (CATCH UP)
// and a follow-up question (MEETING ASK). Replies are built from the trigger
// phrases the specs put in their synthetic meetings — and, to prove Yap's
// checks (meeting_summary.rs), the digests slip in a task for someone nobody
// mentioned ("Zed", due "Tuesday"), and the action plan invents an owner
// ("Mallory") and drops one real task. A spec can make the next calls of a
// kind fail (`failNext`) or answer slowly (`delay`), to see Yap's progress
// and error states.
import http from 'node:http';

/** Yap's own token estimate (meeting_summary::estimate_tokens). */
export function estimateTokens(text) {
  let ascii = 0;
  let other = 0;
  for (const ch of text) {
    if (ch.charCodeAt(0) < 128) ascii += 1;
    else other += 1;
  }
  return Math.ceil((ascii * 2) / 7) + other;
}

const DIGEST = 'You are a meeting note-taker';
const ACTION_PLAN = 'You are a meeting assistant';
const TITLE = 'You name meetings';
const CATCH_UP = 'You help someone catch up on a meeting';
const MEETING_ASK = 'You answer questions about a meeting';

const KINDS = [
  [DIGEST, 'digest'],
  [ACTION_PLAN, 'actionPlan'],
  [TITLE, 'title'],
  [CATCH_UP, 'catchUp'],
  [MEETING_ASK, 'meetingAsk'],
];

/** The meeting's title, from what was said. */
function titleReply(user) {
  const said = user.split('Transcript (start):')[1] ?? user;
  return /budget/i.test(said) ? 'Q3 Budget Review with Alice' : 'Team Sync';
}

/** "What did I miss?": from the lines said since the person last looked. */
function catchUpReply(user) {
  const since = user.split('Since they last looked')[1] ?? '';
  const lines = since.split('\n').filter((l) => /^(You|Them): /.test(l));
  const first = (lines[0] ?? '').replace(/^(You|Them): /, '').split(' ').slice(0, 5).join(' ');
  const out = [`- ${lines.length} new ${lines.length === 1 ? 'line' : 'lines'} since you looked, opening with "${first}".`];
  if (lines.some((l) => /send the slides/i.test(l))) out.push('- Them asked You to send the slides by Friday.');
  return out.join('\n');
}

/** A follow-up question about the meeting. */
function meetingAskReply(user) {
  const q = /Question: (.*)$/m.exec(user)?.[1] ?? '';
  return /budget/i.test(q) ? 'Alice owns the budget: she said she would send it by Friday.' : 'That isn\'t in the meeting so far.';
}

// Phrase in the transcript → the digest's action item for it.
const TRIGGERS = [
  [/send the revised budget/i, 'Alice: send the revised budget (due: Friday)'],
  [/book the venue/i, 'Bob: book the venue (due: Monday)'],
  [/update the wiki/i, 'Unassigned: update the wiki'],
  [/draft the agenda/i, 'You: draft the agenda'],
];

function digestReply(user) {
  const part = /Transcript, part (\d+)/.exec(user)?.[1] ?? '?';
  const body = user.split(/Transcript, part \d+ \([^)]*\):\n/)[1] ?? '';
  const lines = body.split('\n').filter((l) => /^(You|Them): /.test(l));
  const first = (lines[0] ?? '')
    .replace(/^(You|Them): (SEG-\d+ )?/, '')
    .split(' ')
    .slice(0, 6)
    .join(' ');
  const actions = TRIGGERS.filter(([re]) => re.test(body)).map(([, item]) => `- [ ] ${item}`);
  if (part === '2') actions.push('- [ ] Zed: order pizza (due: Tuesday)');
  const decisions = lines
    .filter((l) => /we decided/i.test(l))
    .map((l) => `- ${l.replace(/^.*we decided (to )?/i, '').replace(/\.$/, '')}`);
  return [
    '### Key points',
    `- Part ${part} covered ${lines.length} lines, opening with "${first}".`,
    '### Decisions',
    ...(decisions.length ? decisions : ['- None']),
    '### Action items',
    ...(actions.length ? actions : ['- None']),
    '### Open questions',
    lines.some((l) => /catering\?/i.test(l)) ? '- Who handles catering?' : '- None',
  ].join('\n');
}

function actionPlanReply(user) {
  // Every task the input gives (digest items, and any in the raw tail).
  const items = [...user.matchAll(/^- \[ \] ([^:\n]+): (.+)$/gm)].map((m) => ({
    owner: m[1].trim(),
    task: m[2].trim(),
  }));
  for (const [re, item] of TRIGGERS) {
    const [owner, task] = item.split(': ');
    const inTail = (user.split('## Transcript of the last part')[1] ?? '').match(re);
    if (inTail && !items.some((i) => i.task === task)) items.push({ owner, task });
  }
  const byOwner = new Map();
  for (const { owner, task } of items) {
    if (owner === 'Bob') continue; // dropped: Yap must put it back
    const list = byOwner.get(owner) ?? [];
    if (!list.includes(task)) list.push(task);
    byOwner.set(owner, list);
  }
  const unassigned = byOwner.get('Unassigned') ?? [];
  byOwner.delete('Unassigned');
  const out = ['The team planned the Q3 offsite: budget, venue and agenda.', '', '## Action plan'];
  for (const [owner, tasks] of byOwner) {
    out.push('', `### ${owner}`, ...tasks.map((t) => `- [ ] ${t}`));
  }
  out.push('', '### Mallory', '- [ ] Print the badges (due: Thursday)'); // invented
  out.push('', '## Decisions', '- Hold the offsite in Lisbon');
  out.push('', '## Open questions', '- Who handles catering?');
  if (unassigned.length) out.push('', '## Unassigned', ...unassigned.map((t) => `- [ ] ${t}`));
  return out.join('\n');
}

/**
 * Start the server. `requests` collects `{ kind, model, maxTokens, tokens,
 * system, user }` per chat call (`kind`: "digest" | "actionPlan" | "title" |
 * "catchUp" | "meetingAsk" | "other"). `failNext(kind, n)` answers the next
 * `n` calls of a kind with HTTP 500; `delay(kind, ms)` holds that kind's
 * answers back (0 = no delay).
 */
export async function startFakeLlm() {
  const requests = [];
  const failures = new Map();
  const delays = new Map();
  const server = http.createServer((req, res) => {
    let raw = '';
    req.on('data', (c) => (raw += c));
    req.on('end', async () => {
      if (req.method !== 'POST' || !req.url.endsWith('/chat/completions')) {
        res.writeHead(404).end();
        return;
      }
      let body;
      try {
        body = JSON.parse(raw);
      } catch {
        res.writeHead(400).end();
        return;
      }
      const messages = body.messages ?? [];
      const system = messages.find((m) => m.role === 'system')?.content ?? '';
      const user = messages.filter((m) => m.role === 'user').at(-1)?.content ?? '';
      const kind = KINDS.find(([prefix]) => system.startsWith(prefix))?.[1] ?? 'other';
      requests.push({
        kind,
        model: body.model,
        maxTokens: body.max_tokens ?? null,
        tokens: messages.reduce((n, m) => n + estimateTokens(String(m.content ?? '')), 0),
        system,
        user,
      });
      const wait = delays.get(kind) ?? 0;
      if (wait > 0) await new Promise((r) => setTimeout(r, wait));
      const failing = failures.get(kind) ?? 0;
      if (failing > 0) {
        failures.set(kind, failing - 1);
        res.writeHead(500, { 'content-type': 'text/plain' }).end('fake failure: the model is down');
        return;
      }
      const content =
        kind === 'digest'
          ? digestReply(user)
          : kind === 'actionPlan'
            ? actionPlanReply(user)
            : kind === 'title'
              ? titleReply(user)
              : kind === 'catchUp'
                ? catchUpReply(user)
                : kind === 'meetingAsk'
                  ? meetingAskReply(user)
                  : 'OK';
      res.writeHead(200, { 'content-type': 'application/json' });
      res.end(
        JSON.stringify({
          id: `fake-${requests.length}`,
          object: 'chat.completion',
          model: body.model,
          choices: [{ index: 0, message: { role: 'assistant', content }, finish_reason: 'stop' }],
          usage: { total_tokens: 0 },
        })
      );
    });
  });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  return {
    base: `http://127.0.0.1:${server.address().port}/v1`,
    requests,
    failNext: (kind, n = 1) => failures.set(kind, n),
    delay: (kind, ms) => delays.set(kind, ms),
    close: () => new Promise((resolve) => server.close(resolve)),
  };
}
