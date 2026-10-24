// Styles by app (src-tauri/src/cleanup_styles.rs): what each kind of app's
// tones look like on the page — a line per tone and a before/after example
// per kind and tone (Wispr Flow's example cards, by behaviour) — and the
// helpers Settings' styles (settings/CleanupStyles.svelte) and onboarding's
// preview share. The kinds, their tones and built-in apps come from Rust
// (`cleanup_styles_catalog`); the examples are written to match what the
// on-device Qwen3.5 4B wrote for them in the model evaluations
// (docs/model-evals.md "Styles by app"). People are fictional.

/** One line under a tone's picker. */
export const TONE_LINES = {
  formal: 'Complete sentences, no contractions, no slang',
  casual: 'Everyday words and contractions, light punctuation',
  veryCasual: 'All lower case, light punctuation, no full stop at the end',
  natural: 'Your own words, tidied: fillers out, punctuation in',
  literal: 'Your words in your order: nothing added, nothing rephrased, never answered',
  command: 'Exactly as said: fillers out, no capital or full stop added',
  off: 'No AI here: the words exactly as you said them',
};

/** Per kind: what was said, and what Yap types in each tone. */
export const EXAMPLES = {
  email: {
    said: "hi sarah um thanks for your time today i'll send the updated proposal by thursday let me know if you need anything before then thanks",
    tones: {
      formal:
        'Hi Sarah,\n\nThank you for your time today. I will send the updated proposal by Thursday. Please let me know if you need anything before then.\n\nThanks',
      casual:
        "Hi Sarah,\n\nThanks for your time today. I'll send the updated proposal by Thursday, let me know if you need anything before then.\n\nThanks",
      natural:
        "Hi Sarah, thanks for your time today. I'll send the updated proposal by Thursday. Let me know if you need anything before then. Thanks.",
    },
  },
  workChat: {
    said: 'hey can you send me the deck before the call um i think the numbers on slide three are out of date no rush',
    tones: {
      formal: 'Hi, could you send me the deck before the call? I think the numbers on slide three are out of date. There is no rush.',
      casual: 'Hey, can you send me the deck before the call? I think the numbers on slide three are out of date. No rush.',
      veryCasual: 'hey can you send me the deck before the call? i think the numbers on slide three are out of date, no rush',
    },
  },
  personalChat: {
    said: 'hey are you around this evening i was thinking we could grab a coffee around six let me know',
    tones: {
      formal: 'Hello, are you available this evening? I was thinking we could have a coffee at around six. Let me know.',
      casual: 'Hey, are you around this evening? I was thinking we could grab a coffee around six. Let me know.',
      veryCasual: 'hey are you around this evening? i was thinking we could grab a coffee around six, let me know',
    },
  },
  documents: {
    said: "the main risk is um the timeline we can't ship the new onboarding until the payments work is done so we should plan for that",
    tones: {
      formal:
        'The main risk is the timeline: we cannot ship the new onboarding until the payments work is complete, so we should plan for that.',
      casual: "The main risk is the timeline. We can't ship the new onboarding until the payments work is done, so we should plan for that.",
      natural:
        "The main risk is the timeline: we can't ship the new onboarding until the payments work is done, so we should plan for that.",
    },
  },
  aiPrompts: {
    said: 'um can you write a python function that uh reverses a string and add some tests',
    tones: {
      literal: 'Can you write a Python function that reverses a string and add some tests?',
      natural: 'Can you write a Python function that reverses a string, and add some tests?',
    },
  },
  terminal: {
    said: 'um git push origin main',
    tones: {
      command: 'git push origin main',
      off: 'Um, git push origin main.',
    },
  },
  other: {
    said: 'so um the thing is we should probably move the meeting to next week because uh half the team is away',
    tones: {
      natural: 'So the thing is, we should probably move the meeting to next week because half the team is away.',
      formal: 'We should probably move the meeting to next week, as half the team is away.',
      casual: "Thing is, we should probably move the meeting to next week, half the team's away.",
    },
  },
};

/** An app's name as Yap compares it: lower case, no ".exe". */
export function norm(name) {
  return String(name || '')
    .trim()
    .toLowerCase()
    .replace(/\.exe$/, '');
}

/** `entry` ("ms-teams.exe", "Teams", "Gmail") is the built-in `app` ({ name, match }). */
export function isApp(entry, app) {
  const e = norm(entry);
  return !!e && (norm(app.name) === e || (app.match || []).includes(e));
}

/** A kind's apps as the person has them: the built-in ones kept, then those added. */
export function appsOf(kind, profile) {
  const removed = profile?.removedApps || [];
  return [
    ...(kind.apps || []).filter((a) => !removed.some((r) => norm(r) === norm(a.name))).map((a) => ({ name: a.name, builtIn: true })),
    ...(profile?.apps || []).map((name) => ({ name, builtIn: false })),
  ];
}

/** "Outlook, Gmail, Thunderbird and 10 more" (or "No apps yet"). */
export function appsSummary(names, shown = 3) {
  if (!names.length) return 'No apps';
  if (names.length <= shown + 1) return names.length === 1 ? names[0] : `${names.slice(0, -1).join(', ')} and ${names.at(-1)}`;
  return `${names.slice(0, shown).join(', ')} and ${names.length - shown} more`;
}

/**
 * Add `name` to `kind` (an app's process name or a site), taking it out of
 * every other kind: a built-in one there is taken out, one added there
 * dropped; here a built-in one taken out comes back. `profiles`: the kinds'
 * profiles by kind id. Mutates them (Settings' copy auto-saves).
 */
export function addApp(catalog, profiles, kind, name) {
  const n = String(name || '').trim();
  if (!n) return;
  for (const k of catalog) {
    const p = profiles[k.id];
    if (!p) continue;
    p.apps = p.apps || [];
    p.removedApps = p.removedApps || [];
    const builtIn = (k.apps || []).find((a) => isApp(n, a));
    if (k.id === kind.id) {
      if (builtIn) p.removedApps = p.removedApps.filter((r) => norm(r) !== norm(builtIn.name));
      else if (!p.apps.some((a) => norm(a) === norm(n))) p.apps = [...p.apps, n];
    } else {
      if (builtIn && !p.removedApps.some((r) => norm(r) === norm(builtIn.name))) p.removedApps = [...p.removedApps, builtIn.name];
      p.apps = p.apps.filter((a) => norm(a) !== norm(n));
    }
  }
}

/** Take `app` ({ name, builtIn }) out of its kind's profile. */
export function removeApp(profile, app) {
  if (app.builtIn) {
    profile.removedApps = [...(profile.removedApps || []), app.name];
  } else {
    profile.apps = (profile.apps || []).filter((a) => a !== app.name);
  }
}

/** A new profile for `kind` (Rust makes them at load; a copy from before has none). */
export function newStyleProfile(kind) {
  return {
    id: kind.profileId,
    name: kind.name,
    prompt: '',
    provider: '',
    baseUrl: '',
    model: '',
    apiKey: '',
    category: kind.id,
    tone: kind.defaultTone,
    apps: [],
    removedApps: [],
  };
}
