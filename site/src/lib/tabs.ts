// Tabs for the live demos: [data-tabs] holding [role=tab] buttons, each with
// aria-controls naming its [role=tabpanel]. Arrow keys, Home and End move
// between tabs. Panels show by [data-active] (scenes.css hides the others
// behind .js, so without script every panel shows). `data-tabs-auto="ms"`
// moves on by itself while the tabs are on screen, until someone picks one,
// never with reduced motion. Each change fires `tabchange` on the root.

export function initTabs(root: HTMLElement) {
  const tabs = [...root.querySelectorAll<HTMLButtonElement>('[role="tab"]')];
  const panels = tabs.map((t) => document.getElementById(t.getAttribute('aria-controls') ?? ''));
  let current = Math.max(0, tabs.findIndex((t) => t.getAttribute('aria-selected') === 'true'));
  let auto = Number(root.dataset.tabsAuto ?? 0);
  if (matchMedia('(prefers-reduced-motion: reduce)').matches) auto = 0;
  let timer = 0;
  let visible = false;

  const schedule = () => {
    clearTimeout(timer);
    if (auto && visible) timer = window.setTimeout(() => select(current + 1), auto);
  };

  const select = (i: number, focus = false) => {
    current = (i + tabs.length) % tabs.length;
    tabs.forEach((t, j) => {
      const on = j === current;
      t.setAttribute('aria-selected', String(on));
      t.tabIndex = on ? 0 : -1;
      panels[j]?.toggleAttribute('data-active', on);
    });
    if (focus) tabs[current].focus();
    root.dispatchEvent(new CustomEvent('tabchange', { detail: { index: current, panel: panels[current] } }));
    schedule();
  };

  const stopAuto = () => {
    auto = 0;
    clearTimeout(timer);
  };

  tabs.forEach((t, i) => {
    t.tabIndex = i === current ? 0 : -1;
    t.addEventListener('click', () => {
      stopAuto();
      select(i);
    });
    t.addEventListener('keydown', (e) => {
      const moves: Record<string, number> = {
        ArrowRight: i + 1,
        ArrowDown: i + 1,
        ArrowLeft: i - 1,
        ArrowUp: i - 1,
        Home: 0,
        End: tabs.length - 1,
      };
      if (e.key in moves) {
        e.preventDefault();
        stopAuto();
        select(moves[e.key], true);
      }
    });
  });

  new IntersectionObserver(
    ([e]) => {
      const was = visible;
      visible = e.isIntersecting;
      if (visible && !was) select(current);
      if (!visible) clearTimeout(timer);
    },
    { threshold: 0.35 },
  ).observe(root);
}
