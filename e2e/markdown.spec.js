// src/lib/core/markdown.js on its own (no app needed): markdown-it + DOMPurify
// with Yap's own rules, as chat answers, notes, the notepad and update notes
// show it. Models write numbered lists with blank lines between the items
// ("1. …\n\n2. …"), which showed "1." on every item until 2026-10-21 (each
// item was a list of its own); then tables and <https://…> links broke the
// hand-written parser in turn, and it became markdown-it.
//
// DOMPurify needs a DOM: jsdom's window stands in for the page's, for this
// file's tests only.
import { test, expect } from '@playwright/test';
import { JSDOM } from 'jsdom';
import { renderMarkdown, markdownToText, taskLines, emailHtml, emailText } from '../src/lib/core/markdown.js';

let dom;
test.beforeAll(() => {
  dom = new JSDOM('');
  globalThis.window = dom.window;
});
test.afterAll(() => {
  delete globalThis.window;
  dom.window.close();
});

// The lists' tags in order (classes aside).
const tags = (html) => (html.match(/<\/?(?:ol|ul|li)\b[^>]*>/g) ?? []).map((t) => t.replace(/ class="[^"]*"/, ''));
/** The rendered HTML as elements, to ask what's really in it. */
const parse = (html) => {
  const box = dom.window.document.createElement('div');
  box.innerHTML = html;
  return box;
};

test('a blank line between items keeps one list, numbered as written', () => {
  const html = renderMarkdown('Here are three:\n\n1. **Starfall Requiem**\n\n2. **Ashen Crown II**\n\n3. **Tidebound Saga**\n\nWant more?');
  expect(tags(html)).toEqual(['<ol>', '<li>', '</li>', '<li>', '</li>', '<li>', '</li>', '</ol>']);
  expect(html).toContain('<p>Here are three:</p>');
  // A "loose" list (blank lines between items) still renders tight: one
  // paragraph per item shows without its <p> and its margins.
  expect(html).toContain('<li><strong>Ashen Crown II</strong></li>');
  expect(html).toMatch(/<\/ol>\n<p>Want more\?<\/p>$/);
  // Bullets too, whichever mark each item uses (CommonMark alone would start
  // a new list at the "*").
  expect(tags(renderMarkdown('- one\n\n- two\n\n* three'))).toEqual(['<ul>', '<li>', '</li>', '<li>', '</li>', '<li>', '</li>', '</ul>']);
  expect(tags(renderMarkdown('1. one\n2) two'))).toEqual(['<ol>', '<li>', '</li>', '<li>', '</li>', '</ol>']);
});

test('a list split by a paragraph starts again at its own number', () => {
  const html = renderMarkdown('1. one\n2. two\n\nAnd then:\n\n3. three\n4. four');
  expect(tags(html)).toEqual(['<ol>', '<li>', '</li>', '<li>', '</li>', '</ol>', '<ol start="3">', '<li>', '</li>', '<li>', '</li>', '</ol>']);
  expect(renderMarkdown('5. five')).toBe('<ol start="5">\n<li>five</li>\n</ol>');
});

test('lines and lists indented under an item belong to it', () => {
  const html = renderMarkdown(
    '1. **Starfall Requiem** [1]\n   A sci-fi party RPG.\n   - PC and consoles\n   - 12 March\n\n2. **Ashen Crown II**\n\n   The sequel.'
  );
  expect(tags(html)).toEqual(['<ol>', '<li>', '<ul>', '<li>', '</li>', '<li>', '</li>', '</ul>', '</li>', '<li>', '</li>', '</ol>']);
  expect(html).toContain('<li><strong>Starfall Requiem</strong> [1]<br>\nA sci-fi party RPG.\n<ul>');
  // An item with two paragraphs keeps them as paragraphs (md-loose: app.css
  // trims the space above the first and below the last).
  expect(html).toContain('<li class="md-loose">\n<p><strong>Ashen Crown II</strong></p>\n<p>The sequel.</p>\n</li>\n</ol>');
  // A different kind of list at the same level is a list of its own.
  expect(tags(renderMarkdown('1. one\n- bullet'))).toEqual(['<ol>', '<li>', '</li>', '</ol>', '<ul>', '<li>', '</li>', '</ul>']);
  // Tasks keep their checkboxes; a heading ends the list.
  const tasks = renderMarkdown('- [ ] ship it\n\n- [x] test it\n## Next');
  expect(tasks).toContain('<li class="task"><input type="checkbox" disabled="" aria-label="ship it"> ship it');
  expect(tasks).toContain('<li class="task"><input type="checkbox" disabled="" checked="" aria-label="test it"> test it</li>\n</ul>\n<h3>Next</h3>');
  // Only in bullet lists, and only at the start of the item.
  expect(renderMarkdown('1. [ ] not a task')).not.toContain('<input');
  expect(renderMarkdown('- see [x] later')).not.toContain('<input');
});

test("a summary's tasks can be ticked, each mapped to its line", () => {
  const plan = '## Action plan\n### You\n- [ ] Draft the **agenda** (due: Monday)\n- [x] Share the slides\n\n```\n- [ ] code, not a task\n```\n1. [ ] numbered, not a task\n- [ ] Book the venue\n  - [ ] nested <b>one</b>';
  // Tickable: enabled, named by their words (as markup-free text).
  const html = renderMarkdown(plan, { tickable: true });
  const boxes = [...parse(html).querySelectorAll('input')];
  expect(boxes.map((b) => [b.getAttribute('aria-label'), b.checked, b.disabled])).toEqual([
    ['Draft the **agenda** (due: Monday)', false, false],
    ['Share the slides', true, false],
    ['Book the venue', false, false],
    ['nested <b>one</b>', false, false],
  ]);
  // Everywhere else (an answer): never tickable.
  expect(parse(renderMarkdown(plan)).querySelectorAll('input:not([disabled])')).toHaveLength(0);
  // The nth checkbox is the task on taskLines(text)[n] (src-tauri/src/notes/tasks.rs ticks it there).
  expect(taskLines(plan)).toEqual([2, 3, 9, 10]);
  expect(taskLines('No tasks here.')).toEqual([]);
  // "• " bullets keep their lines.
  expect(taskLines('Intro\n• [ ] one\n• [x] two')).toEqual([1, 2]);
});

test('nested lists, loose and tight, keep their levels', () => {
  const html = renderMarkdown('- fruit\n  - apple\n  - pear\n\n    Ripe in autumn.\n- veg\n  1. leek\n  2. kale');
  expect(tags(html)).toEqual([
    '<ul>', '<li>', '<ul>', '<li>', '</li>', '<li>', '</li>', '</ul>', '</li>',
    '<li>', '<ol>', '<li>', '</li>', '<li>', '</li>', '</ol>', '</li>', '</ul>',
  ]);
  // The inner list is loose (a blank line inside "pear"): that item keeps
  // its two paragraphs, "apple" stays tight.
  expect(html).toContain('<li>apple</li>');
  expect(html).toContain('<li class="md-loose">\n<p>pear</p>\n<p>Ripe in autumn.</p>\n</li>');
  expect(html).toContain('<li>fruit\n<ul>');
  // "• " lines (small models write them) are bullets too.
  expect(tags(renderMarkdown('• one\n• two'))).toEqual(['<ul>', '<li>', '</li>', '<li>', '</li>', '</ul>']);
});

test('headings shift down a level; line breaks are kept', () => {
  expect(renderMarkdown('# One\n## Two\n### Three\n#### Four\n##### Five')).toBe(
    '<h2>One</h2>\n<h3>Two</h3>\n<h4>Three</h4>\n<h5>Four</h5>\n<h5>Five</h5>'
  );
  expect(renderMarkdown('Alice: the budget\nBob: the venue')).toBe('<p>Alice: the budget<br>\nBob: the venue</p>');
});

test('block quotes, rules, strikethrough and emphasis', () => {
  expect(renderMarkdown('> Ship it **Friday**\n> or Monday.')).toBe(
    '<blockquote class="md-quote">\n<p>Ship it <strong>Friday</strong><br>\nor Monday.</p>\n</blockquote>'
  );
  expect(renderMarkdown('Above\n\n---\n\nBelow')).toBe('<p>Above</p>\n<hr class="md-rule">\n<p>Below</p>');
  expect(renderMarkdown('~~Tuesday~~ Wednesday, *maybe* _not_ __sure__')).toBe(
    '<p><s>Tuesday</s> Wednesday, <em>maybe</em> <em>not</em> <strong>sure</strong></p>'
  );
  // An underscore inside a word is just a letter.
  expect(renderMarkdown('use snake_case_names')).toBe('<p>use snake_case_names</p>');
});

// Groq's gpt-oss writes tables; Chat showed the raw pipes (2026-10-21).
test('a table becomes a table, aligned as its rule line says', () => {
  const md =
    'Here are the plans:\n\n| Plan | Price | Usage |\n|:-----|------:|:-----:|\n| Plus | $8 | 1x |\n| **Premium** | $30 | 4x |\n\nPick one.';
  const html = renderMarkdown(md);
  expect(html).toContain('<div class="md-table"><table><thead><tr><th>Plan</th><th class="r">Price</th><th class="c">Usage</th></tr></thead>');
  expect(html).toContain('<tr><td>Plus</td><td class="r">$8</td><td class="c">1x</td></tr>');
  expect(html).toContain('<td><strong>Premium</strong></td>');
  expect(html).toMatch(/<\/table><\/div>\n<p>Pick one\.<\/p>$/);
  // Without outer pipes, and a short row filled in.
  const bare = renderMarkdown('Name | Score\n--- | ---\nAda | 3\nBo |\nAfter');
  expect(bare).toContain('<tr><td>Ada</td><td>3</td></tr><tr><td>Bo</td><td></td></tr></tbody>');
  // A line without a pipe ends the table.
  expect(bare).toMatch(/<\/table><\/div>\n<p>After<\/p>$/);
  // A lone pipe in a sentence is just text.
  expect(renderMarkdown('cats | dogs')).toBe('<p>cats | dogs</p>');
  // Inline code in a cell, and an escaped pipe that stays in its cell.
  expect(renderMarkdown('| Command | Does |\n|---|---|\n| `ls \\| wc -l` | counts a \\| b |')).toBe(
    '<div class="md-table"><table><thead><tr><th>Command</th><th>Does</th></tr></thead>' +
      '<tbody><tr><td><code>ls | wc -l</code></td><td>counts a | b</td></tr></tbody></table></div>'
  );
});

// gpt-oss writes "<https://x.ai/pricing>"; Chat showed the angle brackets (2026-10-21).
test('an autolink in angle brackets is just the link', () => {
  const html = renderMarkdown('Sign up at <https://x.ai/pricing>.');
  expect(html).toBe('<p>Sign up at <a class="md-link" href="https://x.ai/pricing" title="https://x.ai/pricing">https://x.ai/pricing</a>.</p>');
  // Other angle brackets stay text.
  expect(renderMarkdown('a <b> c')).toBe('<p>a &lt;b&gt; c</p>');
});

test('web and mail addresses become links; nothing else does', () => {
  const link = (url, text = url) => `<a class="md-link" href="${url}" title="${url}">${text}</a>`;
  // Bare addresses, without their trailing full stop; a mail address.
  expect(renderMarkdown('See https://example.org/app. Or write to help@example.org!')).toBe(
    `<p>See ${link('https://example.org/app')}. Or write to ${link('mailto:help@example.org', 'help@example.org')}!</p>`
  );
  // A file name or a bare domain isn't a link.
  expect(renderMarkdown('Edit README.md on x.ai')).toBe('<p>Edit README.md on x.ai</p>');
  // [text](url), the url's & escaped.
  expect(renderMarkdown('[the plan](https://docs.example.com/plan?a=1&b=2 "Plan")')).toBe(
    `<p>${link('https://docs.example.com/plan?a=1&amp;b=2', 'the plan')}</p>`
  );
  // javascript:, data:, file: and relative links stay text.
  for (const md of [
    '[x](javascript:alert(1))',
    '[x](JavaScript:alert(1))',
    '<javascript:alert(1)>',
    '[x](data:text/html,<b>hi</b>)',
    '[x](file:///C:/Windows)',
    '[x](/settings)',
    'ftp://files.example.com/a',
    '[x]: javascript:alert(1)\n\n[x]',
  ]) {
    expect(renderMarkdown(md), md).not.toContain('<a');
  }
  // No images: an image would make Yap fetch its address.
  expect(renderMarkdown('![a cat](https://example.org/cat.png)')).not.toContain('<img');
});

test('raw HTML from a model shows as text, never as markup', () => {
  expect(renderMarkdown('<img src=x onerror=alert(1)>')).toBe('<p>&lt;img src=x onerror=alert(1)&gt;</p>');
  expect(renderMarkdown('Hi <script>alert(1)</script> there')).toBe('<p>Hi &lt;script&gt;alert(1)&lt;/script&gt; there</p>');
  expect(renderMarkdown('<div onclick="x()">\n\n**bold**\n\n</div>')).toBe(
    '<p>&lt;div onclick="x()"&gt;</p>\n<p><strong>bold</strong></p>\n<p>&lt;/div&gt;</p>'
  );
  expect(renderMarkdown('<a href="javascript:alert(1)">x</a>')).not.toContain('<a');
  expect(renderMarkdown('<style>body{display:none}</style>')).not.toContain('<style');
  // In a code block too.
  expect(renderMarkdown('```html\n<script>alert(1)</script>\n```')).toBe(
    '<pre class="md-code"><code>&lt;script&gt;alert(1)&lt;/script&gt;</code></pre>'
  );
});

test('whatever the input, only the elements and attributes Yap makes come out', () => {
  const TAGS = new Set('P BR STRONG EM S CODE PRE A SPAN INPUT UL OL LI H2 H3 H4 H5 BLOCKQUOTE HR DIV TABLE THEAD TBODY TR TH TD'.split(' '));
  const ATTRS = new Set(['class', 'href', 'title', 'aria-label', 'start', 'type', 'checked', 'disabled']);
  const hostile = [
    '<iframe src="https://evil.example"></iframe>',
    '<svg><script>alert(1)</script></svg>',
    '<math><mi xlink:href="javascript:alert(1)">x</mi></math>',
    '[x](https://ok.example "a\\" onmouseover=\\"alert(1)")',
    '[x](https://ok.example/"onmouseover="alert(1))',
    '[x](<javascript:alert(1)>)',
    '[x](java&#115;cript:alert(1))',
    '[x](&#x6A;avascript:alert(1))',
    '[x]( javascript:alert(1))',
    '[x](vbscript:msgbox(1))',
    '<form action="https://evil.example"><input name=pw></form>',
    '<a href="https://ok.example" style="position:fixed">x</a>',
    '- [x] <input type=text value=hi>',
    '| a | b |\n|---|---|\n| <img src=x onerror=alert(1)> | `<b>` |',
    '```\n</code></pre><img src=x onerror=alert(1)>\n```',
    '> <details open ontoggle=alert(1)>',
    '# <span style="color:red">big</span>',
    'mailto:x@y.z?subject=<script>',
  ];
  for (const md of hostile) {
    for (const el of parse(renderMarkdown(md, { sources: SOURCES })).querySelectorAll('*')) {
      expect(TAGS.has(el.nodeName), `${md} → <${el.nodeName}>`).toBe(true);
      for (const name of el.getAttributeNames()) expect(ATTRS.has(name), `${md} → ${name}`).toBe(true);
      if (el.hasAttribute('href')) expect(el.getAttribute('href'), md).toMatch(/^(https?:\/\/|mailto:)/i);
      if (el.nodeName === 'INPUT') {
        expect(el.getAttribute('type')).toBe('checkbox');
        expect(el.hasAttribute('disabled')).toBe(true);
      }
    }
  }
});

test('code: fences, indented blocks and inline code; a cut-off fence still shows', () => {
  expect(renderMarkdown('Run:\n\n```js\nconsole.log("<hi>");\n```\n\nDone.')).toBe(
    '<p>Run:</p>\n<pre class="md-code"><code>console.log("&lt;hi&gt;");</code></pre>\n<p>Done.</p>'
  );
  expect(renderMarkdown('Then:\n\n    npm run build\n')).toBe('<p>Then:</p>\n<pre class="md-code"><code>npm run build</code></pre>');
  expect(renderMarkdown('Use `**not bold**` here')).toBe('<p>Use <code>**not bold**</code> here</p>');
  // An answer cut off mid-block (a limit, a stop) still shows its code.
  expect(renderMarkdown('Here:\n\n```python\ndef f():\n    return 1')).toBe(
    '<p>Here:</p>\n<pre class="md-code"><code>def f():\n    return 1</code></pre>'
  );
});

const SOURCES = [
  { n: 1, title: 'Gale', url: 'https://bg3.wiki/wiki/Gale', site: 'bg3.wiki' },
  { n: 2, title: 'Where to find "Gale"', url: 'https://guides.example/gale?x=1&y=2', site: 'guides.example' },
];
const pill = (s, label) => `<a class="md-link md-cite" href="${s.url.replaceAll('&', '&amp;')}" title="${label}" aria-label="Source ${s.n}: ${label}">${s.n}</a>`;
const PILL1 = pill(SOURCES[0], 'Gale · bg3.wiki');
const PILL2 = pill(SOURCES[1], 'Where to find &quot;Gale&quot; · guides.example');

test('a web answer: citations are pills, only its sources stay links', () => {
  const web = (md) => renderMarkdown(md, { sources: SOURCES });
  expect(web('On the beach [1], in a circle [1][2] or [2, 1].')).toBe(
    `<p>On the beach <span class="md-cites">${PILL1}</span>, in a circle <span class="md-cites">${PILL1}</span>` +
      `<span class="md-cites">${PILL2}</span> or <span class="md-cites">${PILL2}${PILL1}</span>.</p>`
  );
  // Unknown numbers stay text; a link whose text is "1" stays that link.
  expect(web('Not [3] nor [1, 9].')).toBe('<p>Not [3] nor [1, 9].</p>');
  expect(web('[1](https://bg3.wiki/wiki/Gale)')).toBe(
    '<p><a class="md-link" href="https://bg3.wiki/wiki/Gale" title="https://bg3.wiki/wiki/Gale">1</a></p>'
  );
  // Inline code keeps its brackets.
  expect(web('Write `[1]`')).toBe('<p>Write <code>[1]</code></p>');
  // A source's own link stays a link (a trailing slash or #part aside);
  // any other address the model wrote is text.
  expect(web('From <https://bg3.wiki/wiki/Gale/#Recruit> and [the guide](https://guides.example/gale?x=1&y=2).')).toBe(
    '<p>From <a class="md-link" href="https://bg3.wiki/wiki/Gale/#Recruit" title="https://bg3.wiki/wiki/Gale/#Recruit">https://bg3.wiki/wiki/Gale/#Recruit</a>' +
      ' and <a class="md-link" href="https://guides.example/gale?x=1&amp;y=2" title="https://guides.example/gale?x=1&amp;y=2">the guide</a>.</p>'
  );
  expect(web('Log in at [your bank](https://evil.example/login), https://evil.example/x or <https://evil.example/y>.')).toBe(
    '<p>Log in at your bank, https://evil.example/x or https://evil.example/y.</p>'
  );
  // Without sources, "[1]" is just text and every web link is a link.
  expect(renderMarkdown('See [1] at https://evil.example/x')).toBe(
    '<p>See [1] at <a class="md-link" href="https://evil.example/x" title="https://evil.example/x">https://evil.example/x</a></p>'
  );
  // A source with an unsafe address, or markup in its title, can't get through.
  const odd = [
    { n: 1, title: '<img src=x onerror=alert(1)>', url: 'https://ok.example/', site: '' },
    { n: 2, title: 'x', url: 'javascript:alert(1)', site: 'x' },
  ];
  const page = parse(renderMarkdown('A [1] and B [2].', { sources: odd }));
  expect(page.querySelector('img')).toBeNull();
  expect(page.querySelector('a.md-cite').title).toBe('<img src=x onerror=alert(1)>');
  expect(page.querySelectorAll('a')).toHaveLength(1);
  expect(page.textContent).toBe('A 1 and B [2].');
});

test('a citation inside a table cell is a pill too', () => {
  const html = renderMarkdown('| Who | Where |\n|---|---|\n| Gale [1] | Beach [1, 2] |', { sources: SOURCES });
  expect(html).toContain(`<td>Gale <span class="md-cites">${PILL1}</span></td>`);
  expect(html).toContain(`<td>Beach <span class="md-cites">${PILL1}${PILL2}</span></td>`);
});

test('markdownToText: plain text for pasting', () => {
  expect(markdownToText('## Plan\n- [ ] ship it\n- [x] test it\n- **bold** and `code`\n  * nested')).toBe(
    'Plan\n☐ ship it\n☑ test it\n• bold and code\n  • nested'
  );
});

test('a summary copied for email: headings, lists and tasks in HTML, the same as plain text', () => {
  const md = [
    '## Recap',
    'The team planned the **offsite**.',
    '- Budget',
    '',
    '## Action plan',
    '### You',
    '- [ ] Draft the agenda (due: tonight)',
    '- [x] Share the slides',
    '### Priya',
    '- [ ] Send the [revised budget](https://example.com/budget)',
    '',
    '## Decisions',
    '- Hold it in Lisbon — the flights are cheaper',
    '',
    '| Who | What |',
    '|---|---|',
    '| Priya | budget |',
    '',
    '<script>alert(1)</script>',
  ].join('\n');
  const html = emailHtml(md, { title: 'Offsite <planning>' });
  const box = parse(html);
  const root = box.firstElementChild;
  // One wrapper with the mail client's body font, no colours anywhere.
  expect(root.nodeName).toBe('DIV');
  expect(root.getAttribute('style')).toContain('font-family');
  expect(html).not.toMatch(/color\s*:/);
  // The title is the heading (escaped), then the summary's own, a level down.
  expect(root.firstElementChild.outerHTML).toMatch(/^<h2 style="[^"]+">Offsite &lt;planning&gt;<\/h2>$/);
  expect([...box.querySelectorAll('h3')].map((h) => h.textContent)).toEqual(['Recap', 'Action plan', 'Decisions']);
  expect([...box.querySelectorAll('h4')].map((h) => h.textContent)).toEqual(['You', 'Priya']);
  // Tasks as ☐ / ☑ (a mail has no checkboxes), every list item styled, no classes.
  const items = [...box.querySelectorAll('li')].map((li) => li.textContent.trim());
  expect(items).toContain('☐ Draft the agenda (due: tonight)');
  expect(items).toContain('☑ Share the slides');
  expect(items).toContain('Hold it in Lisbon — the flights are cheaper');
  expect(box.querySelector('input')).toBeNull();
  expect(box.querySelector('[class]')).toBeNull();
  expect([...box.querySelectorAll('li, p, ul, h3, h4, table, td, th')].every((el) => el.hasAttribute('style'))).toBe(true);
  // A link stays a link; a table a table (its scroller gone); raw HTML stays text.
  expect(box.querySelector('a').getAttribute('href')).toBe('https://example.com/budget');
  expect(box.querySelector('table td').textContent).toBe('Priya');
  expect(root.querySelector('div')).toBeNull();
  expect(box.querySelector('script')).toBeNull();
  expect(box.textContent).toContain('<script>alert(1)</script>');
  // The plain-text twin: the title, then the summary as text.
  const text = emailText(md, { title: 'Offsite <planning>' });
  expect(text.split('\n').slice(0, 3)).toEqual(['Offsite <planning>', '', 'Recap']);
  expect(text).toContain('☐ Draft the agenda (due: tonight)');
  expect(text).toContain('☑ Share the slides');
  expect(text).not.toMatch(/^#|\*\*/m);
  // No title: no heading.
  expect(parse(emailHtml('- [ ] one')).querySelector('h2')).toBeNull();
  expect(emailText('- [ ] one')).toBe('☐ one');
});
