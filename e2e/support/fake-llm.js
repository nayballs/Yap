// A stand-in for an OpenAI-compatible chat server (Groq, OpenAI, a local
// llamafile…): deterministic replies, and a record of every request so a
// spec can check what Yap sent. Point a test instance's Note Formatting
// scope at `base` (see meetings.spec.js).
//
// It knows Yap's meeting calls by their system prompts (src-tauri/src/llm.rs):
// a rolling DIGEST of one part of a meeting, the final ACTION PLAN, and the
// meeting notepad's helpers: the meeting TITLE, "What did I miss?" (CATCH UP),
// a follow-up question (MEETING ASK) and the one-tap chips (QUESTIONS to ask,
// the FOLLOW-UP email — in markdown, with a preamble, for Yap to clean up —
// and BLIND SPOTS, "Gaps and risks"). Replies are built from the trigger
// phrases the specs put in their synthetic meetings — and, to prove Yap's
// checks (meeting_summary.rs), the digests slip in a task for someone nobody
// mentioned ("Zed", due "Tuesday"), and the action plan invents an owner
// ("Mallory") and drops one real task. A spec can make the next calls of a
// kind fail (`failNext`) or answer slowly (`delay`), to see Yap's progress
// and error states. For the AI Chat's tool loop (src-tauri/src/tools.rs) a
// spec scripts the model call by call (`scriptChat`): each chat call that
// offers tools takes the next step, a tool call or an answer, and the
// request record keeps the messages (the tool results Yap sent back).
//
// It also answers the providers' own web searches (src-tauri/src/web/
// native.rs), the sub-call Yap makes for a `web_search` with "The AI's own
// search" on: Groq's chat completions with `tools: [{type:
// "browser_search"}]` (kind "groqSearch": gpt-oss's answer with its
// `executed_tools` and a 【1†L4-L5】 citation) and Gemini's native
// `…/models/<model>:generateContent` with `google_search` (kind
// "geminiSearch": grounding chunks with Google's redirect links). Both find
// Gale in Baldur's Gate 3. `failNext("groqSearch", 1, 429)` answers the
// next one with an HTTP error, to see Yap fall back to its own search.
//
// It also lists models (src-tauri/src/model_catalog.rs): `GET <base>/models`
// as a self-hosted OpenAI-compatible server (`serverModels`), and — through
// `modelsStandin`, the debug-only YAP_MODELS_STANDIN — each cloud provider's
// own list in that provider's shape (`listModels(provider, models)`: Groq's,
// OpenAI's, OpenRouter's `{data}`, Anthropic's paged `{data, has_more}`,
// Gemini's `{models}`). `failModels(provider, n)` answers the next `n` lists
// with HTTP 500; `modelRequests` keeps `{ provider, host, path, url,
// headers }` for every list asked for. `serveOllama({ loaded, show })` makes
// the server answer Ollama's own `GET /api/ps` (`loaded`: model → the
// context it's loaded with) and `POST /api/show` (`show`: model → `{ max,
// numCtx }`), which Yap reads for the context a self-hosted model runs
// with; `serveOllama(null)` stops.
import http from 'node:http';

export const GALE_URL = 'https://bg3.wiki/wiki/Gale';
export const GUIDE_URL = 'https://guides.example/bg3-gale';
export const GROUNDING_REDIRECT = 'https://vertexaisearch.cloud.google.com/grounding-api-redirect/';

/** Groq's gpt-oss answer with browser_search (the shape the eval spike recorded). */
function groqSearchReply(query) {
  const found = /gale|baldur|bg3/i.test(query);
  return {
    role: 'assistant',
    content: found
      ? 'Gale is stuck in a rune circle on the beach west of the Nautiloid crash site.【1†L4-L5】'
      : 'I found nothing about that.',
    reasoning: 'Search, open the wiki, answer.',
    executed_tools: found
      ? [
          {
            index: 0,
            name: 'browser.search',
            type: 'browser_search',
            arguments: JSON.stringify({ query, topn: 5 }),
            output: `L0: \nL1: URL: https://exa.ai/search?q=${encodeURIComponent(query)}\nL2: # Search Results\nL4: * 【0†Gale - bg3.wiki†bg3.wiki】\nL5: * 【1†Where to find Gale in BG3†guides.example】`,
            search_results: {
              results: [
                { title: 'Gale - bg3.wiki', url: GALE_URL, content: '', score: 0 },
                { title: 'Where to find Gale in BG3', url: GUIDE_URL, content: '', score: 0 },
              ],
            },
          },
          {
            index: 1,
            name: 'browser.open',
            type: 'browser_search',
            arguments: '{"cursor": 0, "id": 0}',
            output: '',
            search_results: {
              results: [
                {
                  title: 'bg3.wiki - viewing lines [0 - 6] of 6',
                  url: GALE_URL,
                  content: `L0: \nL1: URL: ${GALE_URL}\nL2: Gale - bg3.wiki\nL3: \nL4: Gale is found on the beach west of the Nautiloid crash site,\nL5: stuck in a rune circle. Pull him out to recruit him.\nL6: Gale can be romanced.`,
                  score: 0,
                },
              ],
            },
          },
        ]
      : [],
  };
}

