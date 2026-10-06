<script>
  // The split-screen preview (notepad.rs `show_preview`): while the pointer
  // is on the notepad's split button, this click-through window sits over
  // the slot the call's window will move to, as a glass outline that reads
  // against any desktop (Wispr Flow's measured style). The window is the
  // slot plus a margin for the shadow (`PREVIEW_MARGIN`, 24 px).
</script>

<div class="glass" aria-hidden="true"></div>

<style>
  .glass {
    position: fixed;
    inset: 24px;
    box-sizing: border-box;
    border-radius: 12px;
    background: rgba(255, 255, 255, 0.1);
    border: 2px solid rgba(255, 255, 255, 0.45);
    box-shadow:
      0 0 0 1px rgba(0, 0, 0, 0.12),
      0 6px 24px rgba(0, 0, 0, 0.18);
  }
</style>
