// Every big download's progress and Cancel, the same everywhere
// (src-tauri/src/shell/download.rs): the on-device AI models and their
// engine, the speech models, the meeting search model, Kokoro and the
// speaker engine. Each one's progress event carries its bytes so far and in
// all, the speed over the last few seconds (`bytesPerSec`), the time left
// at it (`etaSecs`) and what Cancel cancels (`cancelId`); `download_cancel
// {id}` stops it and takes its half-downloaded file away.
import { invoke } from '@tauri-apps/api/core';

/** The error a cancelled download ends with (Rust's `download::CANCELLED`). */
export const CANCELLED = 'Download cancelled';

/** Was this error a cancel? (Not a failure to show.) */
export function isCancelled(e) {
  return String(e?.message ?? e ?? '').includes(CANCELLED);
}

/** Cancel the download `id`; resolves `true` when one was running. */
export function cancelDownload(id) {
  if (!id) return Promise.resolve(false);
  return invoke('download_cancel', { id }).catch(() => false);
}

/** "820 MB", "1.2 GB", "640 KB". */
export function formatBytes(n) {
  const b = Number(n) || 0;
  if (b >= 1024 ** 3) return `${(b / 1024 ** 3).toFixed(1)} GB`;
  if (b >= 1024 ** 2) return `${Math.round(b / 1024 ** 2)} MB`;
  return `${Math.max(1, Math.round(b / 1024))} KB`;
}

/** "24 MB/s", "820 KB/s", "1.4 MB/s"; '' when not known yet. */
export function formatSpeed(bps) {
  const b = Number(bps) || 0;
  if (b <= 0) return '';
  if (b >= 10 * 1024 ** 2) return `${Math.round(b / 1024 ** 2)} MB/s`;
  if (b >= 1024 ** 2) return `${(b / 1024 ** 2).toFixed(1)} MB/s`;
  return `${Math.max(1, Math.round(b / 1024))} KB/s`;
}

/** "about 3 min left", "less than a minute left", "about 1 h 20 min left"; '' when not known. */
export function formatEta(secs) {
  if (secs == null || !Number.isFinite(Number(secs))) return '';
  const s = Math.max(0, Math.round(Number(secs)));
  if (s < 60) return s <= 5 ? 'almost done' : 'less than a minute left';
  const min = Math.round(s / 60);
  if (min < 60) return `about ${min} min left`;
  const h = Math.floor(min / 60);
  const m = min % 60;
  return m ? `about ${h} h ${m} min left` : `about ${h} h left`;
}

/**
 * A progress event's payload in one shape, whatever its feature calls the
 * fields: `{ percent, downloaded, total, bytesPerSec, etaSecs, cancelId }`
 * (bytes; `total` 0 when the server didn't say).
 */
export function fromEvent(p) {
  if (!p) return null;
  const mb = 1024 * 1024;
  const downloaded = p.downloadedBytes ?? p.downloaded ?? Math.round((p.downloaded_mb ?? p.downloadedMb ?? 0) * mb);
  const total = p.totalBytes ?? p.total ?? Math.round((p.total_mb ?? p.totalMb ?? 0) * mb);
  const percent = p.percent ?? (total ? Math.floor((downloaded / total) * 100) : 0);
  return { percent, downloaded, total, bytesPerSec: p.bytesPerSec ?? 0, etaSecs: p.etaSecs ?? null, cancelId: p.cancelId ?? '' };
}

/** "1.2 GB of 2.8 GB · 24 MB/s · about 1 min left" (what's known of it). */
export function downloadLine(p) {
  if (!p) return '';
  const parts = [];
  if (p.total > 0) parts.push(`${formatBytes(p.downloaded)} of ${formatBytes(p.total)}`);
  else if (p.downloaded > 0) parts.push(formatBytes(p.downloaded));
  const speed = formatSpeed(p.bytesPerSec);
  if (speed) parts.push(speed);
  const left = formatEta(p.etaSecs);
  if (left && p.total > 0) parts.push(left);
  return parts.join(' · ');
}
