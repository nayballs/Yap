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
    gap: 6px;
    border: 1px solid transparent;
    border-radius: 7px;
    cursor: pointer;
    font: inherit;
    font-weight: 500;
    line-height: 1;
    transition:
      background 0.15s ease,
      border-color 0.15s ease,
      color 0.15s ease;
  }
  .btn:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .md {
    padding: 9px 16px;
    font-size: 13px;
  }
  .sm {
    padding: 6px 11px;
    font-size: 12px;
  }

  .primary {
    background: #3b82f6;
    color: #fff;
  }
  .primary:hover:not(:disabled) {
    background: #2563eb;
  }

  .secondary {
    background: #181b22;
    border-color: #2a2f3a;
    color: #e5e7eb;
  }
  .secondary:hover:not(:disabled) {
    border-color: #3b82f6;
  }

  .danger {
    background: transparent;
    border-color: rgba(239, 68, 68, 0.4);
    color: #ef4444;
  }
  .danger:hover:not(:disabled) {
    background: rgba(239, 68, 68, 0.12);
    border-color: #ef4444;
  }

  .ghost {
    background: transparent;
    color: #9ca3af;
  }
  .ghost:hover:not(:disabled) {
    color: #e5e7eb;
  }
</style>
