// The microphones' order (Settings → Dictation → Microphone, onboarding's
// mic check): the same rules as dictation/mics.rs, for the page's own copy
// of the order while it's being edited. A choice is `{ id, name }` (`id`
// the Core Audio endpoint id; empty for one kept by name only); Windows'
// list (`list_mics`) is `[{ id, name, bluetooth, default, virtual }]`.

/** Does `choice` name `ep`? By its id; else (no id, or an id nobody has now) by its name. */
export function matches(choice, ep, present) {
  if (!choice || !ep) return false;
  if (choice.id && choice.id === ep.id) return true;
  const idHere = !!choice.id && present.some((p) => p.id === choice.id);
  return !idHere && !!choice.name?.trim() && choice.name === ep.name;
}

/** The connected microphone `choice` names, or null. */
export function findMic(choice, present) {
  return present.find((e) => choice?.id && e.id === choice.id) || present.find((e) => matches(choice, e, present)) || null;
}

/** The microphone a dictation starting now uses: the first in `order` that's connected, else Windows' default. */
export function micInUse(order, present) {
  for (const c of order || []) {
    const e = findMic(c, present);
    if (e) return e;
  }
  return present.find((e) => e.default) || present[0] || null;
}

/** The telling part of a name: "Headset Microphone (Contoso H2)" → "Contoso H2". */
export function shortMicName(name) {
  let n = String(name || '').trim();
  const m = n.match(/ \((.+)\)$/);
  if (m && m[1].trim()) n = m[1].trim();
  n = n.replace(/ Hands-Free( AG)?( Audio)?$/, '').trim();
  return n || 'Your microphone';
}
