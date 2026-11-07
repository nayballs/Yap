// **The app's navigation as a rail** (2026-11-07; the maintainer, in Chat's
// Code side full screen: "hide the left-hand side stuff… more space for the
// code and the thread"): the main window's sidebar (Home, Insights, Chat…,
// Settings and the account at its foot) collapses to a column of icons, in
// every view, after Windows' own NavigationView: its ☰ at the top, a label
// as a tooltip on each icon, never opened over the content by a hover.
// Pure, no Svelte state: ControlPanel draws it, stores/navRail.svelte.js
// keeps its state, and Chat's room (views/chat/chatRoom.js) asks it to give
// way first when a panel needs the width.

/** The rail's whole width expanded (its 204 px of rows and their margins: the sidebar as it always was). */
export const RAIL_WIDE = 220;
/** Collapsed: one 40 px icon and its margins. */
export const RAIL_NARROW = 56;
/** This window's storage: "1" while the person keeps it collapsed (expanded, the first run's, otherwise). */
export const RAIL_KEY = 'yapNavCollapsed';
/** Its shortcut, as the sheet and the tooltips write it (Notion's and Figma's sidebar key). */
export const RAIL_SHORTCUT = 'Ctrl+\\';

/**
 * The rail's button (and Ctrl+\): the next `{ collapsed, keptOpen }` from
 * the rail as it is now. Narrow → expanded, the person's own (remembered);
 * expanded over a view that had narrowed it for the room (`auto`), it stays
 * open for as long as that lasts (`keptOpen`). Expanded → collapsed, the
 * person's own.
 */
export function railToggle({ narrow, auto = false }) {
  if (narrow) return { collapsed: false, keptOpen: !!auto };
  return { collapsed: true, keptOpen: false };
}

/**
 * Ctrl+\ — by its character, by its place (a layout whose \ needs AltGr
 * keeps the key that's there on a US keyboard), or by its key code (a page
 * beside the chat's keys come as `yap-page-key`, a key code alone).
 */
export function isRailKey(e) {
  if (!e || !(e.ctrlKey || e.metaKey) || e.shiftKey || e.altKey) return false;
  return e.key === '\\' || e.code === 'Backslash' || (!e.key && e.keyCode === 220);
}
