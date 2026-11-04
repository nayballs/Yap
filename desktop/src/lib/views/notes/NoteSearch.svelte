<script>
  // The Notes view's search results (NoteList loads it when a search is
  // typed): every note, whatever its folder — its title, My thoughts,
  // summary and transcript — holding the words typed
  // (src-tauri/src/search/library.rs `notes_search`), and with "Search by
  // meaning" on, a few that mean them (asked second: the words' results show
  // at once). Each shows the line it found, the words marked; it opens the
  // note there (notesState `openRequested`: the transcript line lit, My
  // thoughts' words selected, the summary's marked).
  //
  //   notes:    the view's state (notesState.svelte.js)
  //   query:    the search box's words
  //   searchEl: the search box (↑ from the first result goes back to it)
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import { typedWords } from '../../core/findWords.js';
  import { displayTitle, relTime } from './noteFormat.js';

  let { notes, query = '', searchEl = null } = $props();

  // `{ q, hits, semantic }` for the query shown; `searching` while asked.
  let found = $state(null);
  let searching = $state(false);
  let meaningOn = $state(false);
  let seq = 0;
  let debounce = null;

  const byId = $derived(new Map(notes.notes.map((n) => [n.id, n])));
  const results = $derived((found?.hits || []).filter((h) => byId.has(h.id)));
  let resultEls = $state([]);

  onMount(() => {
    invoke('search_status')
      .then((s) => {
        meaningOn = !!(s?.enabled && s?.installed);
        if (meaningOn) run(query);
      })
      .catch(() => {});
    return () => clearTimeout(debounce);
  });

  $effect(() => run(query));

  function run(text) {
    clearTimeout(debounce);
    const q = String(text || '').trim();
    if (!q) {
      seq++;
      found = null;
      searching = false;
      return;
    }
    debounce = setTimeout(async () => {
      const mine = ++seq;
      searching = true;
      try {
        // The words first (at once), then — with search by meaning — again.
        const words = await invoke('notes_search', { query: q, meaning: false });
        if (mine !== seq) return;
        found = { q, ...words };
        if (meaningOn) {
          const both = await invoke('notes_search', { query: q, meaning: true });
          if (mine === seq) found = { q, ...both };
        }
      } catch {
        if (mine === seq) found = { q, hits: [], semantic: false };
      } finally {
        if (mine === seq) searching = false;
      }
    }, 120);
  }

  // Where a result's line is: "Transcript · 12:40 · Priya", "My thoughts", "Summary".
  function where(h) {
    if (h.field === 'transcript') return ['Transcript', h.at, h.speaker].filter(Boolean).join(' · ');
    if (h.field === 'summary') return 'Summary';
    if (h.field === 'content') return 'My thoughts';
    return '';
  }

  function open(h) {
    const words = typedWords(found?.q || query);
    notes.openRequested({
      id: h.id,
      stop: false,
      line: h.field === 'transcript' ? h.line : null,
      ts: h.field === 'transcript' ? h.ts : null,
      tab: h.field === 'summary' ? 'enhanced' : h.field === 'content' ? 'raw' : null,
      find: h.words && h.field !== 'title' ? { words, line: h.field === 'content' || h.field === 'summary' ? h.line : null } : null,
    });
  }

  /** The search box's keys: ↓ to the first result, Enter opens it. */
  export function keydown(e) {
    if (e.key === 'ArrowDown' && results.length) {
      e.preventDefault();
      resultEls[0]?.focus();
    } else if (e.key === 'Enter' && results.length) {
      e.preventDefault();
      open(results[0]);
    }
  }

  function resultKeydown(e, i) {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      resultEls[i + 1]?.focus();
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      (i === 0 ? searchEl : resultEls[i - 1])?.focus();
    } else if (e.key === 'Escape') {
      searchEl?.focus();
    }
  }
</script>

{#snippet marked(parts)}{#each parts as p, k (k)}{#if p.hit}<mark class="found">{p.text}</mark>{:else}{p.text}{/if}{/each}{/snippet}

<div class="seccap">
  <span>
    {#if found}{results.length === 1 ? '1 note' : `${results.length} notes`}{#if found.semantic}{' · by meaning too'}{/if}{:else}Searching…{/if}
  </span>
</div>
{#if found && results.length === 0}
  <div class="none">
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="11" cy="11" r="7" /><path d="m21 21-4.3-4.3" /></svg>
    <p>{searching ? 'Searching…' : 'No notes match that'}</p>
  </div>
{:else}
  <div class="items" role="group" aria-label="Notes that match" aria-busy={searching}>
    {#each results as h, i (h.id)}
      {@const n = byId.get(h.id)}
      <button
        bind:this={resultEls[i]}
        class="result"
        class:active={notes.selected?.id === h.id}
        onclick={() => open(h)}
        onkeydown={(e) => resultKeydown(e, i)}
      >
        <span class="title">
          <span class="ttext">{#if h.title && h.titleParts?.length}{@render marked(h.titleParts)}{:else}{displayTitle(n)}{/if}</span>
          <span class="when">{relTime(n.updatedTs)}</span>
        </span>
        {#if h.parts?.length && h.field !== 'title'}
          <span class="line">{@render marked(h.parts)}</span>
        {/if}
        <span class="meta">
          {#if !h.words}<span class="meaning" title="No word of the search is in it: it's about the same thing">by meaning</span>{/if}
          {#if where(h)}<span>{where(h)}</span>{/if}
          {#if (n.folder || 'Personal') !== notes.activeFolder}<span>{n.folder || 'Personal'}</span>{/if}
        </span>
      </button>
    {/each}
  </div>
{/if}

<style>
  .seccap {
    padding: 12px 18px 4px;
    font-size: 10px;
    font-weight: 700;
    letter-spacing: 0.09em;
    text-transform: uppercase;
    color: var(--yap-muted-55);
  }
  .none {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    padding: 28px 16px;
    color: var(--yap-muted-55);
    font-size: 12px;
  }
  .none svg {
    width: 22px;
    height: 22px;
  }
  .none p {
    margin: 0;
  }
  .items {
    flex: 1 1 auto;
    overflow-y: auto;
    min-height: 0;
    padding: 0 8px 12px;
  }
  .result {
    display: flex;
    flex-direction: column;
    gap: 2px;
    width: 100%;
    padding: 8px 10px;
    border: none;
    border-radius: var(--yap-r);
    background: none;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .result:hover {
    background: var(--yap-s2);
  }
  .result.active {
    background: var(--yap-primary-wash);
  }
  .result:focus-visible {
    outline: none;
    box-shadow: var(--yap-focus);
  }
  .title {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12.5px;
    font-weight: 600;
  }
  .ttext {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .when {
    flex: 0 0 auto;
    font-size: 10px;
    font-weight: 500;
    color: var(--yap-muted-55);
  }
  .line {
    display: -webkit-box;
    overflow: hidden;
    -webkit-line-clamp: 3;
    line-clamp: 3;
    -webkit-box-orient: vertical;
    font-size: 11.5px;
    line-height: 1.45;
    color: var(--yap-muted);
    overflow-wrap: anywhere;
  }
  .meta {
    display: flex;
    flex-wrap: wrap;
    gap: 0 4px;
    font-size: 10.5px;
    color: var(--yap-muted-55);
  }
  .meta span + span::before {
    content: '· ';
  }
  .meaning {
    color: var(--yap-link);
  }
</style>
