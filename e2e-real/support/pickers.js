// A web page standing in for a chat app's or an AI chat box's message box
// with an "@" list (chat-messages.real.js, editor-files.real.js): what's typed
// after "@" opens a list of matching entries, Tab (or Enter while it's open)
// puts the highlighted one in as a pill, and Enter otherwise sends what's in
// the box into a list above it. Nothing is sent anywhere.
//
// The box keeps its spaces as typed (`white-space: pre-wrap`), as the
// editors chat apps are built on do (Slack's Quill, Discord's Slate,
// Lexical, ProseMirror): the space a picked mention puts after itself is
// then on screen, and in what a screen reader reads (without it, a trailing
// space collapses and only shows once text follows it).

/**
 * A message box with an "@" list of `items` (`{ label, insert }`), Enter
 * sending what's in it (`enterSends`; else Enter adds a line), the list's
 * highlighted entry told to a screen reader (`describe`) or not.
 */
export function pickerPage({ items, boxAttrs, enterSends = true, describe = true, aside = '' }) {
  return `${aside}
<style>#box{white-space:pre-wrap;min-height:60px;border:1px solid #888;background:#fff;padding:4px}.pill{background:#e8e0ff;border-radius:3px;padding:0 2px}#picker{border:1px solid #888;background:#fff;margin:0;padding:2px;list-style:none}#picker [aria-selected=true]{background:#cde}</style>
<ul id="sent" aria-label="Sent messages"></ul>
<div id="box" contenteditable="true" role="textbox" aria-multiline="true" aria-autocomplete="list" aria-haspopup="listbox" aria-expanded="false" aria-controls="picker" ${boxAttrs}></div>
<ul id="picker" role="listbox" aria-label="Suggestions" hidden></ul>
<script>
const ITEMS = ${JSON.stringify(items)};
const ENTER_SENDS = ${enterSends}, DESCRIBE = ${describe};
const box = document.getElementById('box'), picker = document.getElementById('picker'), sent = document.getElementById('sent');
let query = null;
const before = () => {
  const s = getSelection();
  if (!s.rangeCount) return '';
  const r = document.createRange();
  r.selectNodeContents(box);
  r.setEnd(s.getRangeAt(0).endContainer, s.getRangeAt(0).endOffset);
  return r.toString();
};
function close() {
  query = null;
  picker.hidden = true;
  picker.innerHTML = '';
  box.setAttribute('aria-expanded', 'false');
  box.removeAttribute('aria-activedescendant');
}
function open(q) {
  const lower = q.toLowerCase();
  const key = lower.replace(/[^a-z0-9]/g, '');
  const hits = ITEMS.filter((it) => it.label.toLowerCase().split(/[\\s.\\/]+/).some((w) => w.startsWith(lower)) || it.label.toLowerCase().replace(/[^a-z0-9]/g, '').startsWith(key));
  if (!hits.length) return close();
  query = q;
  picker.innerHTML = hits.map((it, i) => '<li role="option" id="opt' + i + '" aria-selected="' + (i === 0) + '" data-insert="' + it.insert + '">' + it.label + '</li>').join('');
  picker.hidden = false;
  box.setAttribute('aria-expanded', 'true');
  if (DESCRIBE) box.setAttribute('aria-activedescendant', 'opt0');
}
box.addEventListener('input', () => {
  const m = /@([\\w.]*)$/.exec(before());
  if (m) open(m[1]);
  else close();
});
function pick() {
  const opt = picker.querySelector('[aria-selected="true"]');
  const last = box.lastChild;
  const typed = '@' + query;
  if (!opt || !last || last.nodeType !== 3 || !last.data.endsWith(typed)) return close();
  last.data = last.data.slice(0, -typed.length);
  const pill = document.createElement('span');
  pill.className = 'pill';
  pill.contentEditable = 'false';
  pill.textContent = opt.dataset.insert;
  const space = document.createTextNode(' ');
  box.append(pill, space);
  const r = document.createRange();
  r.setStart(space, 1);
  r.collapse(true);
  const s = getSelection();
  s.removeAllRanges();
  s.addRange(r);
  close();
}
function send() {
  const text = box.innerText.trim();
  if (!text) return;
  const li = document.createElement('li');
  li.textContent = text;
  sent.append(li);
  box.innerHTML = '';
}
box.addEventListener('keydown', (e) => {
  if ((e.key === 'Tab' || e.key === 'Enter') && query !== null) {
    e.preventDefault();
    pick();
    return;
  }
  if (e.key === 'Enter' && !e.shiftKey && ENTER_SENDS) {
    e.preventDefault();
    send();
  }
});
</script>`;
}
