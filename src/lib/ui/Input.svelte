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
    background: var(--yap-s1);
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r);
    color: var(--yap-fg);
    padding: 8px 12px;
    font: inherit;
    font-size: 12.5px;
    transition: border-color var(--yap-dur) ease, box-shadow var(--yap-dur) ease;
  }
  .input:hover:not(:disabled) {
    border-color: var(--yap-border-hover);
  }
  .input:focus {
    outline: none;
    border-color: var(--yap-primary);
    box-shadow: 0 0 0 3px var(--yap-primary-wash);
  }
  .input::placeholder {
    color: var(--yap-fg-45);
  }
  .input:disabled {
    opacity: 0.5;
    cursor: default;
  }
</style>
