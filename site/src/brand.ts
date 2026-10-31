/**
 * The product's name and everything that follows from it, in ONE place.
 *
 * PLACEHOLDER: the name below stays until the rename. On the day:
 *   1. NAME (the initial, titles, meta, alt text and the favicon follow);
 *   2. DOMAIN once it's bought;
 *   3. REPO and the installer file names, when the release pipeline renames them;
 *   4. FORMER_NAME / RENAMED_ON, for the footer's "was called … until …" line;
 *   5. INDEXABLE = true only when the site goes public (drops `noindex`,
 *      opens robots.txt, hides the draft notes).
 *
 * Nothing else in site/ may spell the name: `npm run check:name` (part of
 * `npm run build`) fails if it does.
 */

/** The product's name. Placeholder until the rename. */
export const NAME = 'Yap';
export const NAME_INITIAL = NAME.charAt(0).toUpperCase();

/** The site's own address. Placeholder: no domain is bought yet. */
export const DOMAIN = 'example.com';
export const SITE_URL = `https://${DOMAIN}`;

/**
 * The preview switch. false = a private draft: every page says
 * `<meta name="robots" content="noindex">`, robots.txt disallows everything,
 * the hosting headers add `X-Robots-Tag: noindex`, and maintainer-facing draft
 * notes show on the pages. Flip to true when the site goes public.
 */
export const INDEXABLE = false;
export const SHOW_DRAFT_NOTES = !INDEXABLE;

/** The footer's name-history line, once there's history to tell. */
export const FORMER_NAME: string | null = null;
export const RENAMED_ON: string | null = null;

/** The default dictation shortcut (the app's default). */
export const HOTKEY = 'F9';

/** The public source repository and the links built from it. */
export const REPO = 'nayballs/Yap';
export const REPO_URL = `https://github.com/${REPO}`;
export const RELEASES_URL = `${REPO_URL}/releases`;
export const LATEST_RELEASE_URL = `${REPO_URL}/releases/latest`;
export const NIGHTLY_RELEASE_URL = `${REPO_URL}/releases/tag/nightly`;
export const ISSUES_URL = `${REPO_URL}/issues`;
export const SECURITY_REPORT_URL = `${REPO_URL}/security/advisories/new`;
export const SECURITY_POLICY_URL = `${REPO_URL}/blob/main/SECURITY.md`;
export const LICENSE_URL = `${REPO_URL}/blob/main/LICENSE`;
export const DOCS_URL = `${REPO_URL}/tree/main/docs/architecture`;

/**
 * The installers. Constant names, so these links always fetch the newest
 * build of each channel. They change with the release pipeline, not with NAME.
 */
export const STABLE_INSTALLER = 'Yap-setup.exe';
export const NIGHTLY_INSTALLER = 'Yap-nightly-setup.exe';
export const DOWNLOAD_STABLE = `${REPO_URL}/releases/latest/download/${STABLE_INSTALLER}`;
export const DOWNLOAD_NIGHTLY = `${REPO_URL}/releases/download/nightly/${NIGHTLY_INSTALLER}`;

/**
 * Read the latest release's version, date, size and SHA-256 from GitHub's API
 * at build time (never in the visitor's browser). Off, or offline, the pages
 * fall back to "the latest version" with no numbers. `SITE_OFFLINE=1` skips it.
 */
export const FETCH_RELEASE = true;

/**
 * The privacy policy that applies today (the optional account service's),
 * until the site has its own canonical policy.
 */
export const CURRENT_POLICY_URL = 'https://auth.contextmirror.com/privacy';

/** One-line descriptions, used in meta tags and the home page. */
export const TAGLINE = `Free, open-source dictation, meeting notes and a voice assistant for Windows. Speech and AI run on your PC.`;
