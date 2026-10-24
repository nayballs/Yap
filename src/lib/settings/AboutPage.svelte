<script>
  // Settings → About: the version and its updates (updates.svelte.js, Rust's
  // updates.rs), Report a problem (ReportProblem.svelte, diagnostics.rs),
  // the project's links and the setup guide. Whether Yap checks
  // by itself is in General → Startup and updates; logs and the data folder
  // are in Privacy & data.
  //
  //   cfg:     Settings' config $state.
  //   version: the running version.
  //   goto:    opens another Settings page.
  import { invoke } from '@tauri-apps/api/core';
  import yapIcon from '../../assets/yap-logo.svg';
  import Group from '../ui/Group.svelte';
  import Row from '../ui/Row.svelte';
  import Button from '../ui/Button.svelte';
  import ReportProblem from './ReportProblem.svelte';
  import { updates, installUpdate, checkForUpdates, openRelease, formatAgo } from '../stores/updates.svelte.js';
  import { renderMarkdown } from '../core/markdown.js';
  import { createExternalLinkHandler } from '../core/externalLinks.js';

  let { cfg, version = '', goto = null } = $props();
</script>

<div class="page-h">
  <h1>About</h1>
  <p>Your version of Yap, its updates and where to find out more.</p>
</div>

<div class="brand">
  <img class="logo" src={yapIcon} alt="" aria-hidden="true" />
  <div class="btext">
    <div class="name">Yap <span class="ver">{version}</span></div>
    <div class="tag">Press your hotkey, speak, press again: Yap transcribes on this PC and types the text where you are.</div>
  </div>
