<script>
  let {
    variant = 'primary', // primary | secondary | danger | ghost
    size = 'md', // sm | md
    disabled = false,
    title = '',
    type = 'button',
    onclick,
    children,
  } = $props();
</script>

<button class="btn {variant} {size}" {disabled} {title} {type} {onclick}>
  {@render children?.()}
</button>

<style>
  .btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 7px;
    border: 1px solid transparent;
    border-radius: var(--yap-r);
    cursor: pointer;
    font: inherit;
    font-weight: 600;
    line-height: 1;
    transition:
      background var(--yap-dur) ease,
      border-color var(--yap-dur) ease,
      color var(--yap-dur) ease,
      transform var(--yap-dur) ease;
  }
  .btn:active:not(:disabled) {
    transform: scale(0.985);
  }
  .btn:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .md {
    padding: 0 16px;
    height: 34px;
    font-size: 12.5px;
  }
  .sm {
    padding: 0 12px;
    height: 30px;
    font-size: 12px;
  }

  .primary {
    background: var(--yap-primary);
    color: var(--yap-primary-fg);
    border-color: var(--yap-primary-line);
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.25);
  }
  .primary:hover:not(:disabled) {
    background: var(--yap-primary-hover);
  }

  .secondary {
    background: rgba(255, 255, 255, 0.05);
    border-color: rgba(255, 255, 255, 0.08);
    color: var(--yap-fg-80);
  }
  .secondary:hover:not(:disabled) {
    background: rgba(255, 255, 255, 0.09);
  }

  .danger {
    background: transparent;
    border-color: rgba(224, 86, 79, 0.4);
    color: var(--yap-danger);
  }
  .danger:hover:not(:disabled) {
    background: rgba(224, 86, 79, 0.12);
    border-color: var(--yap-danger);
  }

  .ghost {
    background: transparent;
    color: var(--yap-muted);
  }
  .ghost:hover:not(:disabled) {
    color: var(--yap-fg);
    background: rgba(255, 255, 255, 0.05);
  }
</style>
