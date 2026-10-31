// The latest releases' facts (version, date, size, SHA-256), read once per
// build from GitHub's public API. This runs at BUILD time only: the pages
// ship them as plain text, and a visitor's browser never contacts GitHub.
// Any failure (offline, rate-limited, a renamed asset) gives null, and the
// pages fall back to wording without numbers.

import { REPO, STABLE_INSTALLER, NIGHTLY_INSTALLER, FETCH_RELEASE } from '../brand';

export interface ReleaseInfo {
  /** "0.1.0" (stable) or "0.1.1-nightly.123" */
  version: string | null;
  /** "4 October 2026" */
  date: string | null;
  /** ISO date for <time datetime> */
  dateIso: string | null;
  /** "15.4 MB", as Windows Explorer counts it */
  size: string | null;
  /** 64 hex characters, or null when GitHub doesn't say */
  sha256: string | null;
}

const API = `https://api.github.com/repos/${REPO}`;
const HEADERS = { Accept: 'application/vnd.github+json', 'User-Agent': 'site-build' };
const enabled = FETCH_RELEASE && process.env.SITE_OFFLINE !== '1';

const fmtDate = (iso: string) =>
  new Date(iso).toLocaleDateString('en-GB', { day: 'numeric', month: 'long', year: 'numeric', timeZone: 'UTC' });
const fmtSize = (bytes: number) => `${(bytes / 1024 / 1024).toFixed(1)} MB`;

async function getJson(url: string): Promise<any | null> {
  try {
    const res = await fetch(url, { headers: HEADERS, signal: AbortSignal.timeout(8000) });
    if (!res.ok) return null;
    return await res.json();
  } catch {
    return null;
  }
}

function fromAsset(asset: any, version: string | null, when: string | null): ReleaseInfo {
  const digest: string | undefined = asset?.digest;
  return {
    version,
    date: when ? fmtDate(when) : null,
    dateIso: when ? when.slice(0, 10) : null,
    size: typeof asset?.size === 'number' ? fmtSize(asset.size) : null,
    sha256: digest?.startsWith('sha256:') ? digest.slice(7) : null,
  };
}

let stable: Promise<ReleaseInfo | null> | undefined;
let nightly: Promise<ReleaseInfo | null> | undefined;

/** The stable channel's latest release, or null. */
export function stableRelease(): Promise<ReleaseInfo | null> {
  stable ??= (async () => {
    if (!enabled) return null;
    const rel = await getJson(`${API}/releases/latest`);
    if (!rel) return null;
    const asset = rel.assets?.find((a: any) => a.name === STABLE_INSTALLER);
    const version = typeof rel.tag_name === 'string' ? rel.tag_name.replace(/^v/, '') : null;
    return fromAsset(asset, version, rel.published_at ?? null);
  })();
  return stable;
}

/** The rolling nightly pre-release, or null. Its version is in its latest.json. */
export function nightlyRelease(): Promise<ReleaseInfo | null> {
  nightly ??= (async () => {
    if (!enabled) return null;
    const rel = await getJson(`${API}/releases/tags/nightly`);
    if (!rel) return null;
    const asset = rel.assets?.find((a: any) => a.name === NIGHTLY_INSTALLER);
    const manifest = rel.assets?.find((a: any) => a.name === 'latest.json');
    const latest = manifest ? await getJson(manifest.browser_download_url) : null;
    const version = typeof latest?.version === 'string' ? latest.version : null;
    return fromAsset(asset, version, asset?.updated_at ?? null);
  })();
  return nightly;
}
