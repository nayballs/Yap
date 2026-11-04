<script>
  // The window's one right-click menu and confirm dialog (contextMenu.svelte.js):
  // every view's items open them through `openContextMenu` / `askConfirm`.
  import ContextMenu from './ContextMenu.svelte';
  import ConfirmDialog from './ConfirmDialog.svelte';
  import { contextMenu, confirmDialog, runConfirm } from './contextMenu.svelte.js';
</script>

<ContextMenu
  bind:open={contextMenu.open}
  x={contextMenu.x}
  y={contextMenu.y}
  items={contextMenu.items}
  label={contextMenu.label}
  keyboard={contextMenu.keyboard}
  alignEnd={contextMenu.alignEnd}
  returnFocus={contextMenu.returnFocus}
/>
<ConfirmDialog
  bind:open={confirmDialog.open}
  title={confirmDialog.title}
  body={confirmDialog.body}
  confirmLabel={confirmDialog.confirmLabel}
  busyLabel={confirmDialog.busyLabel}
  danger={confirmDialog.danger}
  busy={confirmDialog.busy}
  returnFocus={confirmDialog.returnFocus}
  onconfirm={runConfirm}
/>
