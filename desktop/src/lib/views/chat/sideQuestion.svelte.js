// **A side question** (Ctrl+;, chat/side.rs; the Claude app's side chat): a
// quick question about the chat on screen, answered in a box over the thread
// and not added to it. The box's state lives here; the box is
// SideQuestion.svelte, ChatView opens it.
//
// - Asking (`ask`): `chat_side_ask` streamed into the box (core/streaming.js);
//   a new question replaces the one being answered. The answer is the page's
//   alone: Rust keeps nothing.
// - Esc or ✕ (`close`): the answer stops if it's still coming, and it's gone.
// - Keep it (`keep`): `chat_side_keep` adds the question and the answer to the
//   thread as an ordinary exchange — never while the thread is answering
//   (`busy`: the box waits, and says so).
import { invoke } from '@tauri-apps/api/core';
import { streamCall, liveAnswer, applyEvent, STOPPED } from '../../core/streaming.js';

/** What Keep it says while the thread is answering (chat/side.rs KEEP_LATER). */
export const KEEP_LATER = 'Keep it once the answer in the chat is done.';

export function createSideQuestion() {
  let open = $state(false);
  /** The conversation it's about. */
  let conversation = $state(null);
  let input = $state('');
  /** The question asked (being answered, or answered). */
  let asked = $state('');
  let live = $state(liveAnswer());
  let answering = $state(false);
  /** The answer, whole: `{text, model, tokens, stopped}`. */
  let result = $state(null);
  let error = $state('');
  let keeping = $state(false);
  let call = null;
  let seq = 0;

  function reset() {
    call?.stop();
    call = null;
    seq++;
    asked = '';
    live = liveAnswer();
    answering = false;
    result = null;
    error = '';
    keeping = false;
  }

  /** The box over conversation `id`'s thread (another chat's goes). */
  function show(id) {
    if (open && conversation === id) return;
    reset();
    input = '';
    conversation = id;
    open = true;
  }

  /** Esc or ✕: stopped if it's still coming, and gone. */
  function close() {
    reset();
    input = '';
    open = false;
    conversation = null;
  }

  async function ask() {
    const question = input.trim();
    if (!question || conversation == null) return;
    call?.stop();
    const n = ++seq;
    asked = question;
    input = '';
    result = null;
    error = '';
    live = liveAnswer();
    answering = true;
    const c = streamCall('chat_side_ask', { conversationId: conversation, question }, (ev) => {
      if (n === seq) live = applyEvent(live, ev);
    });
    call = c;
    try {
      const r = await c.promise;
      if (n === seq) result = r;
    } catch (e) {
      const msg = String(e?.message ?? e);
      if (n === seq) error = msg.includes(STOPPED) ? '' : msg;
    } finally {
      if (n === seq) {
        answering = false;
        call = null;
      }
    }
  }

  /** Keep it: into the thread. `true` once it's there. */
  async function keep() {
    if (!result?.text || conversation == null || keeping) return false;
    keeping = true;
    error = '';
    try {
      await invoke('chat_side_keep', { conversationId: conversation, question: asked, answer: result.text, model: result.model ?? null, tokens: result.tokens ?? null });
      return true;
    } catch (e) {
      error = String(e?.message ?? e);
      return false;
    } finally {
      keeping = false;
    }
  }

  return {
    get open() {
      return open;
    },
    get conversation() {
      return conversation;
    },
    get input() {
      return input;
    },
    set input(v) {
      input = v;
    },
    get asked() {
      return asked;
    },
    get live() {
      return live;
    },
    get answering() {
      return answering;
    },
    get result() {
      return result;
    },
    get error() {
      return error;
    },
    get keeping() {
      return keeping;
    },
    show,
    close,
    ask,
    keep,
  };
}
