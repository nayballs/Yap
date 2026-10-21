// src/lib/markdown.js on its own (no app needed): the lists in chat answers,
// notes and update notes. Models write numbered lists with blank lines
// between the items ("1. …\n\n2. …"), which showed "1." on every item until
// 2026-10-21 (each item was a list of its own).
import { test, expect } from '@playwright/test';
import { renderMarkdown } from '../src/lib/markdown.js';

const tags = (html) => html.match(/<\/?(?:ol|ul|li)\b[^>]*>/g) ?? [];

test('a blank line between items keeps one list, numbered as written', () => {
  const html = renderMarkdown('Here are three:\n\n1. **Starfall Requiem**\n\n2. **Ashen Crown II**\n\n3. **Tidebound Saga**\n\nWant more?');
  expect(tags(html)).toEqual(['<ol>', '<li>', '</li>', '<li>', '</li>', '<li>', '</li>', '</ol>']);
  expect(html).toContain('<p>Here are three:</p>');
  expect(html).toContain('<li><strong>Ashen Crown II</strong></li>');
  expect(html).toMatch(/<\/ol>\n<p>Want more\?<\/p>$/);
  // Bullets too.
  expect(tags(renderMarkdown('- one\n\n- two\n\n* three'))).toEqual(['<ul>', '<li>', '</li>', '<li>', '</li>', '<li>', '</li>', '</ul>']);
});

test('a list split by a paragraph starts again at its own number', () => {
  const html = renderMarkdown('1. one\n2. two\n\nAnd then:\n\n3. three\n4. four');
  expect(tags(html)).toEqual(['<ol>', '<li>', '</li>', '<li>', '</li>', '</ol>', '<ol start="3">', '<li>', '</li>', '<li>', '</li>', '</ol>']);
  expect(renderMarkdown('5. five')).toBe('<ol start="5">\n<li>five</li></ol>');
});

test('lines and lists indented under an item belong to it', () => {
  const html = renderMarkdown(
    '1. **Starfall Requiem** [1]\n   A sci-fi party RPG.\n   - PC and consoles\n   - 12 March\n\n2. **Ashen Crown II**\n\n   The sequel.'
  );
  expect(tags(html)).toEqual(['<ol>', '<li>', '<ul>', '<li>', '</li>', '<li>', '</li>', '</ul>', '</li>', '<li>', '</li>', '</ol>']);
  expect(html).toContain('<li><strong>Starfall Requiem</strong> [1]<br>A sci-fi party RPG.');
  expect(html).toContain('<li><strong>Ashen Crown II</strong><br><br>The sequel.</li></ol>');
  // A different kind of list at the same level is a list of its own.
  expect(tags(renderMarkdown('1. one\n- bullet'))).toEqual(['<ol>', '<li>', '</li>', '</ol>', '<ul>', '<li>', '</li>', '</ul>']);
  // Tasks keep their checkboxes; a heading ends the list.
  const tasks = renderMarkdown('- [ ] ship it\n\n- [x] test it\n## Next');
  expect(tasks).toContain('<li class="task"><input type="checkbox" disabled /> ship it');
  expect(tasks).toContain('<li class="task"><input type="checkbox" disabled checked /> test it</li></ul>\n<h3>Next</h3>');
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
});
