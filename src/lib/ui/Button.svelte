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

  /* Filled buttons are INK on the light theme — amber stays reserved for
     keycaps/highlights (falls back to primary under the dark-token scope). */
  .primary {
    background: var(--yap-ink, var(--yap-primary));
    color: var(--yap-ink-fg, var(--yap-primary-fg));
    border-color: transparent;
    box-shadow: var(--yap-shadow-sm);
  }
  .primary:hover:not(:disabled) {
    background: var(--yap-ink-hover, var(--yap-primary-hover));
  }

  .secondary {
    background: var(--yap-s2);
    border-color: var(--yap-border);
    color: var(--yap-fg-80);
  }
  .secondary:hover:not(:disabled) {
    background: var(--yap-s3);
    border-color: var(--yap-border-hover);
  }

  .danger {
    background: transparent;
    border-color: color-mix(in srgb, var(--yap-danger) 40%, transparent);
    color: var(--yap-danger);
  }
  .danger:hover:not(:disabled) {
    background: color-mix(in srgb, var(--yap-danger) 10%, transparent);
    border-color: var(--yap-danger);
  }

  .ghost {
    background: transparent;
    color: var(--yap-muted);
  }
  .ghost:hover:not(:disabled) {
    color: var(--yap-fg);
    background: var(--yap-s3);
  }
</style>
