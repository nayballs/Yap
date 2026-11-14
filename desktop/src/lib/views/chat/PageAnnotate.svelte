<script>
  // **Annotating a page beside the chat** (14 Nov 2026; the Claude Code
  // app's pen, shapes and text, as the maintainer showed them): a still of
  // the page as it's in view (chat/pages/pick.rs `page_still`), shown in the
  // page's place while the live page steps aside, and drawn on — the pen,
  // a rectangle, an ellipse or an arrow by dragging, words where you click
  // (Enter puts them down, Esc drops them), Ctrl+Z takes the last mark back.
  // `picture()` lays the marks on the still at its own pixels, for the
  // message box (`page_annotated`).

  /**
   * `src`: the still (a `data:` PNG); `tool`: pen, shape or text; `shape`:
   * rect, ellipse or arrow; `onmarks(n)`: how many marks there are now.
   */
  let { src, tool = 'pen', shape = 'rect', onmarks = () => {} } = $props();

  let imgEl = $state(null);
  let canvas = $state(null);
  let wrap = $state(null);
  /** The marks, in the still's own pixels: `{kind, points}` / `{kind, a, b}` / `{kind:'text', at, text}`. */
  let marks = $state([]);
  let drawing = null;
  /** Words being typed: where (CSS px in the wrap, and the still's pixels) and what. */
  let typing = $state(null);
  let typeEl = $state(null);

  const colour = () => getComputedStyle(document.documentElement).getPropertyValue('--yap-danger').trim() || '#e5484d';

  /** CSS px → the still's pixels. */
  function toPx(e) {
    const r = canvas.getBoundingClientRect();
    const k = canvas.width / Math.max(1, r.width);
    return [(e.clientX - r.left) * k, (e.clientY - r.top) * k];
  }

  function lineWidth(w) {
    return Math.max(3, Math.round(w / 300));
  }

  function drawMark(ctx, m, w) {
    ctx.strokeStyle = colour();
    ctx.fillStyle = colour();
    ctx.lineWidth = lineWidth(w);
    ctx.lineCap = 'round';
    ctx.lineJoin = 'round';
    if (m.kind === 'pen') {
      ctx.beginPath();
      m.points.forEach(([x, y], i) => (i ? ctx.lineTo(x, y) : ctx.moveTo(x, y)));
      if (m.points.length === 1) ctx.lineTo(m.points[0][0] + 0.1, m.points[0][1]);
      ctx.stroke();
    } else if (m.kind === 'rect') {
      ctx.strokeRect(Math.min(m.a[0], m.b[0]), Math.min(m.a[1], m.b[1]), Math.abs(m.b[0] - m.a[0]), Math.abs(m.b[1] - m.a[1]));
    } else if (m.kind === 'ellipse') {
      ctx.beginPath();
      ctx.ellipse((m.a[0] + m.b[0]) / 2, (m.a[1] + m.b[1]) / 2, Math.abs(m.b[0] - m.a[0]) / 2, Math.abs(m.b[1] - m.a[1]) / 2, 0, 0, Math.PI * 2);
      ctx.stroke();
    } else if (m.kind === 'arrow') {
      const [x1, y1] = m.a;
      const [x2, y2] = m.b;
      const head = lineWidth(w) * 5;
      const ang = Math.atan2(y2 - y1, x2 - x1);
      ctx.beginPath();
      ctx.moveTo(x1, y1);
      ctx.lineTo(x2, y2);
      ctx.stroke();
      ctx.beginPath();
      ctx.moveTo(x2, y2);
      ctx.lineTo(x2 - head * Math.cos(ang - Math.PI / 7), y2 - head * Math.sin(ang - Math.PI / 7));
      ctx.lineTo(x2 - head * Math.cos(ang + Math.PI / 7), y2 - head * Math.sin(ang + Math.PI / 7));
      ctx.closePath();
      ctx.fill();
    } else if (m.kind === 'text') {
      const size = Math.max(16, Math.round(w / 45));
      ctx.font = `600 ${size}px system-ui, sans-serif`;
      ctx.textBaseline = 'top';
      // A light edge so the words read on any page.
      ctx.lineWidth = Math.max(3, size / 5);
      ctx.strokeStyle = 'rgba(255,255,255,0.9)';
      ctx.strokeText(m.text, m.at[0], m.at[1]);
      ctx.fillText(m.text, m.at[0], m.at[1]);
    }
  }

  function redraw() {
    if (!canvas) return;
    const ctx = canvas.getContext('2d');
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    for (const m of marks) drawMark(ctx, m, canvas.width);
    if (drawing) drawMark(ctx, drawing, canvas.width);
  }

  function loaded() {
    if (!imgEl || !canvas) return;
    canvas.width = imgEl.naturalWidth;
    canvas.height = imgEl.naturalHeight;
    redraw();
  }

  function down(e) {
    if (e.button !== 0 || typing) return;
    const p = toPx(e);
    if (tool === 'text') {
      const r = wrap.getBoundingClientRect();
      typing = { x: e.clientX - r.left, y: e.clientY - r.top, at: p, text: '' };
      queueMicrotask(() => typeEl?.focus());
      e.preventDefault();
      return;
    }
    canvas.setPointerCapture?.(e.pointerId);
    drawing = tool === 'pen' ? { kind: 'pen', points: [p] } : { kind: shape, a: p, b: p };
    redraw();
  }
  function move(e) {
    if (!drawing) return;
    const p = toPx(e);
    if (drawing.kind === 'pen') drawing.points.push(p);
    else drawing.b = p;
    redraw();
  }
  function up() {
    if (!drawing) return;
    const m = drawing;
    drawing = null;
    // (A click with no drag isn't a shape.)
    const tiny = m.kind !== 'pen' && Math.hypot(m.b[0] - m.a[0], m.b[1] - m.a[1]) < 4;
    if (!tiny) marks = [...marks, m];
    onmarks(marks.length);
    redraw();
  }

  function putText(keep) {
    const t = typing;
    typing = null;
    if (keep && t?.text.trim()) {
      marks = [...marks, { kind: 'text', at: t.at, text: t.text.trim() }];
      onmarks(marks.length);
    }
    redraw();
  }

  /** Ctrl+Z: the last mark back. */
  export function undo() {
    if (!marks.length) return false;
    marks = marks.slice(0, -1);
    onmarks(marks.length);
    redraw();
    return true;
  }

  /** The still with the marks on it, at its own pixels, as a `data:` PNG. */
  export function picture() {
    if (typing) putText(true);
    const out = document.createElement('canvas');
    out.width = imgEl.naturalWidth;
    out.height = imgEl.naturalHeight;
    const ctx = out.getContext('2d');
    ctx.drawImage(imgEl, 0, 0);
    for (const m of marks) drawMark(ctx, m, out.width);
    return out.toDataURL('image/png');
  }

  function keys(e) {
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'z' && !typing) {
      if (undo()) e.preventDefault();
    }
  }
