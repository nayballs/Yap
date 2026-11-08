<script>
  // "Connectors (preview)" (Settings → Connectors, Connectors wave 6): the
  // switch for the whole feature, `connectors_preview` in config.json, off
  // by default (integrations/connectors.rs `enabled`). Off, Connectors is
  // the calendars' page and Rust reads nothing about connectors; on, the
  // connectors' page (Settings.svelte asks Rust again after the save).
  import Toggle from '../../ui/Toggle.svelte';

  let { cfg } = $props();
</script>

{#if cfg}
  <section class="preview" data-setting="connectors-preview">
    <Toggle
      bind:checked={cfg.connectorsPreview}
      label="Connectors (preview)"
      badge="Preview"
      desc="Give Chat and the Voice Assistant tools from your apps and services, over MCP. Each change a tool makes asks you first, and nothing connects until you add a connector."
    />
  </section>
{/if}

<style>
  .preview {
    padding-top: 4px;
  }
</style>
