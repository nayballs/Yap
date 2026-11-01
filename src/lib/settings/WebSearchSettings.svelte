<script>
  // Settings → Voice Assistant → Web search (web.rs): Chat and the Voice
  // Assistant look things up on the web when a question needs it, and show
  // where the answer came from.
  // - "Look things up on the web" (cfg.webSearch, off by default);
  // - "Search with": the AI's own search (cfg.webSearchNative, the default
  //   and recommended: a cloud model searches with its provider's built-in
  //   search, web/native.rs), or Exa / Tavily for every model
  //   (cfg.webSearchService). With the AI's own search, local and
  //   self-hosted models use "Yap's search" (Exa or Tavily), and what each
  //   provider charges for its search is listed;
  // - each service's key kept in cfg.webSearchKeys like the AI keys
  //   (masked; "Get a free key" opens the service's dashboard);
  // - "Free sources, no key needed" (web/sources.rs, 2026-11-01): always
  //   included — Wikipedia and game wikis, the weather (Open-Meteo), places
  //   (OpenStreetMap), Stack Exchange, GitHub and arXiv, picked by the kind
  //   of question — what Yap uses with no key, and when a service has no
  //   results; with their credits;
  // - "What works with what": no key / the AI's own search / an Exa or
  //   Tavily key, said plainly;
  // - "Search engine" (cfg.webSearchBrowser, DuckDuckGo by
  //   default): the engine the button under an answer the free sources
  //   couldn't give opens;
  // - where the search words go, said plainly.
  //
  //   cfg:  Settings' config $state. Auto-save does the saving.
  //   goto: opens another Settings page (Privacy & data).
  //
  // What each provider charges sits behind a disclosure (review #10:
  // details on demand).
  import Group from '../ui/Group.svelte';
  import Row from '../ui/Row.svelte';
  import Toggle from '../ui/Toggle.svelte';
  import Segmented from '../ui/Segmented.svelte';
  import Select from '../ui/Select.svelte';
  import Input from '../ui/Input.svelte';
  import { createExternalLinkHandler } from '../core/externalLinks.js';
  import { maskKey } from '../core/providers.js';

  let { cfg, goto = null } = $props();

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
  const MODES = [
    {
      value: 'native',
      title: "The AI's own search",
      recommended: true,
      desc: "Cloud models (Groq, Google Gemini, Anthropic, OpenAI, OpenRouter) search with their provider's built-in search, on the API key Chat already uses. Local and self-hosted models use Yap's search below.",
    },
    { value: 'exa', title: 'Exa', desc: `Every model searches with Exa. ${INFO.exa.free}` },
    { value: 'tavily', title: 'Tavily', desc: `Every model searches with Tavily. ${INFO.tavily.free}` },
  ];
  // What each provider's own search costs, as published on 18 October 2026.
  const COSTS = [
    {
      name: 'Groq',
      cost: 'No charge on the free plan, but each searched answer uses about 4,000 of the 8,000 tokens a minute the plan allows. Paid plans: $5 per 1,000 searches and $1 per 1,000 pages opened.',
    },
    {
      name: 'Google Gemini',
      cost: 'Gemini 3 and newer need billing turned on: 5,000 searches a month free, then $14 per 1,000. On a free key, Yap uses its own search instead. Gemini 2.5 Flash and Flash-Lite, where still offered: 500 searched answers a day free. Google may use free-tier requests to improve its products.',
    },
    { name: 'Anthropic', cost: '$10 per 1,000 searches on your Anthropic bill, plus the tokens for the results.' },
    { name: 'OpenAI', cost: '$10 per 1,000 searches on your OpenAI bill, plus the search results’ tokens at the model’s rate.' },
    {
      name: 'OpenRouter',
      cost: 'The model provider’s own search price where it has one, otherwise $0.007 per search (Exa), from your OpenRouter credits.',
    },
  ];

  // Yap's free sources (web/sources.rs): what each answers, and its credit
  // (CC BY 4.0, ODbL, arXiv's acknowledgement).
  const FREE = [
    { name: 'Wikipedia and game wikis', what: 'facts, people, places, a game’s guides' },
    { name: 'Open-Meteo', what: 'the weather and forecasts, in your units' },
    { name: 'OpenStreetMap', what: 'where a place is, an address' },
    { name: 'Stack Exchange and GitHub', what: 'programming questions, errors, libraries and tools' },
    { name: 'arXiv', what: 'research papers' },
  ];
  const ENGINES = [
    { value: 'duckduckgo', label: 'DuckDuckGo' },
    { value: 'google', label: 'Google' },
    { value: 'bing', label: 'Bing' },
    { value: 'brave', label: 'Brave Search' },
    { value: 'ecosia', label: 'Ecosia' },
    { value: 'startpage', label: 'Startpage' },
  ];
  const FREE_NAMES = 'Wikipedia, Open-Meteo, OpenStreetMap, Stack Exchange, GitHub and arXiv';

  const native = $derived(cfg.webSearchNative !== false);
  const service = $derived(INFO[cfg.webSearchService] ? cfg.webSearchService : 'exa');
  const mode = $derived(native ? 'native' : service);
  const info = $derived(INFO[service]);
  const key = $derived((cfg.webSearchKeys ?? {})[service] ?? '');
  const masked = $derived(maskKey(key));
  let editing = $state(false);

  function setMode(v) {
    if (v === 'native') {
      cfg.webSearchNative = true;
    } else {
      cfg.webSearchNative = false;
      cfg.webSearchService = v;
    }
    editing = false;
  }
  function setService(v) {
    cfg.webSearchService = v;
    editing = false;
  }
  function setKey(v) {
    cfg.webSearchKeys = { ...(cfg.webSearchKeys ?? {}), [service]: (v ?? '').trim() };
  }

  const own = $derived(
    key
      ? `${info.name} (${info.host}), and to Yap’s free sources when ${info.name} has no results`
      : `Yap’s free sources (${FREE_NAMES}: only the one that fits the question), as no ${info.name} API key is set`
  );
  const where = $derived(
    native
      ? `With a cloud model, your search words go to that model’s provider, which runs the search on its own servers. With a local or self-hosted model, or when a provider’s search is unavailable, they go to ${own}.`
      : key
        ? `Searches are sent to ${own}.`
        : `No ${info.name} API key is set, so searches are sent to Yap’s free sources (${FREE_NAMES}: only the one that fits the question).`
  );
