<script>
  let {
    value = $bindable(''),
    placeholder = '',
    rows = 4,
    disabled = false,
    oninput,
  } = $props();
</script>

<textarea
  class="textarea"
  {placeholder}
  {rows}
  {disabled}
  bind:value
  oninput={() => oninput?.(value)}
></textarea>

<style>
  .textarea {
    width: 100%;
    box-sizing: border-box;
    background: var(--yap-s1);
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
    border-color: var(--yap-primary);
  }
  .textarea:disabled {
    opacity: 0.5;
    cursor: default;
  }
</style>
