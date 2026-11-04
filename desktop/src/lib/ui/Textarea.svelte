<script>
  let {
    value = $bindable(''),
    placeholder = '',
    rows = 4,
    maxlength = undefined,
    ariaLabel = undefined,
    disabled = false,
    oninput,
  } = $props();
</script>

<textarea
  class="textarea"
  {placeholder}
  {rows}
  {maxlength}
  aria-label={ariaLabel}
  {disabled}
  bind:value
  oninput={() => oninput?.(value)}
></textarea>

<style>
  .textarea {
    width: 100%;
    box-sizing: border-box;
    background: var(--yap-s2);
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r);
    color: var(--yap-fg);
    padding: 8px 9px;
    font: inherit;
    font-size: 13px;
    line-height: 1.5;
    resize: vertical;
  }
  .textarea:focus {
    outline: none;
  }
  .textarea:focus-visible {
    box-shadow: var(--yap-focus);
  }
  .textarea:disabled {
    opacity: 0.5;
    cursor: default;
  }
</style>