/** Gemini's generateContent answer grounded with Google Search. */
function geminiSearchReply(query) {
  if (!/gale|baldur|bg3/i.test(query)) {
    return { candidates: [{ content: { role: 'model', parts: [{ text: 'Nothing found.' }] }, finishReason: 'STOP' }] };
  }
  const text = 'Gale is stuck in a rune circle on the beach west of the Nautiloid crash site. Pull him out to recruit him.';
  const first = 'Gale is stuck in a rune circle on the beach west of the Nautiloid crash site.';
  return {
    candidates: [
      {
        content: { role: 'model', parts: [{ text }] },
        finishReason: 'STOP',
        groundingMetadata: {
          webSearchQueries: [query],
          searchEntryPoint: { renderedContent: '<div class="container">Google</div>' },
          groundingChunks: [
            { web: { uri: `${GROUNDING_REDIRECT}gale-1`, title: 'bg3.wiki' } },
            { web: { uri: `${GROUNDING_REDIRECT}gale-2`, title: 'guides.example' } },
          ],
          groundingSupports: [
            { segment: { startIndex: 0, endIndex: Buffer.byteLength(first), text: first }, groundingChunkIndices: [0] },
            {
              segment: { startIndex: Buffer.byteLength(first) + 1, endIndex: Buffer.byteLength(text), text: 'Pull him out to recruit him.' },
              groundingChunkIndices: [0, 1],
            },
          ],
        },
      },
    ],
    usageMetadata: { promptTokenCount: 30, candidatesTokenCount: 25, totalTokenCount: 55 },
  };
}

/** A stand-in address's host → the provider it stands in for. */
const LIST_HOSTS = {
  'api.groq.com': 'groq',
  'api.openai.com': 'openai',
  'api.anthropic.com': 'anthropic',
  'generativelanguage.googleapis.com': 'gemini',
  'openrouter.ai': 'openrouter',
};

/** A provider's list, in its own shape. */
function listBody(provider, models) {
  if (provider === 'gemini') return { models };
  if (provider === 'anthropic') return { data: models, has_more: false, first_id: models[0]?.id ?? null, last_id: models.at(-1)?.id ?? null };
  if (provider === 'openrouter') return { data: models, links: { next: null } };
  return { object: 'list', data: models };
}

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
const QUESTIONS = 'You suggest questions for someone';
const FOLLOW_UP = 'You write the follow-up email';
const BLIND_SPOTS = 'You review a meeting for what it left open';
// A dictation's AI cleanup (llm.rs BASE_PROMPT).
const CLEANUP = 'You are a dictation cleanup engine';
// Memory's passes (memory/prompt.rs): learning from a chat or a meeting,
// "Tell Yap what to change", an import. Answered `{"changes": []}` unless a
// spec says otherwise (`reply('memory', fn)`).
const MEMORY = "You keep Yap's memory of the user";
// A pre-meeting brief's "In short" (llm.rs MEETING_BRIEF_PROMPT,
// calendar/brief.rs). "Nothing to add." unless a spec scripts it
// (`reply('brief', fn)`; brief.spec.js).
const BRIEF = 'You write a short brief for someone about to go into a meeting';

const KINDS = [
  [DIGEST, 'digest'],
  [ACTION_PLAN, 'actionPlan'],
  [TITLE, 'title'],
  [CATCH_UP, 'catchUp'],
  [MEETING_ASK, 'meetingAsk'],
  [QUESTIONS, 'questions'],
  [FOLLOW_UP, 'followUpEmail'],
  [BLIND_SPOTS, 'blindSpots'],
  [CLEANUP, 'cleanup'],
  [MEMORY, 'memory'],
  [BRIEF, 'brief'],
];