</div>
<p class="promise">
  Your voice never leaves this PC.
  {#if goto}<button type="button" class="set-link" onclick={() => goto('privacy')}>What does, and when</button>{/if}
</p>

<Group title="Updates" id="settings-updates">
  <Row setting="updates">
    <div class="upd">
      <div class="upd-main">
        <div class="upd-text" aria-live="polite">
          {#if updates.status === 'installing'}
            <span class="upd-title">Restarting to update…</span>
            <span class="upd-sub">Installing {updates.version}. Yap will be right back.</span>
          {:else if updates.status === 'ready' && updates.deferred}
            <span class="upd-title">Restarting after this dictation</span>
            <span class="upd-sub">Yap installs {updates.version} as soon as you finish dictating.</span>
          {:else if updates.status === 'ready'}
            <span class="upd-title">Yap {updates.version} is ready to install</span>
            <span class="upd-sub">Downloaded in the background. Restarting takes a few seconds.</span>
          {:else if updates.status === 'downloading' || (updates.status === 'available' && updates.installQueued)}
            <span class="upd-title">Downloading Yap {updates.version}… {updates.progress}%</span>
            <span class="upd-sub">{updates.installQueued ? 'Yap restarts as soon as it’s done.' : 'You can keep working.'}</span>
          {:else if updates.status === 'available' && updates.portable}
            <span class="upd-title">Yap {updates.version} is available</span>
            <span class="upd-sub">Portable copies update by hand: download the new version from GitHub.</span>
          {:else if updates.status === 'available'}
            <span class="upd-title">Yap {updates.version} is available</span>
            <span class="upd-sub">
              {updates.error ||
                (updates.metered
                  ? 'Not downloaded yet: you’re on a metered connection.'
                  : 'Download it and restart to update.')}
            </span>
          {:else if updates.status === 'checking'}
            <span class="upd-title">Checking for updates…</span>
            <span class="upd-sub">Yap {updates.currentVersion}</span>
          {:else if updates.checked === 'uptodate'}
            <span class="upd-title ok">You’re up to date ✓</span>
            <span class="upd-sub">Yap {updates.currentVersion} is the latest version.</span>
          {:else if updates.checked === 'error'}
            <span class="upd-title">Couldn’t check for updates</span>
            <span class="upd-sub">{updates.checkError}</span>
          {:else}
            <span class="upd-title">Yap {updates.currentVersion || version}</span>
            <span class="upd-sub">
              {updates.lastChecked ? `Last checked ${formatAgo(updates.lastChecked)}` : 'Not checked yet'}
            </span>
          {/if}
        </div>
        <div class="upd-actions">
          {#if updates.status === 'ready' && !updates.deferred}
            <Button size="sm" onclick={installUpdate}>Restart to update</Button>
          {:else if updates.status === 'available' && updates.portable}
            <Button size="sm" onclick={openRelease}>Get it on GitHub</Button>
          {:else if updates.status === 'available' && !updates.installQueued}
            <Button size="sm" onclick={installUpdate}>Download and restart</Button>
          {:else if updates.status === 'idle'}
            <Button variant="secondary" size="sm" onclick={checkForUpdates}>Check for updates</Button>
          {/if}
        </div>
      </div>
      {#if updates.status === 'downloading'}
        <div class="upd-bar"><div class="upd-fill" style={`width:${updates.progress}%`}></div></div>
      {/if}
      {#if updates.error && updates.status === 'ready'}
        <p class="upd-err">{updates.error}</p>
      {/if}
      {#if updates.notes && ['ready', 'available', 'downloading'].includes(updates.status)}
        <div class="upd-notes">
          <div class="upd-notes-h">What’s new in {updates.version}</div>
          <!-- renderMarkdown passes no raw HTML and sanitises its output (lib/core/markdown.js). -->
          <div class="upd-notes-b">{@html renderMarkdown(updates.notes)}</div>
        </div>
      {/if}
      {#if updates.version && ['ready', 'available', 'downloading'].includes(updates.status)}
        <a class="alink" href={updates.releaseUrl} onclick={createExternalLinkHandler(updates.releaseUrl)} target="_blank" rel="noreferrer">Release page on GitHub</a>
      {/if}
    </div>
  </Row>
  <Row>
    <p class="set-note">
      {cfg.updateChecksEnabled ? 'Yap checks GitHub for updates every few hours.' : 'Yap only checks when you ask.'}
      {#if goto}<button type="button" class="set-link" onclick={() => goto('general#update-checks')}>Change in General</button>{/if}
    </p>
  </Row>
</Group>

<Group title="Report a problem" id="settings-report">
  <ReportProblem />
</Group>

<Group title="Learn more" id="settings-links">
  <Row label="Setup guide" desc="The first-run steps again: model, microphone, AI cleanup" setting="setup-guide">
    <Button variant="secondary" size="sm" onclick={() => invoke('open_onboarding')}>Show setup guide again</Button>
  </Row>
  <Row label="Source code" desc="Yap is open source, on GitHub" setting="github">
    <a class="alink inrow" href="https://github.com/nayballs/Yap" onclick={createExternalLinkHandler('https://github.com/nayballs/Yap')} target="_blank" rel="noreferrer">github.com/nayballs/Yap</a>
  </Row>
</Group>

<style>
  .brand {
    display: flex;
    align-items: center;
    gap: 14px;
  }
  .logo {
    width: 44px;
    height: 44px;
    border-radius: 10px;
    object-fit: contain;
    flex: 0 0 auto;
  }
  .btext {
    min-width: 0;
  }
  .name {
    font-size: 15px;
    font-weight: 650;
  }
  .ver {
    color: var(--yap-muted);
    font-weight: 500;
    font-size: 12.5px;
    margin-left: 4px;
    font-variant-numeric: tabular-nums;
  }
  .tag {
    margin-top: 2px;
    font-size: 12.5px;
    line-height: 1.5;
    color: var(--yap-muted);
  }
  .promise {
    margin: 14px 0 30px;
    font-size: 13px;
    color: var(--yap-fg-80);
  }
  .upd {
    width: 100%;
  }
  .upd-main {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 14px;
    flex-wrap: wrap;
  }
  .upd-text {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .upd-title {
    font-size: 13.5px;
    font-weight: 560;
    color: var(--yap-fg);
  }
  .upd-title.ok {
    color: var(--yap-success-text);
  }
  .upd-sub {
    font-size: 12.5px;
    color: var(--yap-muted);
  }
  .upd-actions {
    flex: 0 0 auto;
  }
  .upd-err {
    margin: 8px 0 0;
    font-size: 12.5px;
    color: var(--yap-danger);
  }
  .upd-notes {
    margin-top: 12px;
    padding: 10px 12px;
    border-radius: var(--yap-r);
    background: var(--yap-s2);
    border: 1px solid var(--yap-border-subtle);
  }
  .upd-notes-h {
    font-size: 12.5px;
    font-weight: 640;
    color: var(--yap-fg);
    margin-bottom: 4px;
  }
  .upd-notes-b {
    max-height: 180px;
    overflow-y: auto;
    font-size: 12.5px;
    line-height: 1.5;
    color: var(--yap-fg-80);
    user-select: text;
  }
  .upd-notes-b :global(:is(h2, h3, h4, h5)) {
    margin: 6px 0 2px;
    font-size: 12.5px;
    font-weight: 700;
    color: var(--yap-fg);
  }
  .upd-notes-b :global(p) {
    margin: 0 0 6px;
  }
  .upd-notes-b :global(:is(ul, ol)) {
    margin: 0 0 6px;
    padding-left: 18px;
  }
  .upd-notes-b :global(:last-child) {
    margin-bottom: 0;
  }
  .upd-bar {
    margin-top: 10px;
    height: 4px;
    border-radius: 3px;
    background: var(--yap-raised);
    overflow: hidden;
  }
  .upd-fill {
    height: 100%;
    background: var(--yap-ink);
    transition: width 0.15s ease;
  }
  .alink {
    display: inline-block;
    margin-top: 10px;
    font-size: 12.5px;
    font-weight: 600;
    color: var(--yap-link);
    text-decoration: underline;
    text-decoration-color: color-mix(in srgb, var(--yap-link) 40%, transparent);
    text-underline-offset: 2px;
  }
  .alink:hover {
    text-decoration-color: currentColor;
  }
  .alink:focus-visible {
    outline: none;
    border-radius: 2px;
    box-shadow: var(--yap-focus);
  }
  .alink.inrow {
    margin-top: 0;
  }
</style>
