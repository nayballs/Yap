// desktop/src/lib/core/markdown.js on its own (no app needed): markdown-it + DOMPurify
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
import katex from 'katex';
import { renderMarkdown, markdownToText, taskLines, emailHtml, emailText, markdownTokens, mathOf, mathAt, MATH_MAX, diffLineKind } from '../src/lib/core/markdown.js';
import { diagramRefused, cleanDiagram, cleanMaths } from '../src/lib/core/visualSafety.js';

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
  // The nth checkbox is the task on taskLines(text)[n] (desktop/src-tauri/src/notes/tasks.rs ticks it there).
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
  // A file's path as a link's address: `a.md-file` with its path, no address (a Code chat makes it a link to the file:
  // ui/fileLinks.js); its words only anywhere else.
  expect(renderMarkdown('[the notes](docs/lab.md:3)')).toBe('<p><a class="md-file" data-path="docs/lab.md:3">the notes</a></p>');
  expect(renderMarkdown('[a](my%20file.md)')).toBe('<p><a class="md-file" data-path="my file.md">a</a></p>');
  // javascript:, data:, file:, rooted (`/settings`), in-page and protocol-relative links stay text.
  for (const md of [
    '[x](#top)',
    '[x](//evil.example/x)',
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
    '<pre class="md-code"><code class="lang-html">&lt;script&gt;alert(1)&lt;/script&gt;</code></pre>'
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
    '![x](https://ok.example/a.png "a\\" onerror=\\"alert(1)")',
    '![<img src=x onerror=alert(1)>](https://ok.example/a.png)',
    '![x](javascript:alert(1))',
    '$<svg onload=alert(1)>$ and $$<script>x</script>$$',
    '```mermaid\n<img src=x onerror=alert(1)>\n```',
    '```diff\n+<script>alert(1)</script>\n```',
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
    '<p>Run:</p>\n<pre class="md-code"><code class="lang-js">console.log("&lt;hi&gt;");</code></pre>\n<p>Done.</p>'
  );
  expect(renderMarkdown('Then:\n\n    npm run build\n')).toBe('<p>Then:</p>\n<pre class="md-code"><code>npm run build</code></pre>');
  expect(renderMarkdown('Use `**not bold**` here')).toBe('<p>Use <code>**not bold**</code> here</p>');
  // An answer cut off mid-block (a limit, a stop) still shows its code.
  expect(renderMarkdown('Here:\n\n```python\ndef f():\n    return 1')).toBe(
    '<p>Here:</p>\n<pre class="md-code"><code class="lang-python">def f():\n    return 1</code></pre>'
  );
  // A fence's language is a class on its code (its first word, lower-cased), for the view to colour it
  // afterwards from the text (ui/colourCode.js): the HTML itself never has colours in it.
  expect(renderMarkdown('```TypeScript title="a.ts"\nlet a = 1;\n```')).toBe('<pre class="md-code"><code class="lang-typescript">let a = 1;</code></pre>');
  expect(renderMarkdown('```c++\nint a;\n```')).toBe('<pre class="md-code"><code class="lang-c++">int a;</code></pre>');
  // No language, or a name that isn't a short plain one: no class, and nothing of it gets through as HTML.
  expect(renderMarkdown('```\nplain\n```')).toBe('<pre class="md-code"><code>plain</code></pre>');
  expect(renderMarkdown('```"><img/src=x>\nx\n```')).toBe('<pre class="md-code"><code>x</code></pre>');
  expect(renderMarkdown(`\`\`\`${'a'.repeat(40)}\nx\n\`\`\``)).toBe('<pre class="md-code"><code>x</code></pre>');
});

