<script>
  // **Chat's model asks the person** (`ask_user`, chat/cards/ask.rs, 15 Nov
  // 2026; the Claude app's question card): one to four questions with their
  // options (QuestionForm.svelte, the same form as a Code chat's question from
  // Claude Code), on the answer being written. The turn waits for the answer:
  // an option clicked (one question with one answer), **Send** (several, or
  // several picks), the person's own words, or **Let Yap decide** — or,
  // asked out loud, whatever they say next (`card.question.voice`: the card
  // says so). Once answered **the question and the choice stay in the
  // thread**, as the Claude app keeps them: each question in bold and what was
  // picked under it, or that it was left to Yap. Kept with the answer
  // (ChatMessage.confirms), so they come back with the chat.
  import { invoke } from '@tauri-apps/api/core';
  import QuestionForm from './QuestionForm.svelte';

  /** `card`: the turn's question card (`confirm` event, act "question": `question: {questions, answers, said, voice}`). */
  let { card } = $props();

  const q = $derived(card.question || { questions: [] });
  let picks = $state({});
  // Answered here, before Rust's next event says so.
  let sent = $state('');
  const phase = $derived(card.state === 'asking' && sent ? sent : card.state);
  const asking = $derived(phase === 'asking' || phase === 'sending');

  const setPick = (i, next) => (picks = { ...picks, [i]: { ...(picks[i] || { labels: [], other: '', own: false }), ...next } });
  async function answer(answers) {
    if (sent) return;
    sent = 'sending';
    try {
      await invoke('chat_question_answer', { id: card.id, answers });
    } catch {
      /* the question had lapsed (the turn stopped): its card says so */
    }
  }

  /** What the card says it ended as, under the questions. */
  const ending = $derived(
    {
      skipped: 'Left to Yap to decide',
      timedOut: 'Not answered in time: left to Yap',
      refused: 'Not answered',
    }[phase] || ''
  );
  /** A question's answer as the card shows it once answered. */
  const answerOf = (question) => q.answers?.[question.question] || '';
</script>

{#if asking}
  <div class="cquestion asking" role="group" aria-label="A question for you" data-testid="chat-question-card" data-state={phase}>
    <svg class="qicon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="12" r="9" /><path d="M9.5 9.2a2.6 2.6 0 0 1 5 .9c0 1.7-2.5 2.1-2.5 3.9" /><path d="M12 17.2h.01" /></svg>
    <div class="qcbody">
      <p class="qtitle"><strong>{q.questions.length > 1 ? `${q.questions.length} questions for you` : 'A question for you'}</strong>{#if q.voice}<span class="qsay"> · Say your answer, or pick one</span>{/if}</p>
      <QuestionForm
        questions={q.questions}
        {picks}
        onpick={setPick}
        onsend={answer}
        ondecide={() => answer(null)}
        busy={phase === 'sending'}
        decideLabel="Let Yap decide"
        testid="chat-question"
      />
    </div>
  </div>
{:else}
  <!-- Answered (or left): the question and the choice, as the thread keeps them. -->
  <div class="cquestion done" role="group" aria-label="Your answer" data-testid="chat-question-card" data-state={phase}>
    {#each q.questions as question, i (i)}
      <p class="aq">{question.question}</p>
      {#if answerOf(question)}
        <p class="aa" data-testid="chat-question-answer">{answerOf(question)}</p>
      {/if}
    {/each}
    {#if q.said}
      <p class="asaid" data-testid="chat-question-said">You said: “{q.said}”</p>
    {/if}
    {#if ending}
      <p class="aend">{ending}</p>
    {/if}
  </div>
{/if}

<style>
  .cquestion {
    box-sizing: border-box;
    color: var(--yap-paper-ink);
    font-size: 13px;
  }
  .cquestion.asking {
    display: flex;
    gap: 10px;
    width: min(600px, 100%);
    margin: 6px 0 2px;
    padding: 10px 12px;
    border: 1px solid color-mix(in srgb, var(--yap-primary) 40%, var(--yap-paper-rule));
    border-radius: 10px;
    background: var(--yap-paper);
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--yap-primary) 12%, transparent);
  }
  .qicon {
    flex: 0 0 auto;
    width: 18px;
    height: 18px;
    margin-top: 1px;
    color: var(--yap-primary);
  }
  .qcbody {
    flex: 1 1 auto;
    min-width: 0;
  }
  .qtitle {
    margin: 0;
  }
  .qtitle strong {
    font-weight: 650;
  }
  .qsay {
    color: var(--yap-paper-muted);
    font-size: 12px;
  }
  /* Answered: quiet, the question over the choice (as the Claude app's). */
  .cquestion.done {
    width: fit-content;
    max-width: min(480px, 100%);
    margin: 6px 0 2px;
    padding: 9px 12px;
    border: 1px solid var(--yap-paper-rule);
    border-radius: 10px;
    background: var(--yap-paper-s1);
  }
  .aq {
    margin: 0;
    font-weight: 650;
    overflow-wrap: anywhere;
  }
  .aq + .aq {
    margin-top: 6px;
  }
  .aa + .aq {
    margin-top: 8px;
  }
  .aa {
    margin: 2px 0 0;
    color: var(--yap-paper-muted);
    overflow-wrap: anywhere;
  }
  .asaid,
  .aend {
    margin: 6px 0 0;
    color: var(--yap-paper-muted);
    font-size: 12px;
    overflow-wrap: anywhere;
  }
</style>
