// "What leaves your PC": one row per thing the app does, where it happens,
// and what goes online (and only when you chose it). Kept in step with the
// app's Settings → Privacy & data and the README. Cells are HTML.

import { NAME } from '../brand';

export interface LeaveRow {
  what: string;
  where: string;
  online: string;
  /** shown in the home page's short version */
  short?: boolean;
}

export const LEAVES: LeaveRow[] = [
  {
    what: 'Your voice',
    where: 'Turned into text on your graphics card or processor.',
    online: 'Nothing. Speech models are downloaded once and checked; after that they run offline.',
    short: true,
  },
  {
    what: 'AI cleanup, Commands and Chat',
    where: 'On your PC with the built-in model, or on your own Ollama or LM Studio server.',
    online: 'Only if you add a cloud provider’s key: then the text goes to that provider (Groq, OpenAI, Anthropic, Gemini or OpenRouter). The text around your cursor goes to a cloud model only if you switch that on as well.',
    short: true,
  },
  {
    what: 'History, notes, meetings and memory',
    where: 'Stored on your PC. You choose how long history is kept, or not to keep it.',
    online: 'Nothing, unless you ask a cloud model about them.',
  },
  {
    what: 'Meeting audio',
    where: 'Recorded and kept on your PC, 14 days by default.',
    online: 'Nothing.',
  },
  {
    what: 'Web search',
    where: 'Off until you switch it on.',
    online: 'Your search goes to the search service you chose (Exa, Tavily, the wikis, or your AI provider’s own search).',
  },
  {
    what: 'Spoken replies and Read aloud',
    where: 'Kokoro or Windows’ voices, on your PC.',
    online: 'Only with ElevenLabs and your own key: it receives the text it reads.',
  },
  {
    what: 'Calendar',
    where: 'Meetings are kept on your PC.',
    online: 'If you connect one: read-only, straight from Google or your calendar’s iCal link.',
  },
  {
    what: `${NAME} Link`,
    where: 'Your phone talks to your PC over your home Wi-Fi. Off until you switch it on.',
    online: 'Nothing: no server in between.',
  },
  {
    what: 'Account',
    where: 'Optional.',
    online: `Only when you sign in. Signed out, ${NAME} never contacts the account service on its own.`,
  },
  {
    what: 'Updates',
    where: '',
    online: 'A check with GitHub about every four hours, which you can switch off.',
    short: true,
  },
];
