// Saved transforms (transforms.rs): the list in config (`transforms`), Yap's
// five built-ins as Settings shows them, the languages Translate offers, and
// the shortcut rules a transform's key is checked against. The built-ins'
// instructions are Rust's (`builtin_instruction`): here only their names
// and one line each.

/** The most transforms the list keeps (transforms.rs `MAX`). */
export const MAX_TRANSFORMS = 12;

/** Translate's language until one is picked (transforms.rs `DEFAULT_LANGUAGE`). */
export const DEFAULT_LANGUAGE = 'Spanish';

/** Yap's own, in their first order: name and the line under it. */
export const BUILTINS = {
  polish: { name: 'Polish', line: 'Fixes grammar and smooths the wording, keeping your tone and length' },
  shorter: { name: 'Shorter', line: 'Says the same in about half the words, every fact kept' },
  grammar: { name: 'Fix grammar only', line: 'Fixes mistakes and changes nothing else' },
  translate: { name: 'Translate', line: 'Names, numbers, emails and links kept as they are' },
  prompt: { name: 'Make it a prompt', line: 'Turns your text into a clear prompt for an AI, without answering it' },
};

/** The list a config without one starts with (config.rs's default). */
export function defaultTransforms() {
  return Object.keys(BUILTINS).map((kind) => ({
    id: kind,
    builtin: kind,
    name: '',
    instruction: '',
    hotkey: '',
    language: kind === 'translate' ? DEFAULT_LANGUAGE : '',
  }));
}

/** Languages Translate offers (their English names go into the instruction). */
export const LANGUAGES = [
  'Arabic',
  'Bengali',
  'Chinese (Simplified)',
  'Chinese (Traditional)',
  'Czech',
  'Danish',
  'Dutch',
  'English',
  'Filipino',
  'Finnish',
  'French',
  'German',
  'Greek',
  'Hebrew',
  'Hindi',
  'Hungarian',
  'Indonesian',
  'Irish',
  'Italian',
  'Japanese',
  'Korean',
  'Malay',
  'Norwegian',
  'Polish',
  'Portuguese',
  'Romanian',
  'Russian',
  'Spanish',
  'Swahili',
  'Swedish',
  'Thai',
  'Turkish',
  'Ukrainian',
  'Urdu',
  'Vietnamese',
  'Welsh',
];

/** The name people see: "Translate to French", a built-in's own, or theirs. */
export function transformName(t) {
  if (!t) return '';
  if (t.builtin === 'translate') return `Translate to ${(t.language || '').trim() || DEFAULT_LANGUAGE}`;
  if (t.builtin) return BUILTINS[t.builtin]?.name ?? 'Transform';
  return (t.name || '').trim() || 'Untitled transform';
}

/** A new transform of the person's own. */
export function newTransform(name, instruction) {
  return {
    id: `t-${Date.now().toString(36)}${Math.random().toString(36).slice(2, 5)}`,
    builtin: '',
    name: name.trim().slice(0, 60),
    instruction: instruction.trim(),
    hotkey: '',
    language: '',
  };
}
