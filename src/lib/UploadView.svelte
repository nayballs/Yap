<script>
  // Upload — transcribe an audio FILE locally, ported from OpenWhispr's
  // notes/UploadAudioView.tsx state machine (idle → selected → transcribing →
  // complete | error) with the same drop-zone / file-card / progress / result
  // flow. Yap differences: transcription is ALWAYS local (the same warm engine
  // dictation uses — no cloud path, no file-size limits), decode is Symphonia
  // in Rust (media.rs), and the result lands in the Home feed via history.
  // The job itself — its file, progress, Cancel and result — lives in
  // upload.svelte.js, which ControlPanel starts: leaving this view mid-job
  // and coming back finds it as it is (code review 2026-10-23 #8).
  import { invoke } from '@tauri-apps/api/core';
  import { getCurrentWebview } from '@tauri-apps/api/webview';
  import { onMount } from 'svelte';
  import { MODELS } from './models.js';
  import { upload, chooseFile, failed, transcribe, cancel, reset } from './upload.svelte.js';

  // Matches media.rs (Symphonia features): no opus/webm yet.
  const SUPPORTED_EXTENSIONS = ['mp3', 'wav', 'm4a', 'aac', 'flac', 'ogg', 'oga'];

  // The job's, shown here (upload.svelte.js).
  const state = $derived(upload.state); // idle | selected | transcribing | complete | error
  const file = $derived(upload.file);
  const result = $derived(upload.result);
  const error = $derived(upload.error);
  const progress = $derived(upload.progress);
  const chunkProgress = $derived(upload.chunkProgress);
  // This view's own.
  let isDragOver = $state(false);
  let modelLabel = $state('');
  let copied = $state(false);

  function extOf(name) {
    return (name.split('.').pop() || '').toLowerCase();
  }

  async function handleBrowse() {
    try {
      const { open } = await import('@tauri-apps/plugin-dialog');
      const path = await open({
        multiple: false,
        filters: [{ name: 'Audio', extensions: SUPPORTED_EXTENSIONS }],
      });
      if (typeof path === 'string' && path) await chooseFile(path);
    } catch (e) {
      failed(`Couldn't open the file picker: ${e}`);
    }
  }

  async function copyResult() {
    if (!result) return;
    try {
      await navigator.clipboard.writeText(result);
      copied = true;
      setTimeout(() => (copied = false), 1500);
    } catch {
      /* clipboard unavailable */
    }
  }

  onMount(() => {
    // Native drag-drop (only while the view is up): the Tauri webview
    // reports real file paths. Its promise kept, so a view left before it
    // resolves still lets go of it.
    const dragDrop = getCurrentWebview().onDragDropEvent((event) => {
      const t = event.payload.type;
      if (t === 'over' || t === 'enter') {
        isDragOver = true;
      } else if (t === 'leave') {
        isDragOver = false;
      } else if (t === 'drop') {
        isDragOver = false;
        if (upload.state === 'transcribing') return;
        const path = (event.payload.paths || [])[0];
        if (!path) return;
        const name = path.split(/[/\\]/).pop() || '';
        if (!SUPPORTED_EXTENSIONS.includes(extOf(name))) {
          failed(`Unsupported file type ".${extOf(name)}" — supported: ${SUPPORTED_EXTENSIONS.join(', ')}`);
          return;
        }
        chooseFile(path);
      }
    });

    // The active STT model, for the "transcribes with" label.
    // By its name ("Parakeet V3"), not its id ("parakeet-tdt-0.6b-v3").
    invoke('get_config')
      .then((cfg) => {
        const id = cfg?.modelSize || '';
        modelLabel = MODELS.find((m) => m.id === id)?.name || id;
      })
      .catch(() => {});

    return () => dragDrop.then((u) => u && u()).catch(() => {});
  });
</script>

