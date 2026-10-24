<script>
  // The models a self-hosted or custom OpenAI-compatible server offers
  // (Ollama, LM Studio, a proxy…): `GET {baseUrl}/models`, asked by Rust
  // (model_catalog.rs `llm_models`) with the key saved for that scope —
  // OpenWhispr's OpenAICompatiblePanel pattern. Renders a <datalist> (`id`)
  // for the Model field's suggestions and one quiet line: "3 models on this
  // server" or why it couldn't ask. Asks again when the address changes.
  import { invoke } from '@tauri-apps/api/core';

  let { id, provider = 'local', baseUrl = '', scope = 'cleanup', model = '' } = $props();

  let models = $state([]);
  let error = $state('');
  let asked = ''; // the address the latest answer must be for
  let timer;

  async function ask(base) {
    try {
      const v = await invoke('llm_models', { provider, scope, baseUrl: base });
      if (base !== asked) return;
      models = v?.ok ? (v.models ?? []) : [];
      error = v?.ok ? '' : v?.error || '';
    } catch (e) {
      if (base !== asked) return;
      models = [];
      error = String(e);
    }
  }

  $effect(() => {
    const base = (baseUrl || '').trim();
    clearTimeout(timer);
    asked = base;
    if (!/^https?:\/\/\S+/.test(base)) {
      models = [];
      error = '';
      return;
    }
    // After typing stops.
    timer = setTimeout(() => ask(base), 600);
    return () => clearTimeout(timer);
  });

  const unknown = $derived(!!model && models.length > 0 && !models.some((m) => m.id === model));
</script>

<datalist {id}>
  {#each models as m (m.id)}
    <option value={m.id}></option>
  {/each}
</datalist>
{#if models.length > 0}
  <p class="srvline" data-testid="server-models">
    {models.length === 1 ? '1 model' : `${models.length} models`} on this server: {models
      .slice(0, 4)
      .map((m) => m.id)
      .join(', ')}{models.length > 4 ? '…' : ''}
    {#if unknown}<span class="warn"> The server doesn't list “{model}”.</span>{/if}
  </p>
{:else if error}
  <p class="srvline" data-testid="server-models">Couldn't list the server's models: {error}.</p>
{/if}

<style>
  /* Inset like a settings Row: it sits in the endpoint's card. */
  .srvline {
    margin: 0;
    padding: 8px 16px;
    font-size: 11px;
    line-height: 1.5;
    color: var(--yap-muted);
    overflow-wrap: anywhere;
  }
  .warn {
    color: var(--yap-warning);
  }
</style>
