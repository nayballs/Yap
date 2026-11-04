// The issue-labels workflow (.github/workflows/issue-labels.yml) against the
// bug form (.github/ISSUE_TEMPLATE/bug_report.yml): the workflow's areas are
// the form's "Which part of Yap?" options, word for word and in order, and its
// script, run on made-up events with a fake GitHub client, labels what it
// should and leaves everything else alone. Also: the workflow's token can
// only touch issues, and no expression reads the issue's text.
// `npm run test:issue-labels`; CI's check job runs it.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const read = (p) => fs.readFileSync(path.join(root, p), 'utf8').replace(/\r\n/g, '\n');
const FORM = read('.github/ISSUE_TEMPLATE/bug_report.yml');
const WORKFLOW = read('.github/workflows/issue-labels.yml');

const indentOf = (line) => line.search(/\S/);
const unquote = (s) => (/^(["']).*\1$/.test(s) ? s.slice(1, -1) : s);

/** The bug form's area dropdown: its label and options (a line reader for this one file's plain YAML). */
function formArea(yml) {
  const lines = yml.split('\n');
  const at = lines.findIndex((l) => /^\s*id:\s*area\s*$/.test(l));
  assert.ok(at >= 0, 'bug_report.yml has no field with `id: area`');
  // The field's own lines: up to the next `- type:` item.
  let end = lines.findIndex((l, i) => i > at && /^\s*- type:/.test(l));
  if (end < 0) end = lines.length;
  const field = lines.slice(at, end);
  const label = field.map((l) => l.match(/^\s*label:\s*(.+?)\s*$/)).find(Boolean);
  assert.ok(label, 'the area field has no label');
  const opts = field.findIndex((l) => /^\s*options:\s*$/.test(l));
  assert.ok(opts >= 0, 'the area field has no options');
  const options = [];
  for (const line of field.slice(opts + 1)) {
    if (!line.trim()) continue;
    const m = line.match(/^\s*-\s+(.+?)\s*$/);
    if (!m || indentOf(line) <= indentOf(field[opts])) break;
    options.push(unquote(m[1]));
  }
  return { label: unquote(label[1]), options };
}

/** The workflow's `script: |` block, dedented. */
function workflowScript(yml) {
  const lines = yml.split('\n');
  const at = lines.findIndex((l) => /^\s*script:\s*\|\s*$/.test(l));
  assert.ok(at >= 0, 'issue-labels.yml has no `script: |` block');
  const body = [];
  for (const line of lines.slice(at + 1)) {
    if (line.trim() && indentOf(line) <= indentOf(lines[at])) break;
    body.push(line);
  }
  const cut = Math.min(...body.filter((l) => l.trim()).map(indentOf));
  return body.map((l) => l.slice(cut)).join('\n');
}

/** The script's AREAS: [option, label] in order. Every line of the Map must be an entry. */
function workflowAreas(script) {
  const m = script.match(/const AREAS = new Map\(\[\n([\s\S]*?)\n\]\);/);
  assert.ok(m, 'the script has no `const AREAS = new Map([ … ]);`');
  return m[1].split('\n').map((line) => {
    const e = line.match(/^\s*\["([^"\\]+)", "([^"\\]+)"\],\s*$/);
    assert.ok(e, `not an AREAS entry: ${line}`);
    return [e[1], e[2]];
  });
}

const SCRIPT = workflowScript(WORKFLOW);
const AREAS = workflowAreas(SCRIPT);
const AsyncFunction = Object.getPrototypeOf(async () => {}).constructor;

/** An issue body as GitHub writes a filled-in form (the area's answer only varies). */
function formBody(area, eol = '\n') {
  return [
    '### What happened?',
    '',
    'I pressed F9 in a chat app and nothing was pasted.',
    '',
    '### What did you expect to happen?',
    '',
    'The words in the box.',
    '',
    '### Steps to reproduce',
    '',
    '_No response_',
    '',
    '### Which part of Yap?',
    '',
    area,
    '',
    '### Yap version',
    '',
    '0.1.1-nightly.152',
    '',
    '### Windows version',
    '',
    'Windows 11 Home 24H2, build 26200.7019',
  ].join(eol);
}

/** Run the workflow's script on `payload` with a fake client; what it called. */
async function run(payload, { missing = [], removed404 = false } = {}) {
  const calls = [];
  const notFound = () => Object.assign(new Error('Not Found'), { status: 404 });
  const github = {
    rest: {
      issues: {
        getLabel: async (a) => {
          calls.push(['getLabel', a.name]);
          if (missing.includes(a.name)) throw notFound();
        },
        addLabels: async (a) => calls.push(['addLabels', a.issue_number, a.labels]),
        removeLabel: async (a) => {
          calls.push(['removeLabel', a.issue_number, a.name]);
          if (removed404) throw notFound();
        },
      },
    },
  };
  const warnings = [];
  const core = { warning: (m) => warnings.push(m), info: () => {} };
  const context = { payload, repo: { owner: 'example-owner', repo: 'example-repo' } };
  await new AsyncFunction('github', 'context', 'core', SCRIPT)(github, context, core);
  return { calls: calls.filter(([c]) => c !== 'getLabel'), warnings };
}

const opened = (body) => ({ action: 'opened', issue: { number: 7, body, labels: [] } });
const edited = (body, from, extra = {}) => ({
  action: 'edited',
  issue: { number: 7, body, labels: [] },
  changes: from === undefined ? {} : { body: { from } },
  ...extra,
});

test("the workflow's areas are the bug form's options, word for word and in order", () => {
  const form = formArea(FORM);
  assert.ok(form.options.length > 0);
  assert.deepEqual(
    AREAS.map(([option]) => option),
    form.options,
    'change an option in both bug_report.yml and issue-labels.yml',
  );
});

test('the heading it reads is the form field’s label', () => {
  const { label } = formArea(FORM);
  assert.ok(SCRIPT.includes(`const HEADING = "### ${label}";`), `the script should read "### ${label}"`);
});

test('each area has a label of its own: short, lower case, "area: …"', () => {
  const labels = AREAS.map(([, label]) => label);
  assert.equal(new Set(labels).size, labels.length);
  for (const label of labels) assert.match(label, /^area: [a-z]{2,12}$/);
});

test('the token can only touch issues, and no expression reads the issue’s text', () => {
  assert.match(WORKFLOW, /\npermissions:\n {2}issues: write\n\n/);
  assert.equal(WORKFLOW.match(/permissions:/g).length, 1);
  const expressions = WORKFLOW.match(/\$\{\{[^}]*\}\}/g) ?? [];
  for (const e of expressions) assert.equal(e, '${{ github.event.issue.number }}');
  assert.doesNotMatch(WORKFLOW, /^\s*(- )?run:/m, 'no shell steps');
  const uses = WORKFLOW.match(/uses:\s*(\S+)/g).map((u) => u.replace(/uses:\s*/, ''));
  assert.deepEqual(uses, ['actions/github-script@v9']);
});

test('a new issue gets its area’s label, every option, LF or CRLF', async () => {
  for (const [option, label] of AREAS) {
    for (const eol of ['\n', '\r\n']) {
      const { calls } = await run(opened(formBody(option, eol)));
      assert.deepEqual(calls, [['addLabels', 7, [label]]], `${option} (${JSON.stringify(eol)})`);
    }
  }
});

test('no answer, or one that isn’t exactly an option: nothing', async () => {
  for (const body of [
    null,
    '',
    'A blank issue, no form.',
    formBody('_No response_'),
    formBody('Pasting into an app, I think'),
    formBody('pasting into an app'),
    formBody('constructor'),
    formBody('__proto__'),
    formBody('${{ github.token }}'),
    formBody('Pasting into an app"; echo pwned; "'),
    formBody('Pasting into an app\nThe Yap bar'),
  ]) {
    assert.deepEqual((await run(opened(body))).calls, [], JSON.stringify(body));
  }
});

test('an edit that changes the answer moves the label', async () => {
  const { calls } = await run(edited(formBody('Chat or the Voice Assistant'), formBody('Pasting into an app')));
  assert.deepEqual(calls, [
    ['addLabels', 7, ['area: chat']],
    ['removeLabel', 7, 'area: pasting'],
  ]);
  // The old label already taken off by hand: fine.
  const again = await run(edited(formBody('The Yap bar'), formBody('Pasting into an app')), { removed404: true });
  assert.deepEqual(again.calls, [
    ['addLabels', 7, ['area: bar']],
    ['removeLabel', 7, 'area: pasting'],
  ]);
  // An answer before that wasn't an option: only the new label.
  const fromNone = await run(edited(formBody('The Yap bar'), 'A blank issue, no form.'));
  assert.deepEqual(fromNone.calls, [['addLabels', 7, ['area: bar']]]);
});

test('any other edit leaves the labels alone', async () => {
  // The title only.
  assert.deepEqual((await run(edited(formBody('The Yap bar'), undefined))).calls, []);
  assert.deepEqual((await run({ ...edited(formBody('The Yap bar'), undefined), changes: undefined })).calls, []);
  // The body, but not the answer.
  assert.deepEqual((await run(edited(formBody('The Yap bar'), formBody('The Yap bar').replace('nothing', 'none')))).calls, []);
  // The answer to something not in the list: the old label stays.
  assert.deepEqual((await run(edited(formBody('Something new'), formBody('The Yap bar')))).calls, []);
});

test('a label the repo doesn’t have yet: a warning, nothing added', async () => {
  const { calls, warnings } = await run(opened(formBody('The Yap bar')), { missing: ['area: bar'] });
  assert.deepEqual(calls, []);
  assert.equal(warnings.length, 1);
  assert.match(warnings[0], /"area: bar" doesn't exist yet/);
});