<div class="wrap">
  <div class="inner">
    <div class="page-h">
      <h1>Upload</h1>
      <p>
        Transcribe an audio file completely on this PC — meetings, voice memos, recordings.
        Nothing is uploaded anywhere{modelLabel ? ` (transcribes with ${modelLabel})` : ''}.
      </p>
    </div>

    {#if state === 'idle' || (state === 'error' && !file)}
      <div
        class="dropzone"
        class:over={isDragOver}
        role="button"
        tabindex="0"
        onclick={handleBrowse}
        onkeydown={(e) => (e.key === 'Enter' || e.key === ' ') && handleBrowse()}
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" /><path d="M17 8l-5-5-5 5" /><path d="M12 3v12" /></svg>
        <p class="dz-title">Drop an audio file here</p>
        <p class="dz-sub">or click to browse — {SUPPORTED_EXTENSIONS.join(', ')}</p>
      </div>
      {#if state === 'error' && error}
        <p class="errline">{error}</p>
      {/if}
    {:else if file}
      <div class="filecard">
        <span class="fileicon">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" /><path d="M14 2v6h6" /><path d="M10 15.5a2 2 0 1 0 4 0V11l3 1" /></svg>
        </span>
        <div class="filemeta">
          <p class="filename">{file.name}</p>
          <p class="filesize">{file.size}</p>
        </div>
        {#if state !== 'transcribing'}
          <button class="clear" title="Remove file" aria-label="Remove file" onclick={reset}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6 6 18" /></svg>
          </button>
        {/if}
      </div>

      {#if state === 'selected' || state === 'error'}
        {#if error}
          <p class="errline">{error}</p>
        {/if}
        <div class="actions">
          <button class="primary" onclick={transcribe}>
            {state === 'error' ? 'Try again' : 'Transcribe'}
          </button>
        </div>
      {:else if state === 'transcribing'}
        <div class="progresswrap">
          <div class="bar"><div class="fill" style={`width:${progress}%`}></div></div>
          <p class="progressline">
            {#if chunkProgress}
              Transcribing… chunk {chunkProgress.chunksCompleted} of {chunkProgress.chunksTotal}
            {:else}
              Decoding audio…
            {/if}
            <button class="cancel" onclick={cancel}>Cancel</button>
          </p>
        </div>
      {:else if state === 'complete'}
        <div class="resultcard">
          <div class="resulthead">
            <h3>Transcript</h3>
            <button class="copybtn" onclick={copyResult}>
              {copied ? 'Copied!' : 'Copy'}
            </button>
          </div>
          <div class="resulttext">{result}</div>
        </div>
        <div class="actions">
          <button class="secondary" onclick={reset}>Transcribe another file</button>
        </div>
        <p class="hint">Also saved to your Home feed.</p>
      {/if}
    {/if}
  </div>
</div>

<style>
  .wrap {
    flex: 1 1 auto;
    overflow-y: auto;
    min-height: 0;
  }
  .inner {
    max-width: 680px;
    margin: 0 auto;
    padding: 26px 30px 40px;
  }
  .page-h {
    margin: 0 0 22px;
  }
  /* The serif display title, as Meetings and Settings have it. */
  .page-h h1 {
    margin: 0 0 6px;
    font-family: var(--yap-font-display);
    font-size: 28px;
    font-weight: 550;
    letter-spacing: -0.005em;
    line-height: 1.2;
  }
  .page-h p {
    margin: 0;
    font-size: 12px;
    color: var(--yap-muted);
    line-height: 1.55;
  }

  .dropzone {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    padding: 54px 24px;
    border: 1.5px dashed var(--yap-border);
    border-radius: var(--yap-r-lg);
    background: var(--yap-s2);
    color: var(--yap-muted);
    cursor: pointer;
    text-align: center;
    transition:
      border-color var(--yap-dur) ease,
      background var(--yap-dur) ease;
  }
  .dropzone:hover,
  .dropzone.over {
    border-color: var(--yap-primary);
    background: var(--yap-primary-wash);
  }
  .dropzone svg {
    width: 30px;
    height: 30px;
    margin-bottom: 4px;
    color: var(--yap-muted-55);
  }
  .dropzone.over svg {
    color: var(--yap-primary);
  }
  .dz-title {
    margin: 0;
    font-size: 13.5px;
    font-weight: 600;
    color: var(--yap-fg);
  }
  .dz-sub {
    margin: 0;
    font-size: 11.5px;
    color: var(--yap-muted-55);
  }

  .filecard {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 12px 14px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-lg);
    background: var(--yap-s2);
    margin-bottom: 14px;
  }
  .fileicon {
    display: inline-flex;
    width: 34px;
    height: 34px;
    align-items: center;
    justify-content: center;
    border-radius: var(--yap-r);
    background: var(--yap-primary-wash);
    color: var(--yap-primary);
    flex: 0 0 auto;
  }
  .fileicon svg {
    width: 17px;
    height: 17px;
  }
  .filemeta {
    flex: 1 1 auto;
    min-width: 0;
  }
  .filename {
    margin: 0;
    font-size: 13px;
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .filesize {
    margin: 1px 0 0;
    font-size: 11px;
    color: var(--yap-muted-55);
  }
  .clear {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 26px;
    height: 26px;
    border: none;
    border-radius: var(--yap-r-sm);
    background: none;
    color: var(--yap-muted-55);
    cursor: pointer;
  }
  .clear:hover {
    background: var(--yap-s1);
    color: var(--yap-fg);
  }
  .clear svg {
    width: 13px;
    height: 13px;
  }

  .actions {
    display: flex;
    gap: 8px;
  }
  .primary {
    height: 34px;
    padding: 0 18px;
    border: none;
    border-radius: var(--yap-r);
    background: var(--yap-ink, var(--yap-primary));
    color: var(--yap-ink-fg, var(--yap-primary-fg));
    font: inherit;
    font-size: 12.5px;
    font-weight: 600;
    cursor: pointer;
  }
  .primary:hover {
    background: var(--yap-ink-hover, var(--yap-primary-hover));
  }
  .secondary {
    height: 32px;
    padding: 0 14px;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r);
    background: var(--yap-s2);
    color: var(--yap-fg);
    font: inherit;
    font-size: 12px;
    cursor: pointer;
  }
  .secondary:hover {
    border-color: var(--yap-border-hover);
  }

  .progresswrap {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .bar {
    height: 6px;
    border-radius: 3px;
    background: var(--yap-s2);
    overflow: hidden;
  }
  .fill {
    height: 100%;
    border-radius: 3px;
    background: var(--yap-primary);
    transition: width 300ms ease;
  }
  .progressline {
    display: flex;
    align-items: baseline;
    gap: 10px;
    margin: 0;
    font-size: 12px;
    color: var(--yap-muted);
  }
  .cancel {
    border: none;
    background: none;
    color: var(--yap-muted-55);
    font: inherit;
    font-size: 11.5px;
    cursor: pointer;
    padding: 0;
  }
  .cancel:hover {
    color: var(--yap-danger);
  }

  .resultcard {
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-lg);
    background: var(--yap-s2);
    margin-bottom: 12px;
  }
  .resulthead {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 10px 14px;
    border-bottom: 1px solid var(--yap-border-subtle);
  }
  .resulthead h3 {
    margin: 0;
    font-size: 12.5px;
    font-weight: 600;
  }
  .copybtn {
    border: none;
    background: none;
    color: var(--yap-primary);
    font: inherit;
    font-size: 11.5px;
    cursor: pointer;
    padding: 0;
  }
  .copybtn:hover {
    text-decoration: underline;
  }
  .resulttext {
    max-height: 320px;
    overflow-y: auto;
    padding: 12px 14px;
    font-size: 12.5px;
    line-height: 1.65;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  .errline {
    margin: 0 0 12px;
    font-size: 12px;
    color: var(--yap-danger);
    line-height: 1.5;
  }
  .hint {
    margin: 10px 0 0;
    font-size: 11px;
    color: var(--yap-muted-55);
  }
</style>