// Maths, diagrams, diffs and pictures (15 Nov 2026, after the Claude app's
// answers): marked here, as the model wrote them, and drawn by the view
// (ui/markdownVisuals.js: KaTeX, mermaid, a "Show image" box).
test('maths is marked for the view to draw; prices and shell variables stay text', () => {
  expect(renderMarkdown('Inline $E = mc^2$ and $\\sum_{i=1}^n i = \\frac{n(n+1)}{2}$.')).toBe(
    '<p>Inline <span class="md-math">$E = mc^2$</span> and <span class="md-math">$\\sum_{i=1}^n i = \\frac{n(n+1)}{2}$</span>.</p>'
  );
  // Display maths on lines of its own, breaking into a paragraph as models write it.
  expect(renderMarkdown('The area:\n$$\n\\int_0^1 x^2\\,dx = \\frac{1}{3}\n$$\nSo a third.')).toBe(
    '<p>The area:</p>\n<div class="md-math md-math-display">$$\n\\int_0^1 x^2\\,dx = \\frac{1}{3}\n$$</div>\n<p>So a third.</p>'
  );
  expect(renderMarkdown('$$ a^2 + b^2 = c^2 $$')).toBe('<div class="md-math md-math-display">$$ a^2 + b^2 = c^2 $$</div>');
  expect(renderMarkdown('\\[\n\\nabla \\cdot E = 0\n\\]')).toBe('<div class="md-math md-math-display">\\[\n\\nabla \\cdot E = 0\n\\]</div>');
  // Inline in other delimiters, and `$$…$$` within a line.
  expect(renderMarkdown('so \\(a+b\\), \\[c\\] and $$d$$ end')).toBe(
    '<p>so <span class="md-math">\\(a+b\\)</span>, <span class="md-math md-math-display">\\[c\\]</span> and <span class="md-math md-math-display">$$d$$</span> end</p>'
  );
  // Never maths: prices, a currency, shell variables, an escaped dollar, one sign alone.
  for (const text of [
    'It costs $5 and $10 with tax.',
    'Between $5–$10, or $5-$10.',
    'Pay US$5 now.',
    'Set $HOME/$USER and $PATH.',
    'Price $1,000.',
    'Not \\$5 or \\$6$.',
    'A $ sign and another $ here.',
    'Odd $x$y and $x$5.',
    'Unclosed $$\nx^2',
    '\\(not maths',
  ]) {
    expect(renderMarkdown(text), text).not.toContain('md-math');
  }
  // A price, then maths: never one formula from the 5 to the 2.
  expect(renderMarkdown('Pay $5 now; the formula $x^2$ is next.')).toBe('<p>Pay $5 now; the formula <span class="md-math">$x^2$</span> is next.</p>');
  // An escaped dollar inside maths is the formula's own.
  expect(renderMarkdown('$\\$100 + x$')).toBe('<p><span class="md-math">$\\$100 + x$</span></p>');
  // Not in code.
  expect(renderMarkdown('`$x$` and\n\n```\n$$y$$\n```')).not.toContain('md-math');
  // A blank line ends a display block: unclosed, it's text.
  expect(renderMarkdown('$$\nx\n\ny\n$$')).not.toContain('md-math-display">$$\nx');
  // In lists, quotes and tasks too.
  expect(renderMarkdown('- [ ] pay $x$ now\n> $y$')).toContain('pay <span class="md-math">$x$</span> now');
  // Hostile TeX is still only text in a span (KaTeX draws it, sanitised, in the view).
  expect(renderMarkdown('$<img src=x onerror=alert(1)>$')).toBe('<p><span class="md-math">$&lt;img src=x onerror=alert(1)&gt;$</span></p>');
});

test('mathOf reads drawn maths back to its TeX', () => {
  expect(mathOf('$E = mc^2$')).toEqual({ tex: 'E = mc^2', display: false });
  expect(mathOf('$$\n\\int\n$$')).toEqual({ tex: '\n\\int\n', display: true });
  expect(mathOf('\\(a\\)')).toEqual({ tex: 'a', display: false });
  expect(mathOf('\\[b\\]')).toEqual({ tex: 'b', display: true });
  for (const not of ['$ x$', 'plain', '$x$ and more', '', '$$$$']) expect(mathOf(not), not).toBeNull();
  // Too long to draw.
  expect(mathOf(`$${'x'.repeat(MATH_MAX + 1)}$`)).toBeNull();
  expect(mathAt('a $b$', 2)).toMatchObject({ end: 5, tex: 'b', display: false });
});

