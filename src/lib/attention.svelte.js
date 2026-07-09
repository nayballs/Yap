// Settings "needs attention" badge (Wispr-style red count on the Settings
// cog + per-section rows). Settings.svelte computes the items — it is always
// mounted in the main window, so the state is live even while the modal is
// closed — and ControlPanel + the Settings nav render them.
//
// Each item: { section: <Settings section id>, label: <short human reason> }.
export const attention = $state({ items: [] });

export function attentionCount(section = null) {
  if (!section) return attention.items.length;
  return attention.items.filter((i) => i.section === section).length;
}