</script>

<div id="settings-web-search">
  <Group title="Web search">
    <Row setting="web-search">
      <Toggle
        bind:checked={cfg.webSearch}
        label="Look things up on the web"
        desc="For today's news, a price or a game tip, Chat and the Voice Assistant search and show their sources"
      />
    </Row>

    <Row>
      <div class="modes" class:off={!cfg.webSearch} role="radiogroup" aria-label="Search with">
        <span class="klabel">Search with</span>
        {#each MODES as m (m.value)}
          <button
            type="button"
            class="mode"
            class:on={mode === m.value}
            role="radio"
            aria-checked={mode === m.value}
            disabled={!cfg.webSearch}
            onclick={() => setMode(m.value)}
          >
            <span class="radio" aria-hidden="true"></span>
            <span class="mtext">
              <span class="mtitle">
                {m.title}
                {#if m.recommended}<span class="badge">Recommended</span>{/if}
              </span>
              <span class="mdesc">{m.desc}</span>
            </span>
          </button>
        {/each}
      </div>
    </Row>

    {#if native}
      <Row>
        <details class="set-more" class:off={!cfg.webSearch}>
          <summary>What each provider charges for its search</summary>
          <div class="more-body costs">
          <dl>
            {#each COSTS as c (c.name)}
              <div class="cost">
                <dt>{c.name}</dt>
                <dd>{c.cost}</dd>
              </div>
            {/each}
          </dl>
          <span class="asof">
            Searches are billed to your own account with the provider, at the prices it published (as of 18 October 2026).
          </span>
          </div>
        </details>
      </Row>

      <Row label="Yap's search" desc="Used by local and self-hosted models, and when a provider's own search is unavailable">
        <Segmented
          bind:value={() => service, setService}
          options={SERVICES}
          label="Yap's search"
          disabled={!cfg.webSearch}
        />
      </Row>
    {/if}

    <Row>
      <div class="keyblock" class:off={!cfg.webSearch}>
        <div class="keyhead">
          <span class="klabel">{info.name} API key</span>
          <a class="keylink" href={info.keyUrl} onclick={createExternalLinkHandler(info.keyUrl)} target="_blank" rel="noreferrer">
            Get a free key
          </a>
        </div>
        {#if native}
          <span class="kdesc">{info.free}</span>
        {/if}
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
      <div class="always" class:off={!cfg.webSearch} data-testid="web-free-sources">
        <span class="tick" aria-hidden="true">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="m5 12.5 4.5 4.5L19 7.5" /></svg>
        </span>
        <span class="atext">
          <span class="alabel">Free sources, no key needed</span>
          <span class="adesc">
            Always available: Yap picks the one that fits the question. Used when no API key is set and the AI has no
            search of its own, or when {info.name} has no results.
          </span>
          <ul class="free">
            {#each FREE as s (s.name)}
              <li><strong>{s.name}</strong>: {s.what}</li>
            {/each}
          </ul>
          <span class="adesc">
            They don’t cover prices, shopping, the news, reviews or timetables: for those, Yap says so and offers
            “Search beside the chat”.
          </span>
          <span class="credits">
            Weather data by Open-Meteo.com (CC BY 4.0) · Map data © OpenStreetMap contributors · Thank you to arXiv for
            use of its open access interoperability.
          </span>
        </span>
      </div>
    </Row>

    <Row>
      <details class="set-more" class:off={!cfg.webSearch} data-testid="web-what-works">
        <summary>What works with what</summary>
        <div class="more-body">
          <dl class="works">
            <div class="cost">
              <dt>No key</dt>
              <dd>
                The free sources above. For anything else (prices, the news, reviews, travel…) the answer offers “Search
                beside the chat”, or in your own browser; Yap never reads that page.
              </dd>
            </div>
            <div class="cost">
              <dt>The AI’s own search</dt>
              <dd>
                A cloud chat model (Groq, Google Gemini, Anthropic, OpenAI, OpenRouter) searches the whole web with its
                provider’s search, on the key Chat already uses, billed to your account. Models on this PC don’t have it.
              </dd>
            </div>
            <div class="cost">
              <dt>An Exa or Tavily key</dt>
              <dd>Every model, including the ones on this PC, searches the whole web. Both have free monthly searches.</dd>
            </div>
          </dl>
        </div>
      </details>
    </Row>

    <Row setting="web-search-browser" label="Search engine" desc="What a search you open uses: beside the chat, in its address bar, or in your own browser">
      <Select bind:value={cfg.webSearchBrowser} options={ENGINES} ariaLabel="Search engine" disabled={!cfg.webSearch} />
    </Row>

    <Row>
      <p class="privacy">
        {where}
        {#if goto}<button type="button" class="set-link" onclick={() => goto('privacy#leaves')}>Privacy & data</button>{/if}
      </p>
      <p class="privacy safety">
        Every search is listed in Chat. After reading the web, Yap won't open apps, edit notes or copy to the clipboard
        until your next message.
      </p>
    </Row>
  </Group>
</div>

<style>
  .keyblock,
  .always,
  .modes,
  .costs {
    display: flex;
    flex-direction: column;
    gap: 6px;
    width: 100%;
  }
  .modes {
    gap: 8px;
  }
  .off {
    opacity: 0.55;
  }
  .mode {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    width: 100%;
    padding: 10px 12px;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r);
    background: var(--yap-s2);
    font: inherit;
    text-align: left;
    color: var(--yap-fg);
    cursor: pointer;
    transition:
      border-color var(--yap-dur) ease,
      background var(--yap-dur) ease;
  }
  .mode:hover:not(:disabled) {
    border-color: var(--yap-border-hover);
  }
  .mode.on {
    border-color: var(--yap-fg-62);
    box-shadow: var(--yap-shadow-sm);
  }
  .mode:disabled {
    cursor: default;
  }
  .mode:focus-visible {
    outline: none;
    box-shadow: var(--yap-focus);
  }
  .radio {
    flex: 0 0 auto;
    width: 14px;
    height: 14px;
    margin-top: 2px;
    box-sizing: border-box;
    border: 1.5px solid var(--yap-switch-edge);
    border-radius: var(--yap-r-full);
    background: var(--yap-s2);
  }
  .mode.on .radio {
    border-color: var(--yap-ink);
    background: radial-gradient(circle, var(--yap-ink) 0 3px, var(--yap-s2) 3.5px);
  }
  .mtext {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .mtitle {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
    font-weight: 600;
  }
  .badge {
    padding: 1px 7px;
    border-radius: var(--yap-r-full);
    background: var(--yap-primary-wash);
    color: var(--yap-link);
    font-size: 10.5px;
    font-weight: 600;
  }
  .mdesc,
  .kdesc,
  .asof {
    font-size: 12.5px;
    line-height: 1.5;
    color: var(--yap-muted);
  }
  dl {
    display: flex;
    flex-direction: column;
    margin: 0;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r);
    background: var(--yap-s2);
  }
  .cost {
    display: grid;
    grid-template-columns: 110px 1fr;
    gap: 12px;
    padding: 7px 12px;
  }
  .cost + .cost {
    border-top: 1px solid var(--yap-border-subtle);
  }
  dt {
    font-size: 12px;
    font-weight: 600;
    color: var(--yap-fg);
  }
  dd {
    margin: 0;
    font-size: 12px;
    line-height: 1.5;
    color: var(--yap-fg-80);
  }
  .keyhead {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 12px;
  }
  .klabel,
  .alabel {
    font-size: 13.5px;
    font-weight: 560;
    color: var(--yap-fg);
  }
  .keylink {
    font-size: 12.5px;
    font-weight: 600;
    color: var(--yap-link);
    text-decoration: underline;
    text-decoration-color: color-mix(in srgb, var(--yap-link) 40%, transparent);
    text-underline-offset: 2px;
    white-space: nowrap;
  }
  .keylink:hover {
    text-decoration-color: currentColor;
  }
  .keylink:focus-visible,
  .keyedit:focus-visible {
    outline: none;
    border-radius: 3px;
    box-shadow: var(--yap-focus);
  }
  .keymask {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    padding: 7px 10px;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r);
    background: var(--yap-s2);
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
    color: var(--yap-link);
    font: inherit;
    font-size: 12.5px;
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
    background: color-mix(in srgb, var(--yap-success) 12%, var(--yap-s2));
    color: var(--yap-success-text);
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
  .free {
    margin: 4px 0;
    padding-left: 18px;
    font-size: 12.5px;
    line-height: 1.6;
    color: var(--yap-fg-80);
  }
  .free strong {
    font-weight: 600;
    color: var(--yap-fg);
  }
  .credits {
    margin-top: 4px;
    font-size: 11.5px;
    line-height: 1.5;
    color: var(--yap-muted);
  }
  .works {
    margin-top: 2px;
  }
  .works .cost {
    grid-template-columns: 150px 1fr;
  }
  .adesc,
  .privacy {
    font-size: 12.5px;
    line-height: 1.55;
    color: var(--yap-muted);
  }
  .privacy {
    margin: 0;
  }
  .privacy.safety {
    margin-top: 6px;
  }
</style>
