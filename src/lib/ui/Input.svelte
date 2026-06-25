<script>
  let {
    value = $bindable(''),
    type = 'text',
    placeholder = '',
    disabled = false,
    oninput,
  } = $props();
</script>

<input
  class="input"
  {type}
  {placeholder}
  {disabled}
  bind:value
  oninput={() => oninput?.(value)}
/>

<style>
  .input {
    width: 100%;
    box-sizing: border-box;
    background: #181b22;
    border: 1px solid #2a2f3a;
    border-radius: 6px;
    color: #e5e7eb;
    padding: 7px 9px;
    font: inherit;
    font-size: 13px;
  }
  .input:focus {
    outline: none;
    border-color: #3b82f6;
  }
  .input:disabled {
    opacity: 0.5;
    cursor: default;
  }
</style>