test('a mermaid fence is a diagram box round its code; a diff is a line each', () => {
  expect(renderMarkdown('```mermaid\nflowchart LR\n  A[Hotkey] --> B[Record]\n```')).toBe(
    '<div class="md-diagram"><pre class="md-code"><code class="lang-mermaid">flowchart LR\n  A[Hotkey] --&gt; B[Record]</code></pre></div>'
  );
  // Its source lines on the box (a document's page maps a selection back by them).
  expect(renderMarkdown('Hi\n\n```mermaid\nA-->B\n```', { lines: true })).toContain('<div class="md-diagram" data-line="2-5"><pre class="md-code">');
  // Hostile code stays text.
  expect(renderMarkdown('```mermaid\n<script>alert(1)</script>\n```')).toContain('&lt;script&gt;alert(1)&lt;/script&gt;');

  const diff = renderMarkdown('```diff\ndiff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1,2 +1,2 @@\n- const old = "removed";\n+ const neu = "<added>";\n  same\n```');
  expect(diff).toBe(
    '<pre class="md-code md-diff"><code>' +
      '<span class="md-dl md-meta">diff --git a/x b/x\n</span>' +
      '<span class="md-dl md-meta">--- a/x\n</span>' +
      '<span class="md-dl md-meta">+++ b/x\n</span>' +
      '<span class="md-dl md-hunk">@@ -1,2 +1,2 @@\n</span>' +
      '<span class="md-dl md-del">- const old = "removed";\n</span>' +
      '<span class="md-dl md-add">+ const neu = "&lt;added&gt;";\n</span>' +
      '<span class="md-dl">  same\n</span>' +
      '</code></pre>'
  );
  // Its text is the diff as written (a copy gives it back), with no code colours on top (no `lang-` class).
  expect(parse(diff).textContent).toBe('diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1,2 +1,2 @@\n- const old = "removed";\n+ const neu = "<added>";\n  same\n');
  expect(renderMarkdown('```patch\n+a\n```')).toContain('<span class="md-dl md-add">+a\n</span>');
  expect(['+x', '-x', '@@ -1 +1 @@', '--- a', '+++ b', 'index 1..2', ' x', ''].map(diffLineKind)).toEqual(['md-add', 'md-del', 'md-hunk', 'md-meta', 'md-meta', 'md-meta', '', '']);
});

test("a picture is a link the view makes a Show image box; it's never fetched by the page", () => {
  expect(renderMarkdown('![A *map* of Fort Joy](https://wiki.example.com/images/map.png)')).toBe(
    '<p><a class="md-link md-image" href="https://wiki.example.com/images/map.png" title="https://wiki.example.com/images/map.png">A map of Fort Joy</a></p>'
  );
  expect(renderMarkdown('![](https://example.org/a.png)')).toContain('>Picture</a>');
  // Not a web address: its alt text (no "!", no link).
  for (const md of ['![a cat](mailto:x@example.org)', '![a cat](file:///C:/cat.png)', '![a cat](data:image/png;base64,AAAA)']) {
    expect(renderMarkdown(md), md).not.toContain('<a');
  }
  // A web answer: a picture only from a source's own address; any other is its alt text.
  expect(renderMarkdown('See ![the map](https://evil.example/x.png?leak=1) [1]', { sources: SOURCES })).toBe(`<p>See the map ${'<span class="md-cites">'}${PILL1}</span></p>`);
  expect(renderMarkdown('![Gale](https://bg3.wiki/wiki/Gale)', { sources: SOURCES })).toContain('class="md-link md-image" href="https://bg3.wiki/wiki/Gale"');
});

test("the document editor's tokens never change: maths and pictures stay text there", () => {
  const text = 'Inline $E = mc^2$ and ![a map](https://example.org/map.png).\n\n$$\nx^2\n$$\n\n```mermaid\nA-->B\n```';
  const inline = markdownTokens(text).filter((t) => t.type === 'inline');
  expect(inline[0].children.map((c) => c.type)).toEqual(['text', 'link_open', 'text', 'link_close', 'text']);
  expect(inline[0].children[0].content).toBe('Inline $E = mc^2$ and !');
  expect(inline[1].content).toBe('$$\nx^2\n$$');
  expect(markdownTokens(text).map((t) => t.type)).not.toContain('yap_math_block');
  expect(markdownTokens(text).find((t) => t.type === 'fence').info).toBe('mermaid');
  // And a summary's tasks map to the same lines either way.
  expect(taskLines('- [ ] pay $x$\n$$\ny\n$$\n- [x] done')).toEqual([0, 4]);
});

