# The website

The product's own website: a static [Astro](https://astro.build) site (pinned, see
`package.json`) with no analytics, no cookies, no third-party requests and self-hosted
fonts. It's a **`noindex` preview** until it launches, and **the name is a placeholder**.

The plan it follows (pages, copy, the decisions still open) is in the private docs repo:
`docs/website/site-plan.md`.

## Run, build, preview

Node 22.23 or newer. From this folder:

```bash
npm ci               # once
npm run dev          # http://localhost:4321, live reload
npm run build        # the static site in dist/
npm run preview      # serve dist/ to check the build
npm run screenshots  # full-page shots of every page into screenshots/ (git-ignored)
```

Every `astro` command runs through `scripts/astro.mjs`, which switches off Astro's own
usage telemetry. `dev` and `build` first regenerate `src/styles/tokens.css` from the app's
`../src/app.css` (`scripts/tokens.mjs`), so the site's colours follow the app's, light and
dark. `build` also runs the name check (below), and reads the latest stable and nightly
releases' version, date, size and SHA-256 from GitHub's API (at build time only; set
`SITE_OFFLINE=1` to skip it, and the pages fall back to wording without numbers).

`npm run screenshots` needs the build first. It serves `dist/` with `astro preview`, drives the
installed Chrome or Edge with Playwright (desktop 1440 px and phone 390 px, light and dark),
and fails if a page requests another host, sets a cookie, logs a console error (a CSP
violation is one) or scrolls sideways on a phone.

## Rename

The name lives in one file, `src/brand.ts`:

1. `NAME` (titles, meta, alt text, the favicon's initial and the copy all follow);
2. `DOMAIN` once it's bought;
3. `REPO` and the installer file names, when the release pipeline renames them;
4. `FORMER_NAME` and `RENAMED_ON`, for the footer's "was called … until …" line.

`npm run check:name` (part of `build`) fails if the current name or a shortlisted one is
spelled anywhere else under `src/`, `public/` or `astro.config.ts`.

## Go public

Set `INDEXABLE = true` in `src/brand.ts`. That drops the `noindex` meta tag and the
`X-Robots-Tag` header, opens `robots.txt` (with the sitemap), adds HSTS, and hides the
maintainer's dashed **Draft notes** and the preview strip. Nothing is deployed from here:
the plan's host is Cloudflare (Workers static assets or Pages, serving `dist/`, which carries
its own `_headers`).

## How it's put together

- `src/brand.ts`: the name, domain, links and switches.
- `src/layouts/Layout.astro`: head (meta, CSP via Astro's `security.csp`, the theme script),
  header, the sheets, footer, and the scripts that adapt the download buttons to the
  visitor's device (Windows, another computer, a phone), raise sections into view as they
  scroll in, and switch the draft notes on and off (preview builds).
- The look: the page is the app's window frame with the content on **sheets** inset in it,
  paper ones and a few ink ones (`.sheet`, `.sheet-ink` in `src/styles/global.css`). An ink
  sheet carries `.scheme-dark`, so everything drawn on it is the app's own dark theme
  (`scripts/tokens.mjs` writes that scope too). Headlines are Figtree, large and tight; EB
  Garamond only for numerals and the notepad's titles, as in the app.
- `src/components/scenes/`: the **live mock-ups** of the app, all HTML and CSS in the app's
  tokens, every person and message made up: `HeroDesk` (a dictation playing on a desk, the
  big amber key; it plays once on view, then on a click or the real F9), `Steps` (how it
  works: the picture stays put while the steps scroll), `TidyDiff` (said → marked → typed,
  with the app's word diff; `src/lib/diff.ts` works it out at build time), `AppKinds` (the
  rules per kind of window), `MakeItYours`, `NotepadScene`, `AskScene`, `ChatScene` (Chat
  with a document beside it), `VoiceScene` (the Voice Assistant's reading card) and
  `NetworkScene` (Settings → Network connections). Their shared atoms (a Windows window, the
  bar's capsule and waveform, chips, tabs) are in `src/styles/scenes.css`; `src/lib/tabs.ts`
  runs the tabs. Without script each scene shows its finished state; with reduced motion
  nothing plays by itself.
- `src/components/`: `DownloadButton`, `Faq`, `LeavesTable` ("what leaves your PC"),
  `Placeholder` (a visual not captured yet, with its id from the plan's asset list),
  `DraftNote` (preview only, behind the preview strip's switch), `PageHero`, `Closing`, and
  the small parts.
- `src/data/`: the FAQ (one list, each page shows its own) and the "what leaves your PC"
  rows. Every claim should be true of the app today; check `docs/architecture/` before
  changing one.
- `src/pages/`: one file per page, plus `robots.txt`, `sitemap.xml` and `favicon.svg`
  generated from `brand.ts`.
- `public/theme.js`: puts the saved theme, `js` and the draft-notes choice on the page
  before it paints.

Fonts are Figtree and EB Garamond from Fontsource (both SIL Open Font License 1.1, their
licence files in `node_modules/@fontsource-variable/*/LICENSE`), bundled into the build.
`Shot` takes a real screenshot when there is one (optimised at build); none is used today.
