<script>
  // Says once that the window's view is in (index.html's `__yapMounted`):
  // its `onMount` runs after the view beside it rendered without throwing
  // (App.svelte's boundary). Draws nothing.
  import { onMount } from 'svelte';

  let { onmount } = $props();
  onMount(() => onmount?.());
</script>
