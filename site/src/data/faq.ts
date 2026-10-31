// Every question and answer on the site, in one list. Each page shows the
// ones tagged for it; /faq shows them all, by group. Answers are HTML.
// Only what the app does today: anything else is marked as coming.

import { NAME, HOTKEY, DOWNLOAD_NIGHTLY } from '../brand';
import { key } from '../lib/html';

export type Page = 'home' | 'dictation' | 'meetings' | 'assistant' | 'privacy' | 'download' | 'pricing';

export interface Faq {
  id: string;
  group: 'Getting started' | 'Dictation' | 'Meetings' | 'Chat and the assistant' | 'Privacy' | 'Price';
  q: string;
  a: string;
  pages: Page[];
}

export const FAQ: Faq[] = [
  // ---------------------------------------------------------- getting started
  {
    id: 'gpu',
    group: 'Getting started',
    q: 'Do I need a powerful graphics card?',
    a: `<p>No. Speech models run on any graphics card, NVIDIA, AMD or Intel, through Vulkan and DirectML, or on the processor. ${NAME} times both on your PC and uses whichever is faster; Settings shows which it chose and why. There’s nothing extra to install: no CUDA, and no Copilot+ PC needed.</p>`,
    pages: ['home', 'dictation', 'download'],
  },
  {
    id: 'smartscreen',
    group: 'Getting started',
    q: 'Why does Windows warn me when I install it?',
    a: `<p>The installer isn’t code-signed yet, so the first time you run it Windows SmartScreen says “Windows protected your PC”. Choose <strong>More info</strong>, then <strong>Run anyway</strong>. Signing is on its way. Until then, new releases list the installer’s SHA-256 checksum, so you can check that the file is the one GitHub built.</p>`,
    pages: ['home', 'download'],
  },
  {
    id: 'account',
    group: 'Getting started',
    q: 'Do I need an account?',
    a: `<p>No. Download, install and use ${NAME} without one: no sign-up and no email. There’s an optional account; a signed-out ${NAME} never contacts the account service on its own.</p>`,
    pages: ['home', 'download', 'privacy'],
  },
  {
    id: 'platforms',
    group: 'Getting started',
    q: 'Is there a version for Mac, Linux or my phone?',
    a: `<p>Not yet: ${NAME} is for Windows 10 and 11. Linux and macOS are planned, with no dates. Your phone can already talk to ${NAME} on your PC while you’re both on your home Wi-Fi (${NAME} Link, a preview). <a href="/download#other-platforms">Where each platform stands</a>.</p>`,
    pages: ['home', 'download'],
  },
  {
    id: 'updates',
    group: 'Getting started',
    q: 'How do updates work?',
    a: `<p>${NAME} checks GitHub for a new version about every four hours, downloads it in the background, and installs it when you choose <strong>Restart to update</strong>. You can turn the checks off in Settings.</p>`,
    pages: ['download'],
  },
  {
    id: 'channels',
    group: 'Getting started',
    q: 'Stable or nightly?',
    a: `<p><strong>Stable</strong> is the tested release, and it’s what the download buttons give you. <strong>Nightly</strong> is built every day from the newest code: you get changes as soon as they land, and the occasional rough edge. Each keeps itself up to date on its own channel. <a href="${DOWNLOAD_NIGHTLY}">Download the nightly</a>.</p>`,
    pages: ['download'],
  },
  {
    id: 'uninstall',
    group: 'Getting started',
    q: 'How do I uninstall it, and does that remove my data?',
    a: `<p>Uninstall it from Windows’ <strong>Installed apps</strong>. Tick <strong>Delete the application data</strong> to remove your history, notes, settings and the saved sign-in as well; leave it unticked to keep them for a reinstall.</p>`,
    pages: ['download'],
  },

  // ---------------------------------------------------------------- dictation
  {
    id: 'apps',
    group: 'Dictation',
    q: 'Which apps can I dictate into?',
    a: `<p>Any app with a text box: email, chat apps, documents, browsers, code editors and terminals. ${NAME} pastes the text where your cursor is and then puts your clipboard back. Windows won’t let other apps paste into a program running as administrator, so there ${NAME} leaves the text on your clipboard and tells you.</p>`,
    pages: ['home', 'dictation'],
  },
  {
    id: 'languages',
    group: 'Dictation',
    q: 'Which languages does it understand?',
    a: `<p>The Whisper models know about a hundred languages and say which one they heard. Parakeet V3 covers 25 European languages; other models cover English, Chinese, Japanese, Korean or Russian. Tell ${NAME} the languages you speak and it suggests a model that covers them. AI cleanup keeps your text in the language you spoke.</p>`,
    pages: ['home', 'dictation'],
  },
  {
    id: 'modes',
    group: 'Dictation',
    q: 'Do I press the key once, or hold it?',
    a: `<p>Either. Press ${key(HOTKEY)} to start and again to stop, or hold it while you talk and let go when you’re done. Double-tap it to talk hands-free. The shortcut can be any key, a combination or a mouse button.</p>`,
    pages: ['dictation'],
  },
  {
    id: 'cleanup',
    group: 'Dictation',
    q: 'What does AI cleanup change, and is it on?',
    a: `<p>It’s off until you choose a model. Then it takes out the ums, fixes punctuation and grammar, and keeps the version you meant when you change your mind mid-sentence. You choose how much it changes (Light, Medium or High), and its style follows the app: more formal in email, casual in chat, literal in AI prompts, exactly as said in a terminal. <strong>Use what I said</strong> swaps in your exact words whenever you want them.</p>`,
    pages: ['dictation'],
  },
  {
    id: 'words',
    group: 'Dictation',
    q: 'Can it learn names and jargon?',
    a: `<p>Yes. Add a word to the dictionary once, with the ways it gets misheard, and ${NAME} fixes it every time, near misses included. When you correct a word after a dictation, ${NAME} can offer to add it for you. Snippets turn a short phrase you say into a longer text you use often.</p>`,
    pages: ['dictation'],
  },
  {
    id: 'offline',
    group: 'Dictation',
    q: 'Does it work offline?',
    a: `<p>Dictation does: speech becomes text on your PC, and so does AI cleanup when you use the model on your PC. What needs the internet, a cloud model, web search, ElevenLabs’ voices or calendar sync, waits until you’re back online.</p>`,
    pages: ['dictation', 'privacy'],
  },

  // ----------------------------------------------------------------- meetings
  {
    id: 'bot',
    group: 'Meetings',
    q: 'Does a bot join my call?',
    a: `<p>No. ${NAME} records your microphone and the sound your PC plays, on your PC, so nobody in the call sees a bot. Do tell people you’re taking notes: whether you need their agreement depends on where you are.</p>`,
    pages: ['meetings'],
  },
  {
    id: 'call-apps',
    group: 'Meetings',
    q: 'Which call apps does it notice?',
    a: `<p>Teams, Zoom, Google Meet, Slack huddles, Webex, GoTo and others. ${NAME} asks before it takes notes, and you can tell it what to do per app. You can also start notes yourself with ${key('Win+Alt+M')}, for a call it didn’t notice or a meeting in the room.</p>`,
    pages: ['meetings'],
  },
  {
    id: 'audio-kept',
    group: 'Meetings',
    q: 'Where is the audio kept, and for how long?',
    a: `<p>On your PC. Each meeting’s audio is kept for 14 days by default, so you can play a line back or transcribe it again, and then it’s deleted. You can also have transcripts deleted after 30, 90 or 365 days; your notes, summary and tasks stay.</p>`,
    pages: ['meetings', 'privacy'],
  },
  {
    id: 'speakers',
    group: 'Meetings',
    q: 'Can it tell who said what?',
    a: `<p>In beta. ${NAME} separates voices on your PC and suggests names from the invite and from what’s said; you confirm them. Voiceprints are made from that meeting’s own audio and never kept.</p>`,
    pages: ['meetings'],
  },

  // ------------------------------------------------------ chat and the assistant
  {
    id: 'listening',
    group: 'Chat and the assistant',
    q: 'Is it listening all the time?',
    a: `<p>No. There’s no wake word. The microphone opens when you start a dictation, a question or meeting notes, and closes a minute after (straight away on a Bluetooth headset), unless you ask ${NAME} to keep it ready. Windows shows when it’s in use.</p>`,
    pages: ['assistant', 'privacy'],
  },
  {
    id: 'models',
    group: 'Chat and the assistant',
    q: 'Can I use models from OpenAI, Anthropic or Google?',
    a: `<p>Yes, with your own API key from OpenAI, Anthropic, Google (Gemini), Groq or OpenRouter: you pay them directly and ${NAME} adds nothing. Or run a model on your PC, or on your own Ollama or LM Studio server.</p>`,
    pages: ['assistant', 'pricing'],
  },
  {
    id: 'voices',
    group: 'Chat and the assistant',
    q: 'Which voices does it answer in?',
    a: `<p>Kokoro voices that run on your PC (a one-time download), Windows’ own voices, or ElevenLabs with your own key.</p>`,
    pages: ['assistant'],
  },
  {
    id: 'link',
    group: 'Chat and the assistant',
    q: `What is ${NAME} Link?`,
    a: `<p>A preview that lets you ask ${NAME} from your phone while you walk round the house, the answer in your ear. Your phone opens a page your PC serves over your home Wi-Fi; you pair it once with a QR code and a four-digit code. It’s off until you switch it on, it answers only devices on your home network, and it refuses connections on networks Windows marks as Public.</p>`,
    pages: ['assistant'],
  },

  // ------------------------------------------------------------------ privacy
  {
    id: 'leaves',
    group: 'Privacy',
    q: 'Does anything leave my PC?',
    a: `<p>Your voice never does: speech becomes text on your own graphics card or processor. Text goes online only for things you switch on: a cloud AI model with your own key, web search, a calendar you connect, ElevenLabs’ voices, or the optional account. Settings lists every connection ${NAME} made, by purpose and host. <a href="/privacy#what-leaves">What leaves your PC</a>.</p>`,
    pages: ['home', 'privacy'],
  },
  {
    id: 'telemetry',
    group: 'Privacy',
    q: 'Does it collect analytics or crash reports?',
    a: `<p>No. There’s no telemetry, no analytics and no automatic crash report. When something goes wrong, <strong>Report a problem</strong> makes a diagnostic file that you read before you save it; nothing is uploaded.</p>`,
    pages: ['privacy'],
  },

  // -------------------------------------------------------------------- price
  {
    id: 'free',
    group: 'Price',
    q: 'Is it really free?',
    a: `<p>Yes. Everything that runs on your PC is free, with no word limit and no meeting limit, and the code is MIT-licensed. If a paid plan ever comes, it will sell only things that run on servers. <a href="/pricing">How pricing works</a>.</p>`,
    pages: ['home', 'pricing'],
  },
  {
    id: 'worse',
    group: 'Price',
    q: 'Will the free version get worse?',
    a: `<p>No. Nothing on the free list will move to a paid plan, and ${NAME} will never stop working offline because of an account.</p>`,
    pages: ['pricing'],
  },
  {
    id: 'work',
    group: 'Price',
    q: 'Can I use it at work?',
    a: `<p>Yes. The MIT licence allows commercial use. There are no seats to buy and no admin console.</p>`,
    pages: ['pricing'],
  },
  {
    id: 'discounts',
    group: 'Price',
    q: 'Do students or charities pay less?',
    a: `<p>Nobody pays for anything that runs on your PC, so there’s nothing to discount.</p>`,
    pages: ['pricing'],
  },
];

export const faqFor = (page: Page) => FAQ.filter((f) => f.pages.includes(page));
