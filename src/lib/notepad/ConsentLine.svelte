<script>
  // The consent line over the bar while a meeting records: "Let people know
  // you're taking notes", whose Learn more opens the consent message — why,
  // and a message for the meeting chat, editable (ConsentMessage), in the ⋯
  // menu's popover style — with Copy.
  import Icon from './NotepadIcon.svelte';

  /** `menu`: the notepad's open popover; `ontogglemenu(which)`; `consent`: the ConsentMessage. */
  let { menu = $bindable(), consent, ontogglemenu } = $props();

  function openConsent() {
    if (menu !== 'consent') consent.fill();
    ontogglemenu('consent');
  }
</script>

<div class="consent popwrap">
  Let people know you're taking notes.
  <button class="learn" aria-haspopup="dialog" aria-expanded={menu === 'consent'} onclick={openConsent}>
    Learn more
  </button>
  {#if menu === 'consent'}
    <div class="pop consentpop" role="dialog" aria-label="Consent message">
      <p class="consentwhy">Let people know you're taking notes. Yap transcribes on this PC; nothing is uploaded.</p>
      <textarea
        class="consentmsg"
        rows="4"
        aria-label="Message for the meeting chat"
        bind:value={consent.draft}
        onblur={() => consent.save()}
      ></textarea>
      <div class="consentbtns">
        <button class="ink small" onclick={() => consent.copy(false)}><Icon name="copy" />Copy</button>
      </div>
    </div>
  {/if}
</div>

<style>
  button {
    font: inherit;
    cursor: pointer;
  }

  /* The popover's anchor, then the line itself (its own position wins). */
  .popwrap {
    position: relative;
  }
  .pop {
    position: absolute;
    top: calc(100% + 4px);
    right: 0;
    z-index: 50;
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 184px;
    box-sizing: border-box;
    padding: 6px;
    border: 1px solid var(--yap-paper-s2);
    border-radius: 10px;
    background: var(--yap-paper);
    box-shadow: var(--yap-shadow-popover);
    animation: popin 0.08s ease-out;
  }
  @keyframes popin {
    from {
      opacity: 0;
      translate: 0 -4px;
    }
  }
  .consent {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 88px;
    margin: 0;
    text-align: center;
    font-size: 12px;
    line-height: 18px;
    color: var(--yap-paper-date);
    pointer-events: auto;
  }
  .learn {
    padding: 0;
    border: none;
    background: none;
    color: inherit;
    font-size: inherit;
    text-decoration: underline;
    text-underline-offset: 2px;
  }
  .learn:hover,
  .learn[aria-expanded='true'] {
    color: var(--yap-paper-ink);
  }
  /* "Learn more": the consent message, in the ⋯ menu's popover style. */
  .pop.consentpop {
    top: auto;
    right: auto;
    bottom: calc(100% + 8px);
    left: 50%;
    width: min(400px, calc(100vw - 72px));
    gap: 10px;
    padding: 12px;
    text-align: left;
    transform: translateX(-50%);
  }
  .consentwhy {
    margin: 0;
    font-size: 13px;
    line-height: 19px;
    color: var(--yap-paper-text-2);
  }
  .consentmsg {
    display: block;
    width: 100%;
    box-sizing: border-box;
    padding: 8px 10px;
    border: 1px solid var(--yap-paper-s2);
    border-radius: 8px;
    background: var(--yap-paper-card);
    color: var(--yap-paper-ink);
    font: inherit;
    font-size: 14px;
    line-height: 21px;
    resize: vertical;
    outline: none;
    user-select: text;
    -webkit-user-select: text;
    transition:
      border-color 0.15s,
      box-shadow 0.15s;
  }
  .consentmsg:focus {
    border-color: var(--yap-paper-text-2);
    box-shadow: 0 0 0 3px var(--yap-paper-ring);
  }
  .consentbtns {
    display: flex;
    justify-content: flex-end;
  }
  .consentbtns .ink {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .consentbtns .ink :global(svg) {
    width: 13px;
    height: 13px;
  }
</style>
