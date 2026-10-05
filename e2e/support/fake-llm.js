// A stand-in for an OpenAI-compatible chat server (Groq, OpenAI, a local
// llamafile…): deterministic replies, and a record of every request so a
// spec can check what Yap sent. Point a test instance's Note Formatting
// scope at `base` (see meetings.spec.js).
//
// It knows Yap's two meeting calls by their system prompts
// (src-tauri/src/llm.rs): a rolling DIGEST of one part of a meeting, and the
// final ACTION PLAN. Replies are built from the trigger phrases the specs
// put in their synthetic meetings — and, to prove Yap's checks
// (meeting_summary.rs), the digests slip in a task for someone nobody
// mentioned ("Zed", due "Tuesday"), and the action plan invents an owner
// ("Mallory") and drops one real task.
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
 * system, user }` per chat call (`kind`: "digest" | "actionPlan" | "other").
 */
export async function startFakeLlm() {
  const requests = [];
  const server = http.createServer((req, res) => {
    let raw = '';
    req.on('data', (c) => (raw += c));
    req.on('end', () => {
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
      const kind = system.startsWith(DIGEST)
        ? 'digest'
        : system.startsWith(ACTION_PLAN)
          ? 'actionPlan'
          : 'other';
      requests.push({
        kind,
        model: body.model,
        maxTokens: body.max_tokens ?? null,
        tokens: messages.reduce((n, m) => n + estimateTokens(String(m.content ?? '')), 0),
        system,
        user,
      });
      const content =
        kind === 'digest'
          ? digestReply(user)
          : kind === 'actionPlan'
            ? actionPlanReply(user)
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
    close: () => new Promise((resolve) => server.close(resolve)),
  };
}