</script>

<svelte:window onkeydown={keys} />

<div class="annot" bind:this={wrap} data-testid="chat-page-annotate" data-tool={tool} data-marks={marks.length}>
  <img bind:this={imgEl} {src} alt="The page as it was, to draw on" draggable="false" onload={loaded} />
  <canvas
    bind:this={canvas}
    class:texting={tool === 'text'}
    onpointerdown={down}
    onpointermove={move}
    onpointerup={up}
    onpointercancel={up}
    aria-label="Draw on the page"
    data-testid="chat-page-annotate-canvas"
  ></canvas>
  {#if typing}
    <input
      bind:this={typeEl}
      class="annottext"
      style:left="{typing.x}px"
      style:top="{typing.y}px"
      bind:value={typing.text}
      placeholder="Type, then Enter"
      aria-label="Words to put on the page"
      onkeydown={(e) => {
        if (e.key === 'Enter') {
          e.preventDefault();
          putText(true);
        } else if (e.key === 'Escape') {
          e.preventDefault();
          e.stopPropagation();
          putText(false);
        }
      }}
      onblur={() => putText(true)}
      data-testid="chat-page-annotate-text"
    />
  {/if}
</div>

<style>
  .annot {
    position: absolute;
    inset: 0;
    /* Over the stage's own layers (its "Loading the page…", a failure's words). */
    z-index: 2;
    overflow: hidden;
    background: var(--yap-paper);
  }
  .annot img,
  .annot canvas {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: fill;
    user-select: none;
  }
  .annot canvas {
    cursor: crosshair;
    touch-action: none;
  }
  .annot canvas.texting {
    cursor: text;
  }
  .annottext {
    position: absolute;
    min-width: 160px;
    padding: 2px 6px;
    border: 1px solid var(--yap-danger);
    border-radius: var(--yap-r-sm);
    background: var(--yap-paper-card);
    color: var(--yap-danger);
    font: 600 14px system-ui, sans-serif;
    outline: none;
  }
</style>