/**
 * A transcript line as Yap's meeting prompts write it: "You: …", "Them: …",
 * or the speaker's name once the note's speaker map has one ("Priya Shah: …",
 * "Speaker 2: …"; meeting_summary::transcript_lines). Not the headers.
 */
const SPOKEN = /^(?!(?:Attendees|Speakers besides You|Question|Meeting|Subject|Notes|Latest)\b)[A-Z][\w.' -]{0,40}: /;
const isSpoken = (l) => SPOKEN.test(l);
const unlabel = (l) => l.replace(SPOKEN, '');

/** The transcript a cleanup call frames between triple quotes (llm.rs). */
export function cleanupTranscript(user) {
  return /"""\n([\s\S]*?)\n"""/.exec(user)?.[1] ?? user;
}

/** "Questions to ask": from the latest talk. */
function questionsReply(user) {
  const latest = user.split('The latest talk')[1] ?? '';
  const lines = latest.split('\n').filter(isSpoken);
  const out = [`- What should we do next about the ${lines.length} points just raised?`];
  if (/catering/i.test(latest)) out.push('- Who will own catering for the offsite, and by when?');
  if (/budget/i.test(latest)) out.push('- Does finance need anything else with the revised budget?');
  return out.join('\n');
}

/**
 * "Follow-up email": markdown and a preamble a small model might add, which
 * Yap must take out (meeting_summary::clean_email).
 */
function followUpReply(user) {
  const planned = /Yap's notes on the meeting/.test(user);
  return [
    "Sure! Here's a follow-up email you can send:",
    '',
    '**Subject:** Q3 budget follow-up',
    'Hi all,',
    '',
    `Thanks for going through the Q3 budget${planned ? ' and the action plan' : ''}.`,
    '',
    '**Next steps:**',
    '* Alice: send the revised budget (by Friday)',
    '* Unassigned: catering for the offsite',
    '',
    'Thanks,',
    '[Your name]',
  ].join('\n');
}

/** "Gaps and risks": headings over what was left open. */
function blindSpotsReply(user) {
  const out = ['### Missing owners or dates', '- Catering for the offsite has no owner.'];
  if (/Yap's notes on the meeting/.test(user)) out.push('### Risks and open questions', '- The plan mentions Lisbon, but nobody checked the dates.');
  return out.join('\n');
}

/** The meeting's title, from what was said. */
function titleReply(user) {
  const said = user.split('Transcript (start):')[1] ?? user;
  return /budget/i.test(said) ? 'Q3 Budget Review with Alice' : 'Team Sync';
}

/** "What did I miss?": from the lines said since the person last looked. */
function catchUpReply(user) {
  const since = user.split('Since they last looked')[1] ?? '';
  const lines = since.split('\n').filter(isSpoken);
  const first = unlabel(lines[0] ?? '').split(' ').slice(0, 5).join(' ');
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
  const lines = body.split('\n').filter(isSpoken);
  const first = (lines[0] ?? '')
    .replace(SPOKEN, '')
    .replace(/^SEG-\d+ /, '')
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
 * system, user, tools, messages }` per chat call (`kind`: "digest" |
 * "actionPlan" | "title" | "catchUp" | "meetingAsk" | "questions" |
 * "followUpEmail" | "blindSpots" | "cleanup" | "other"; `tools`: the
 * names of the tools offered; "cleanup" is a dictation's AI cleanup, answered
 * with the transcript as it is unless `reply("cleanup", fn)` says otherwise).
 * `reply(kind, fn)` answers that kind with `fn(user)` from then on.
 * `failNext(kind, n, status = 500)` answers the next `n`
 * calls of a kind with that HTTP error ("groqSearch" / "geminiSearch": a
 * provider's own web search, recorded with `url`, `headers`, `query`,
 * `toolTypes`, `toolChoice` and the `body`); `delay(kind, ms)` holds that kind's answers
 * back (0 = no delay). `scriptChat(steps)` queues the replies to the next
 * calls that offer tools: `{ tools: [{ name, args }] }` calls those tools,
 * `{ content }` answers; with the queue empty they answer "OK".
 */
export async function startFakeLlm() {
  const requests = [];
  const failures = new Map();
  const delays = new Map();
  const chatScript = [];
  const replies = new Map();
  const lists = new Map();
  const listFailures = new Map();
  const modelRequests = [];
  let serverModels = [{ id: 'fake-chat' }, { id: 'llama3.1:8b' }, { id: 'qwen2.5-7b-instruct' }, { id: 'nomic-embed-text' }];
  let callIds = 0;
  let ollama = null;
  const server = http.createServer((req, res) => {
    let raw = '';
    req.on('data', (c) => (raw += c));
    req.on('end', async () => {
      const url = new URL(req.url, 'http://fake');
      // A cloud provider's model list, through the stand-in.
      if (req.method === 'GET' && url.pathname.startsWith('/models-standin/')) {
        const [host, ...rest] = url.pathname.slice('/models-standin/'.length).split('/');
        const provider = LIST_HOSTS[host];
        modelRequests.push({ provider, host, path: `/${rest.join('/')}`, url: req.url, headers: req.headers });
        const failing = listFailures.get(provider) ?? 0;
        if (failing > 0) {
          listFailures.set(provider, failing - 1);
          res.writeHead(500, { 'content-type': 'text/plain' }).end('fake failure: no list today');
          return;
        }
        if (!provider || !lists.has(provider)) {
          res.writeHead(404).end();
          return;
        }
        res.writeHead(200, { 'content-type': 'application/json' });
        res.end(JSON.stringify(listBody(provider, lists.get(provider))));
        return;
      }
      // A self-hosted Ollama's own API, when a test turned it on.
      if (ollama && req.method === 'GET' && url.pathname === '/api/ps') {
        const models = Object.entries(ollama.loaded || {}).map(([name, ctx]) => ({ name, model: name, context_length: ctx }));
        res.writeHead(200, { 'content-type': 'application/json' }).end(JSON.stringify({ models }));
        return;
      }
      if (ollama && req.method === 'POST' && url.pathname === '/api/show') {
        let model = '';
        try {
          model = JSON.parse(raw || '{}').model || '';
        } catch {
          /* not JSON */
        }
        const m = ollama.show?.[model];
        if (!m) {
          res.writeHead(404, { 'content-type': 'application/json' }).end(JSON.stringify({ error: `model '${model}' not found` }));
          return;
        }
        res.writeHead(200, { 'content-type': 'application/json' }).end(
          JSON.stringify({
            parameters: m.numCtx ? `stop "<|eot_id|>"\nnum_ctx ${m.numCtx}` : 'stop "<|eot_id|>"',
            model_info: { 'general.architecture': 'llama', 'llama.context_length': m.max },
          })
        );
        return;
      }
      // A self-hosted server's own list.
      if (req.method === 'GET' && url.pathname.endsWith('/models')) {
        modelRequests.push({ provider: 'server', host: 'fake', path: url.pathname, url: req.url, headers: req.headers });
        res.writeHead(200, { 'content-type': 'application/json' });
        res.end(JSON.stringify({ object: 'list', data: serverModels.map((m) => ({ object: 'model', owned_by: 'fake', ...m })) }));
        return;
      }
      const gemini = req.url.endsWith(':generateContent');
      if (req.method !== 'POST' || !(req.url.endsWith('/chat/completions') || gemini)) {
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
      const json = (status, value) => {
        res.writeHead(status, { 'content-type': 'application/json' });
        res.end(JSON.stringify(value));
      };
      // A provider's own web search (web/native.rs): Groq's browser_search
      // or Gemini's google_search, the sub-call's only tool.
      const toolTypes = (body.tools ?? []).map((t) => t.type ?? Object.keys(t)[0]);
      const search = gemini ? 'geminiSearch' : toolTypes.includes('browser_search') ? 'groqSearch' : null;
      if (search) {
        const messages = body.messages ?? [];
        const user = gemini
          ? (body.contents ?? []).at(-1)?.parts?.map((p) => p.text).join('') ?? ''
          : messages.filter((m) => m.role === 'user').at(-1)?.content ?? '';
        const system = gemini ? body.systemInstruction?.parts?.[0]?.text ?? '' : messages.find((m) => m.role === 'system')?.content ?? '';
        const query = /Search the web for: (.*)/.exec(user)?.[1] ?? user;
        requests.push({
          kind: search,
          model: gemini ? decodeURIComponent(/models\/([^/:]+):generateContent/.exec(req.url)?.[1] ?? '') : body.model,
          url: req.url,
          headers: req.headers,
          system,
          user,
          query,
          tools: [],
          toolTypes,
          toolChoice: body.tool_choice ?? null,
          body,
        });
        const failing = failures.get(search);
        if (failing?.n > 0) {
          failing.n -= 1;
          json(failing.status, { error: { message: `fake ${search} error ${failing.status}: Rate limit reached. Please try again in 9.5s.` } });
          return;
        }
        if (gemini) {
          json(200, geminiSearchReply(query));
        } else {
          json(200, {
            id: `fake-${requests.length}`,
            object: 'chat.completion',
            model: body.model,
            choices: [{ index: 0, message: groqSearchReply(query), finish_reason: 'stop' }],
            usage: { prompt_tokens: 3400, completion_tokens: 190, total_tokens: 3590 },
          });
        }
        return;
      }
      const messages = body.messages ?? [];
      const system = messages.find((m) => m.role === 'system')?.content ?? '';
      const user = messages.filter((m) => m.role === 'user').at(-1)?.content ?? '';
      const kind = KINDS.find(([prefix]) => system.startsWith(prefix))?.[1] ?? 'other';
      const offered = (body.tools ?? []).map((t) => t.function?.name);
      requests.push({
        kind,
        model: body.model,
        maxTokens: body.max_tokens ?? null,
        tokens: messages.reduce((n, m) => n + estimateTokens(String(m.content ?? '')), 0),
        system,
        user,
        tools: offered,
        toolChoice: body.tool_choice ?? null,
        responseFormat: body.response_format ?? null,
        messages,
      });
      const wait = delays.get(kind) ?? 0;
      if (wait > 0) await new Promise((r) => setTimeout(r, wait));
      const failing = failures.get(kind);
      if (failing?.n > 0) {
        failing.n -= 1;
        res.writeHead(failing.status, { 'content-type': 'text/plain' }).end('fake failure: the model is down');
        return;
      }
      // The chat's tool loop, scripted: the next step's tool calls or answer.
      const step = offered.length > 0 ? chatScript.shift() : undefined;
      if (step) {
        const message = step.tools
          ? {
              role: 'assistant',
              content: null,
              tool_calls: step.tools.map(({ name, args }) => ({
                id: `call_${++callIds}`,
                type: 'function',
                function: { name, arguments: JSON.stringify(args) },
              })),
            }
          : { role: 'assistant', content: step.content };
        res.writeHead(200, { 'content-type': 'application/json' });
        res.end(
          JSON.stringify({
            id: `fake-${requests.length}`,
            object: 'chat.completion',
            model: body.model,
            choices: [{ index: 0, message, finish_reason: step.tools ? 'tool_calls' : 'stop' }],
            usage: { total_tokens: 0 },
          })
        );
        return;
      }
      const answer = {
        cleanup: cleanupTranscript,
        digest: digestReply,
        actionPlan: actionPlanReply,
        title: titleReply,
        catchUp: catchUpReply,
        meetingAsk: meetingAskReply,
        questions: questionsReply,
        followUpEmail: followUpReply,
        blindSpots: blindSpotsReply,
        memory: () => '{"changes": []}',
        brief: () => 'Nothing to add.',
      };
      const content = (replies.get(kind) ?? answer[kind] ?? (() => 'OK'))(user);
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
  const origin = `http://127.0.0.1:${server.address().port}`;
  return {
    base: `${origin}/v1`,
    /** YAP_MODELS_STANDIN: every provider's model list through this server. */
    modelsStandin: `${origin}/models-standin`,
    modelRequests,
    listModels: (provider, models) => lists.set(provider, models),
    failModels: (provider, n = 1) => listFailures.set(provider, n),
    setServerModels: (models) => (serverModels = models),
    serveOllama: (o) => (ollama = o),
    requests,
    failNext: (kind, n = 1, status = 500) => failures.set(kind, { n, status }),
    delay: (kind, ms) => delays.set(kind, ms),
    scriptChat: (steps) => chatScript.push(...steps),
    reply: (kind, fn) => replies.set(kind, fn),
    close: () => new Promise((resolve) => server.close(resolve)),
  };
}
