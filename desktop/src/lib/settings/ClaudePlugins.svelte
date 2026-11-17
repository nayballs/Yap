<script>
  // **Settings → Yap Code → Plugins** (17 Nov 2026; a Code chat's + menu → Plugins → Manage plugins…): Claude Code's
  // installed plugins (agents/plugins.rs: its own `claude plugin list --json`), each with its one line and a switch
  // that runs `claude plugin enable|disable` in its scope — the person's Claude Code setting, as in their terminal.
  // Nothing is installed from Yap: that's Claude Code's /plugin. A plugin synced from claude.ai or managed by an
  // organisation is switched where it comes from (no switch here).
  import { invoke } from '@tauri-apps/api/core';
  import Group from '../ui/Group.svelte';
  import Row from '../ui/Row.svelte';
  import Toggle from '../ui/Toggle.svelte';
  import { toast } from '../ui/toast.svelte.js';
  import { FEATURE_NAME } from '../core/codeChats.js';

  /** `enabled`: Yap Code is on (off, Claude Code isn't asked anything). */
  let { enabled = false } = $props();

  let listing = $state(null);
  let busy = $state(null);

  async function load() {
    try {
      listing = await invoke('claude_plugins');
    } catch (e) {
      listing = { installed: true, plugins: [], note: String(e?.message ?? e) };
    }
  }
  $effect(() => {
    if (enabled) load();
    else listing = null;
  });

  async function flip(p, on) {
    busy = p.id;
    try {
      await invoke('claude_plugin_set', { id: p.id, on, scope: p.scope });
      toast({ title: `${p.name} is ${on ? 'on' : 'off'} in Claude Code`, description: 'Code chats pick it up from their next message.' });
    } catch (e) {
      toast({ title: `Couldn't switch ${p.name}`, description: String(e?.message ?? e), variant: 'destructive' });
    } finally {
      busy = null;
      await load();
    }
  }

  const SCOPES = { user: 'Everywhere', project: 'In its project', local: 'In its project, for you', synced: 'From claude.ai', managed: 'Managed' };
</script>

<Group title="Plugins" desc="Claude Code's plugins, with their skills, commands and servers. Switching one here switches it in Claude Code everywhere, as /plugin does." id="settings-yap-code-plugins">
  {#if !enabled}
    <Row><p class="set-note" data-setting="yap-code-plugins">Turn {FEATURE_NAME} on to see Claude Code's plugins here.</p></Row>
  {:else if !listing}
    <Row><p class="set-note" data-setting="yap-code-plugins">Asking Claude Code…</p></Row>
  {:else if !listing.installed}
    <Row><p class="set-note" data-setting="yap-code-plugins">Claude Code isn't on this PC.</p></Row>
  {:else if listing.note}
    <Row><p class="set-note" data-setting="yap-code-plugins">{listing.note}</p></Row>
  {:else}
    {#each listing.plugins as p (p.id)}
      <Row setting="yap-code-plugins">
        <div class="prow" data-testid="yap-code-plugin" data-id={p.id}>
          <Toggle
            checked={p.enabled}
            label={p.name}
            desc={[p.description, `${SCOPES[p.scope] || p.scope}${p.version ? ` · ${p.version}` : ''}${p.marketplace ? ` · ${p.marketplace}` : ''}`].filter(Boolean).join(' — ')}
            ariaLabel={`${p.name} in Claude Code`}
            disabled={!p.switchable || busy === p.id}
            onchange={(on) => flip(p, on)}
          />
        </div>
      </Row>
    {:else}
      <Row><p class="set-note" data-setting="yap-code-plugins">No plugins installed. Add them in Claude Code with /plugin.</p></Row>
    {/each}
    <Row>
      <p class="set-note small">Plugins are installed and removed in Claude Code itself (/plugin), never from Yap.</p>
    </Row>
  {/if}
</Group>

<style>
  .prow {
    width: 100%;
  }
  .small {
    font-size: 12px;
  }
</style>
