<script>
  // Toast viewport — port of OpenWhispr's ui/Toast.tsx: bottom-right stack,
  // per-variant accent bar, hover-pause, floating ✕ on hover, destructive
  // descriptions in a copyable mono error box, hairline progress bar,
  // slide-in-from-right entry / fade-out exit. Mounted once in ControlPanel.
  import { toastStore, dismiss, pauseToast, resumeToast } from './toast.svelte.js';

  let copiedId = $state(null);

  async function copyError(t) {
    if (!t.description) return;
    try {
      await navigator.clipboard.writeText(t.description);
      copiedId = t.id;
      setTimeout(() => (copiedId = null), 2000);
    } catch {
      /* clipboard unavailable */
    }
  }
</script>

{#if toastStore.list.length > 0}
  <div class="viewport">
    {#each toastStore.list as t (t.id)}
      <div
        class="toast {t.variant}"
        class:exiting={t.isExiting}
        role="status"
        onmouseenter={() => pauseToast(t.id)}
        onmouseleave={() => resumeToast(t.id)}
      >
        <div class="accent"></div>
        <div class="body">
          {#if t.title}<div class="title">{t.title}</div>{/if}
          {#if t.description && t.variant === 'destructive'}
            <div class="errbox">
              <span class="errtext">{t.description}</span>
              <button class="errcopy" aria-label="Copy error" onclick={() => copyError(t)}>
                {#if copiedId === t.id}
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 12l5 5L20 6" /></svg>
                {:else}
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="9" y="9" width="12" height="12" rx="2" /><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" /></svg>
                {/if}
              </button>
            </div>
          {:else if t.description}
            <div class="desc">{t.description}</div>
          {/if}
        </div>
        <button class="close" aria-label="Close" onclick={() => dismiss(t.id)}>
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6 6 18" /></svg>
        </button>
        {#if t.duration > 0 && !t.isExiting}
          <div class="progresswrap">
            <div class="progress" style={`animation-duration:${t.duration}ms`}></div>
          </div>
        {/if}
      </div>
    {/each}
  </div>
{/if}

<style>
  .viewport {
    position: fixed;
    bottom: 20px;
    right: 20px;
    z-index: 100;
    display: flex;
    flex-direction: column;
    gap: 6px;
    pointer-events: none;
  }
  .toast {
    pointer-events: auto;
    position: relative;
    display: flex;
    width: 300px;
    border-radius: 6px;
    overflow: visible;
    background: rgba(24, 26, 32, 0.97);
    border: 1px solid var(--yap-border);
    box-shadow: 0 8px 30px rgba(0, 0, 0, 0.45);
    animation: toast-in 300ms ease-out;
    transition:
      opacity 200ms ease-out,
      transform 200ms ease-out;
  }
  .toast.exiting {
    opacity: 0;
    transform: translateX(8px) scale(0.98);
  }
  @keyframes toast-in {
    from {
      opacity: 0;
      transform: translateX(16px);
    }
    to {
      opacity: 1;
      transform: translateX(0);
    }
  }
  .accent {
    width: 2.5px;
    flex: 0 0 auto;
    border-radius: 6px 0 0 6px;
    background: rgba(255, 255, 255, 0.2);
  }
  .toast.success .accent {
    background: #34d399;
  }
  .toast.destructive .accent {
    background: #f87171;
  }
  .body {
    flex: 1 1 auto;
    min-width: 0;
    padding: 9px 11px;
  }
  .title {
    font-size: 12px;
    font-weight: 600;
    line-height: 1.35;
    color: rgba(255, 255, 255, 0.92);
  }
  .desc {
    margin-top: 2px;
    font-size: 11.5px;
    line-height: 1.45;
    color: rgba(255, 255, 255, 0.5);
  }
  .errbox {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 6px;
    margin-top: 5px;
    padding: 5px 7px;
    border-radius: 4px;
    background: rgba(255, 255, 255, 0.04);
    border: 1px solid rgba(255, 255, 255, 0.06);
  }
  .errtext {
    min-width: 0;
    font-family: ui-monospace, Consolas, monospace;
    font-size: 11px;
    line-height: 1.45;
    color: rgba(252, 165, 165, 0.85);
    overflow-wrap: anywhere;
    user-select: all;
  }
  .errcopy {
    flex: 0 0 auto;
    display: inline-flex;
    width: 18px;
    height: 18px;
    align-items: center;
    justify-content: center;
    border: none;
    border-radius: 3px;
    background: none;
    color: rgba(255, 255, 255, 0.3);
    cursor: pointer;
  }
  .errcopy:hover {
    color: rgba(255, 255, 255, 0.7);
    background: rgba(255, 255, 255, 0.06);
  }
  .errcopy svg {
    width: 11px;
    height: 11px;
  }
  .close {
    position: absolute;
    top: -8px;
    left: -8px;
    width: 22px;
    height: 22px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 50%;
    border: 1px solid rgba(255, 255, 255, 0.1);
    background: rgba(255, 255, 255, 0.1);
    backdrop-filter: blur(4px);
    color: rgba(255, 255, 255, 0.7);
    cursor: pointer;
    opacity: 0;
    transform: scale(0.75);
    transition:
      opacity 150ms ease,
      transform 150ms ease,
      background 150ms ease;
  }
  .toast:hover .close {
    opacity: 1;
    transform: scale(1);
  }
  .close:hover {
    background: rgba(255, 255, 255, 0.2);
    color: #fff;
  }
  .close svg {
    width: 10px;
    height: 10px;
  }
  .progresswrap {
    position: absolute;
    bottom: 0;
    left: 3px;
    right: 0;
    height: 1px;
    overflow: hidden;
    border-radius: 0 0 6px 6px;
  }
  .progress {
    height: 100%;
    background: rgba(255, 255, 255, 0.15);
    animation-name: toast-progress;
    animation-timing-function: linear;
    animation-fill-mode: forwards;
  }
  .toast.success .progress {
    background: rgba(52, 211, 153, 0.3);
  }
  .toast.destructive .progress {
    background: rgba(248, 113, 113, 0.3);
  }
  @keyframes toast-progress {
    from {
      width: 100%;
    }
    to {
      width: 0%;
    }
  }
</style>
