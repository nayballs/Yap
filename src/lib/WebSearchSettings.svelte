<script>
  // Settings → Voice Assistant → Web search (web.rs): Chat and the Voice
  // Assistant look things up on the web when a question needs it, and show
  // where the answer came from.
  // - "Look things up on the web" (cfg.webSearch, off by default);
  // - the service with a key: Exa (recommended) or Tavily
  //   (cfg.webSearchService), each key kept in cfg.webSearchKeys like the
  //   AI keys (masked; "Get a free key" opens the service's dashboard);
  // - "Wikipedia and reference wikis": always included — what Yap uses
  //   with no key, and when the service has no results;
  // - where the search words go, said plainly.
  //
  //   cfg: Settings' config $state. Auto-save does the saving.
  import Group from './ui/Group.svelte';
  import Row from './ui/Row.svelte';
  import Toggle from './ui/Toggle.svelte';
  import Segmented from './ui/Segmented.svelte';
  import Input from './ui/Input.svelte';
  import { createExternalLinkHandler } from './externalLinks.js';

  let { cfg } = $props();

  const SERVICES = [
    { value: 'exa', label: 'Exa (Recommended)' },
    { value: 'tavily', label: 'Tavily' },
  ];
  const INFO = {
    exa: {
      name: 'Exa',
      host: 'exa.ai',
      keyUrl: 'https://dashboard.exa.ai/api-keys',
      free: '$10 of free searches every month, no card needed. Exa keeps no copy of your searches.',
      placeholder: 'Paste your Exa API key',
    },
    tavily: {
      name: 'Tavily',
      host: 'tavily.com',
      keyUrl: 'https://app.tavily.com/home',
      free: '1,000 free searches a month, no card needed.',
      placeholder: 'tvly-…',
    },
  };

  const service = $derived(INFO[cfg.webSearchService] ? cfg.webSearchService : 'exa');
  const info = $derived(INFO[service]);
  const key = $derived((cfg.webSearchKeys ?? {})[service] ?? '');
  const masked = $derived(!key ? '' : key.length > 8 ? `${key.slice(0, 3)}…${key.slice(-4)}` : '••••••••');
  let editing = $state(false);

  function setService(v) {
    cfg.webSearchService = v;
    editing = false;
  }
  function setKey(v) {
    cfg.webSearchKeys = { ...(cfg.webSearchKeys ?? {}), [service]: (v ?? '').trim() };
  }

  const where = $derived(
    key
      ? `Searches are sent to ${info.name} (${info.host}), and to Wikipedia or a reference wiki when ${info.name} has no results.`
      : `No ${info.name} API key is set, so searches are sent to Wikipedia and reference wikis.`
  );
</script>

<div id="settings-web-search">
  <Group title="Web search">
    <Row>
      <Toggle
        bind:checked={cfg.webSearch}
        label="Look things up on the web"
        desc="When a question needs it — today's news, a price, where to find something in a game — Chat and the Voice Assistant search the web and show where the answer came from."
      />
    </Row>

    <Row label="Search with" desc={info.free}>
      <Segmented bind:value={() => service, setService} options={SERVICES} label="Search with" disabled={!cfg.webSearch} />
    </Row>

    <Row>
      <div class="keyblock" class:off={!cfg.webSearch}>
        <div class="keyhead">
          <span class="klabel">{info.name} API key</span>
          <a class="keylink" href={info.keyUrl} onclick={createExternalLinkHandler(info.keyUrl)} target="_blank" rel="noreferrer">
            Get a free key →
          </a>
        </div>
        {#if masked && !editing}
          <div class="keymask">
            <span class="keytext">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                <circle cx="7.5" cy="15.5" r="4.5" />
                <path d="M10.8 12.2 21 2M15 8l3 3" />
              </svg>
              {masked}
            </span>
            <button class="keyedit" disabled={!cfg.webSearch} onclick={() => (editing = true)}>edit</button>
          </div>
        {:else}
          <!-- Typing keeps the field open; leaving it shows the key masked. -->
          <div class="keymask editing" onfocusout={() => (editing = false)}>
            <Input
              type="password"
              value={key}
              oninput={(v) => {
                editing = true;
                setKey(v);
              }}
              disabled={!cfg.webSearch}
              placeholder={info.placeholder}
            />
            {#if masked}
              <button class="keyedit" onclick={() => (editing = false)}>done</button>
            {/if}
          </div>
        {/if}
      </div>
    </Row>

    <Row>
      <div class="always" class:off={!cfg.webSearch}>
        <span class="tick" aria-hidden="true">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="m5 12.5 4.5 4.5L19 7.5" /></svg>
        </span>
        <span class="atext">
          <span class="alabel">Wikipedia and reference wikis</span>
          <span class="adesc">
            Always available, no API key needed. General questions are answered from Wikipedia in your language, and
            game questions from that game's wiki. Used when no API key is set, or when {info.name} has no results.
          </span>
        </span>
      </div>
    </Row>

    <Row>
      <p class="privacy">
        {where} Web pages are fetched directly from this PC. Yap only searches when a question calls for it, and every
        search is listed in Chat. After reading the web, Yap won't open apps, edit notes or copy to the clipboard until your
        next message.
      </p>
    </Row>
  </Group>
</div>

<style>
  .keyblock,
  .always {
    display: flex;
    flex-direction: column;
    gap: 6px;
    width: 100%;
  }
  .off {
    opacity: 0.55;
  }
  .keyhead {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 12px;
  }
  .klabel,
  .alabel {
    font-size: 12.5px;
    font-weight: 600;
    color: var(--yap-fg);
  }
  .keylink {
    font-size: 11.5px;
    color: var(--yap-primary);
    text-decoration: none;
    white-space: nowrap;
  }
  .keylink:hover {
    text-decoration: underline;
  }
  .keymask {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    padding: 7px 10px;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r);
    background: var(--yap-s1);
  }
  .keymask.editing {
    padding: 0;
    border: none;
    background: none;
  }
  .keytext {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    font-family: ui-monospace, Consolas, monospace;
    font-size: 12px;
    color: var(--yap-fg);
  }
  .keytext svg {
    width: 13px;
    height: 13px;
    color: var(--yap-muted);
  }
  .keyedit {
    padding: 2px 6px;
    border: none;
    background: none;
    color: var(--yap-primary);
    font: inherit;
    font-size: 11.5px;
    font-weight: 600;
    cursor: pointer;
  }
  .keyedit:disabled {
    cursor: default;
    opacity: 0.6;
  }
  .always {
    flex-direction: row;
    align-items: flex-start;
    gap: 10px;
  }
  .tick {
    display: inline-flex;
    flex: 0 0 auto;
    align-items: center;
    justify-content: center;
    width: 18px;
    height: 18px;
    margin-top: 1px;
    border-radius: var(--yap-r-full);
    background: var(--yap-primary-wash);
    color: var(--yap-primary);
  }
  .tick svg {
    width: 11px;
    height: 11px;
  }
  .atext {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .adesc,
  .privacy {
    font-size: 11.5px;
    line-height: 1.55;
    color: var(--yap-muted);
  }
  .privacy {
    margin: 0;
  }
</style>
