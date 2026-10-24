// Upload's job (UploadView.svelte), kept for the main window's life:
// leaving the view mid-job — another view, or Chat opening by itself — used
// to unmount it with its state and its `yap-upload-*` listeners, so coming
// back showed an empty page with no progress or Cancel, the "Transcription
// complete" toast never came, and a retry said Yap was busy (code review
// 2026-10-23 #8). ControlPanel starts it (initUpload), as it does the update
// store; the view only shows it and asks it to act.
//
// Rust runs the job (pipeline.rs `transcribe_file`: decode → ~60 s chunks
// on the warm engine) and says how it goes: `yap-upload-progress {percent,
// chunksTotal, chunksCompleted}`, `-done {text, file}`, `-error`,
// `-cancelled`.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { toast } from './ui/toast.svelte.js';

export const upload = $state({
  /** idle | selected | transcribing | complete | error */
  state: 'idle',
  /** { name, path, size } */
  file: null,
  result: null,
  error: null,
  progress: 0,
  /** { chunksTotal, chunksCompleted } once chunks are under way */
  chunkProgress: null,
});

let started = false;

function formatFileSize(bytes) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

/** Follow the job's events. Idempotent; the main window calls it. */
export function initUpload() {
  if (started) return;
  started = true;
  listen('yap-upload-progress', (e) => {
    const p = e.payload || {};
    upload.progress = p.percent ?? 0;
    upload.chunkProgress = p.chunksTotal > 0 ? { chunksTotal: p.chunksTotal, chunksCompleted: p.chunksCompleted } : null;
  }).catch(() => {});
  listen('yap-upload-done', (e) => {
    upload.progress = 100;
    upload.result = e.payload?.text || '';
    upload.state = 'complete';
    toast({ title: 'Transcription complete', description: e.payload?.file || '', variant: 'success' });
  }).catch(() => {});
  listen('yap-upload-error', (e) => {
    upload.error = String(e.payload || 'Transcription failed');
    upload.progress = 0;
    upload.state = upload.file ? 'error' : 'idle';
    toast({ title: 'Transcription failed', description: upload.error, variant: 'destructive' });
  }).catch(() => {});
  listen('yap-upload-cancelled', () => {
    upload.progress = 0;
    upload.chunkProgress = null;
    upload.state = upload.file ? 'selected' : 'idle';
  }).catch(() => {});
}

/** The file to transcribe, by its path (picked or dropped). */
export async function chooseFile(path) {
  try {
    const info = await invoke('audio_file_info', { path });
    upload.file = { name: info.name, path, size: formatFileSize(info.size) };
    upload.state = 'selected';
    upload.error = null;
    upload.result = null;
  } catch (e) {
    upload.error = String(e);
    upload.state = 'error';
  }
}

/** Say what went wrong before a job (an unsupported file, the picker). */
export function failed(message) {
  upload.error = message;
  upload.state = 'error';
}

export async function transcribe() {
  if (!upload.file || upload.state === 'transcribing') return;
  upload.state = 'transcribing';
  upload.error = null;
  upload.progress = 0;
  upload.chunkProgress = null;
  try {
    // Progress and the result arrive as events (initUpload).
    await invoke('transcribe_file', { path: upload.file.path });
  } catch (e) {
    upload.error = String(e);
    upload.state = 'error';
  }
}

export function cancel() {
  invoke('cancel_file_transcription').catch(() => {});
}

/** Back to the drop zone (not during a job). */
export function reset() {
  if (upload.state === 'transcribing') return;
  upload.state = 'idle';
  upload.file = null;
  upload.result = null;
  upload.error = null;
  upload.progress = 0;
  upload.chunkProgress = null;
}
