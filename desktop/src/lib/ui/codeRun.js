// **▶ Run on a one-line shell block** of a Code chat's answer (15 Nov 2026,
// after the Claude app's Code tab): a Svelte action beside `colourCode` on
// the element an answer's Markdown is rendered into.
//
//   <div class="cbody" use:codeRun={m.streaming ? null : files}>{@html …}</div>
//
// - **Which blocks**: a fence that names a shell (`bash`, `sh`, `shell`,
//   `console`, `zsh`, `powershell`, `pwsh`, `ps1`, `cmd`, `bat`) holding one
//   line (a `$ ` prompt before it allowed: agents/run_block.rs `one_line`
//   decides). Never in a Chat (non-Code) chat (`files` null), never while
//   the answer is still being written (its line may be half there).
// - **The click** hands the block's own text (the code element's, which
//   colouring never changes) and its language to `files.run(...)`
//   (ChatView.svelte: asked first but in Bypass, then run in the workspace,
//   its output in the Runs tab beside the chat).
// - The button sits at the block's right, left of its Copy (app.css
//   `.md-coderun`).

/** The fence names that run, by the shell agents/run_block.rs `shell_for` gives them. */
export const RUN_LANGS = ['bash', 'sh', 'shell', 'console', 'zsh', 'terminal', 'powershell', 'pwsh', 'ps1', 'ps', 'cmd', 'bat', 'batch'];

const ICON = '<svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M8 5.5v13a1 1 0 0 0 1.52.85l10.4-6.5a1 1 0 0 0 0-1.7L9.52 4.65A1 1 0 0 0 8 5.5z"/></svg>';

/** A code element's fence language as written (`lang-bash` → "bash"), lower case; '' for none. Pure. */
export function blockLang(className) {
  return (
    String(className || '')
      .split(/\s+/)
      .find((c) => c.startsWith('lang-'))
      ?.slice(5)
      .toLowerCase() ?? ''
  );
}

/** Whether a block of `text` in `lang` gets ▶: a shell's, and one line. Pure. */
export function runnable(text, lang) {
  if (!RUN_LANGS.includes(String(lang || '').toLowerCase())) return false;
  const lines = String(text || '')
    .replace(/\r/g, '')
    .split('\n')
    .filter((l) => l.trim());
  return lines.length === 1 && lines[0].trim().replace(/^(\$|>|PS>)\s*/, '').length > 0 && lines[0].length <= 4000;
}

function addButton(pre, code) {
  pre.dataset.run = '';
  const b = document.createElement('button');
  b.type = 'button';
  b.className = 'md-coderun';
  b.title = 'Run in the workspace';
  b.setAttribute('aria-label', 'Run in the workspace');
  b.dataset.testid = 'md-code-run';
  b.innerHTML = ICON;
  pre.append(b);
  return code;
}

export function codeRun(node, files) {
  let ctx = files;
  const look = () => {
    if (ctx?.conversation == null || !ctx?.run) return;
    for (const pre of node.querySelectorAll('pre.md-code:not([data-run])')) {
      const code = pre.querySelector('code');
      if (!code) continue;
      if (runnable(code.textContent, blockLang(code.className))) addButton(pre, code);
      else pre.dataset.run = 'no';
    }
  };
  look();
  const seen = new MutationObserver(look);
  seen.observe(node, { childList: true, subtree: true });
  const click = (e) => {
    const b = e.target instanceof Element ? e.target.closest('button.md-coderun') : null;
    if (!b || !node.contains(b)) return;
    e.preventDefault();
    e.stopPropagation();
    const code = b.closest('pre.md-code')?.querySelector('code');
    if (code && ctx?.run) ctx.run({ command: code.textContent.replace(/\n$/, ''), lang: blockLang(code.className) });
  };
  node.addEventListener('click', click);
  return {
    update(next) {
      ctx = next;
      look();
    },
    destroy() {
      seen.disconnect();
      node.removeEventListener('click', click);
    },
  };
}
