<script>
  // Settings → Meetings → Folders by company (calendar/filing/companies.rs):
  // "Meetings with people from example.com go in Clients", one row a
  // company, with Remove (its meetings stay where they are). The rules are
  // made from the offer after a meeting is filed by hand
  // (notices/companyFolders.js); this lists them.
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { onMount } from 'svelte';
  import Group from '../ui/Group.svelte';
  import Row from '../ui/Row.svelte';
  import Button from '../ui/Button.svelte';
  import { toast } from '../ui/toast.svelte.js';

  // [{ domain, folder, createdTs }]
  let rules = $state([]);
  let loaded = $state(false);

  async function load() {
    try {
      rules = (await invoke('company_folders')) || [];
    } catch {
      rules = [];
    }
    loaded = true;
  }

  onMount(() => {
    load();
    // A Yes, an Undo, a folder renamed or deleted.
    const offs = [listen('yap-company-folders', load), listen('yap-notes-changed', load)];
    return () => offs.forEach((u) => u.then((f) => f()));
  });

  async function remove(r) {
    try {
      await invoke('company_folder_remove', { domain: r.domain });
      rules = rules.filter((x) => x.domain !== r.domain);
      toast({ title: `New meetings with ${r.domain} go to Meetings again`, variant: 'success' });
    } catch (e) {
      toast({ title: "Couldn't remove the rule", description: String(e), variant: 'destructive' });
    }
  }
</script>

<Group
  title="Folders by company"
  id="settings-company-folders"
  desc="Meetings with people from these companies go in their folder, by who's on the calendar invite. A meeting's own series rule comes first, and a note you move stays where you put it."
>
  <div class="list" data-setting="company-folders" role="list" aria-label="Folders by company">
    {#each rules as r (r.domain)}
      <div class="item" role="listitem">
        <Row label={r.domain} desc={`Meetings with people from ${r.domain} go in ${r.folder}`}>
          <Button variant="secondary" size="sm" title={`Stop filing meetings with ${r.domain} in ${r.folder}`} onclick={() => remove(r)}>Remove</Button>
        </Row>
      </div>
    {:else}
      {#if loaded}
        <Row label="No company rules yet" desc="File a meeting with people from another company in a folder, and Yap offers to file every meeting with them there." />
      {/if}
    {/each}
  </div>
</Group>

<style>
  /* The group draws the hairline under the list; one between its rows. */
  .item + .item {
    border-top: 1px solid var(--yap-rule);
  }
</style>
