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
    background: #181b22;
    border: 1px solid #2a2f3a;
    border-radius: 6px;
    color: #e5e7eb;
    padding: 8px 9px;
    font: inherit;
    font-size: 13px;
    line-height: 1.5;
    resize: vertical;
  }
  .textarea:focus {
    outline: none;
    border-color: #3b82f6;
  }
  .textarea:disabled {
    opacity: 0.5;
    cursor: default;
  }
</style>