test('a diagram that could reach an address is never drawn; a drawing keeps nothing that could', () => {
  expect(diagramRefused('flowchart LR\n  A[Hotkey] --> B{Cleanup on?}\n  B -- yes --> C[AI cleanup]')).toBeNull();
  expect(diagramRefused('sequenceDiagram\n  You->>Yap: Hi\n  Note right of Yap: C:\\data is fine in a label')).toBeNull();
  for (const [source, why] of [
    ['', 'empty'],
    ['flowchart LR\n  A --> B\n  style A fill:url(https://evil.example/x)', 'an address in its styles'],
    ['flowchart LR\n  classDef x fill:\\75rl(https://evil.example)', 'an escape in its styles'],
    ['%%{init: {"themeCSS": "@import \'https://evil.example\'"}}%%\nflowchart LR\n A-->B', 'an address in its styles'],
    ['flowchart LR\n  A["<img src=https://evil.example/x.png>"]', 'markup'],
    ['flowchart LR\n  A["<a href=https://evil.example>x</a>"]', 'markup'],
    ['flowchart LR\n  A@{ img: "https://evil.example/x.png", label: "x" }', 'a picture shape'],
    [`flowchart LR\n${'A-->B\n'.repeat(5000)}`, 'too long'],
  ]) {
    expect(diagramRefused(source), source.slice(0, 60)).toBe(why);
  }

  // What mermaid makes, with everything a hostile one might add.
  const svg =
    '<svg id="d1" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 50" style="max-width: 100px;" onload="alert(1)">' +
    '<style>#d1 .node rect{fill:#fff;stroke:#333}</style>' +
    '<defs><marker id="d1_end"><path d="M0,0L10,5"/></marker></defs>' +
    '<path d="M0,0L50,0" marker-end="url(#d1_end)" style="fill:none"/>' +
    '<g class="node"><rect width="40" height="20"/><foreignObject width="40" height="20"><div xmlns="http://www.w3.org/1999/xhtml"><span class="nodeLabel"><p>Hotkey</p></span></div></foreignObject></g>' +
    '<a href="https://evil.example"><text>link</text></a>' +
    '<image href="https://evil.example/x.png"/><use href="https://evil.example/s.svg#x"/><use href="#d1_end"/>' +
    '<rect style="fill:url(https://evil.example/x)" width="1" height="1"/>' +
    '<script>alert(1)</script></svg>';
  const box = dom.window.document.createElement('div');
  box.append(cleanDiagram(svg));
  const html = box.innerHTML;
  expect(html).not.toMatch(/evil|<script|onload|<image|<a\b/);
  expect(html).toContain('marker-end="url(#d1_end)"');
  expect(html).not.toContain('<use');
  expect(box.querySelector('foreignObject p')?.textContent).toBe('Hotkey');
  expect(box.querySelector('style').textContent).toContain('#d1 .node rect');
  // A sheet that loads something: the whole drawing is refused.
  expect(cleanDiagram('<svg><style>@import url(https://evil.example/x.css);</style><rect/></svg>')).toBeNull();
  expect(cleanDiagram('<svg><style>rect{fill:url( "https://evil.example")}</style></svg>')).toBeNull();
  expect(cleanDiagram('<p>not a drawing</p>')).toBeNull();
});

test("KaTeX's maths, sanitised: drawn, with nothing that links or loads", () => {
  const drawn = cleanMaths(katex.renderToString('\\frac{1}{3} + \\sqrt{x}', { displayMode: true, output: 'htmlAndMathml' }));
  const box = parse(drawn);
  expect(box.querySelector('.katex-display .katex-html')).not.toBeNull();
  // Its TeX stays with it (a copy gives it back, KaTeX's copy-tex).
  expect(box.querySelector('annotation[encoding="application/x-tex"]')?.textContent).toBe('\\frac{1}{3} + \\sqrt{x}');
  // Without trust KaTeX never links; and whatever it gave, no link, picture or script gets through.
  expect(katex.renderToString('\\href{javascript:alert(1)}{x}', { throwOnError: false })).not.toMatch(/<a[\s>]/);
  const hostile = cleanMaths('<span class="katex"><a href="https://evil.example">x</a><img src="https://evil.example/x.png"><span style="background:url(https://evil.example/y)">y</span><script>alert(1)</script></span>');
  expect(hostile).not.toMatch(/evil|<a\b|<img|<script/);
  expect(hostile).toContain('<span>y</span>');
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

// What's only drawn, never kept (15 Nov 2026, the Claude app's answers side by side with Yap's): footnotes
// and a markdown image's link, at render time only — the document editor reads `markdownTokens`, which keep
// the markdown as written, so a document saved there never loses its "[^1]" or "![…](…)".
test('footnotes: a raised number and the notes under the text, in the order first cited', () => {
  const html = renderMarkdown('Tables wrap.[^b] Maths too.[^a] Again.[^b]\n\n[^a]: The second note.\n\n[^b]: The **first** note.\n\n[^c]: Never cited.');
  const box = parse(html);
  expect([...box.querySelectorAll('sup.md-fnref')].map((s) => s.textContent)).toEqual(['1', '2', '1']);
  const notes = [...box.querySelectorAll('div.md-footnotes li')];
  expect(notes.map((li) => li.textContent)).toEqual(['The first note.', 'The second note.']);
  expect(notes[0].querySelector('strong')?.textContent).toBe('first');
  // No raw marks or definitions left, nothing linked, no ids.
  expect(box.textContent).not.toMatch(/\[\^/);
  expect(box.querySelector('[id], a')).toBeNull();
  // A mark with no note stays as written; no notes, no footnote block.
  expect(renderMarkdown('A claim.[^9]')).toContain('[^9]');
  expect(renderMarkdown('No notes here.')).not.toContain('md-footnotes');
});

test('footnotes in a web answer keep only its sources\u2019 links', () => {
  const sources = [{ n: 1, title: 'Lighthouses', url: 'https://en.wikipedia.org/wiki/Lighthouse', site: 'en.wikipedia.org' }];
  const box = parse(renderMarkdown('A tower.[^n] [1]\n\n[^n]: See [the article](https://en.wikipedia.org/wiki/Lighthouse) and [elsewhere](https://phish.example.com).', { sources }));
  const links = [...box.querySelectorAll('div.md-footnotes a')].map((a) => a.getAttribute('href'));
  expect(links).toEqual(['https://en.wikipedia.org/wiki/Lighthouse']);
  expect(box.querySelector('div.md-footnotes').textContent).toContain('elsewhere');
});

test('a markdown image is a link to the picture, with no stray "!"', () => {
  const box = parse(renderMarkdown('![A sample picture](https://upload.example.com/sample.png)'));
  const a = box.querySelector('a.md-link');
  expect(a.textContent).toBe('A sample picture');
  expect(a.getAttribute('href')).toBe('https://upload.example.com/sample.png');
  expect(box.textContent).toBe('A sample picture');
  expect(box.querySelector('img')).toBeNull();
  // A "!" that ends a sentence before a space and a link stays.
  expect(parse(renderMarkdown('Done! [the notes](https://example.com)')).textContent).toBe('Done! the notes');
  // Bare links aren't touched either.
  expect(parse(renderMarkdown('Wow!https://example.com')).textContent).toContain('Wow!');
});

test("the document editor's tokens keep footnotes and images as written", () => {
  const src = 'A claim.[^1] And ![a picture](https://example.com/p.png)\n\n[^1]: The note.';
  const text = markdownTokens(src)
    .filter((t) => t.type === 'inline')
    .map((t) => t.children.map((c) => c.content).join(''))
    .join('\n');
  expect(text).toContain('[^1]');
  expect(text).toContain('[^1]: The note.');
  expect(text).toContain('!a picture');
});
