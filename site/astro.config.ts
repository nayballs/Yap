import { defineConfig } from 'astro/config';
import fs from 'node:fs';
import { fileURLToPath } from 'node:url';
import { SITE_URL, INDEXABLE } from './src/brand';

// Everything the page itself may load comes from this site: no other host,
// no inline script or style without a hash (Astro adds the hashes to a
// <meta> CSP per page). The headers a <meta> can't carry go in _headers.
const CSP_DIRECTIVES = [
  "default-src 'self'",
  "img-src 'self' data:",
  "font-src 'self'",
  "connect-src 'self'",
  "media-src 'self'",
  "object-src 'none'",
  "base-uri 'self'",
  "form-action 'self'",
];

/**
 * Writes dist/_headers for the static host (Cloudflare Workers static assets
 * and Pages both read it): framing, referrer, sniffing, permissions, and
 * `X-Robots-Tag: noindex` while the site is a preview (brand.ts INDEXABLE).
 */
function hostingHeaders() {
  return {
    name: 'hosting-headers',
    hooks: {
      'astro:build:done': ({ dir }: { dir: URL }) => {
        const lines = [
          '/*',
          `  Content-Security-Policy: frame-ancestors 'none'`,
          '  X-Frame-Options: DENY',
          '  X-Content-Type-Options: nosniff',
          '  Referrer-Policy: strict-origin-when-cross-origin',
          '  Permissions-Policy: camera=(), microphone=(), geolocation=(), interest-cohort=()',
          '  Cross-Origin-Opener-Policy: same-origin',
          ...(INDEXABLE ? ['  Strict-Transport-Security: max-age=31536000'] : ['  X-Robots-Tag: noindex, nofollow']),
          '',
          '/_astro/*',
          '  Cache-Control: public, max-age=31536000, immutable',
          '',
        ];
        fs.writeFileSync(fileURLToPath(new URL('_headers', dir)), lines.join('\n'));
      },
    },
  };
}

export default defineConfig({
  site: SITE_URL,
  trailingSlash: 'ignore',
  build: {
    format: 'directory',
    // Stylesheets as files, never inline: the CSP stays 'self'.
    inlineStylesheets: 'never',
  },
  devToolbar: { enabled: false },
  // No Markdown code blocks on this site; Shiki's inline styles would need 'unsafe-inline'.
  markdown: { syntaxHighlight: false },
  security: {
    csp: { directives: CSP_DIRECTIVES },
  },
  vite: {
    build: {
      // Small assets as files too, not data: URIs or inline scripts.
      assetsInlineLimit: 0,
    },
    server: {
      // The pages reuse the app's screenshots from the repo's docs/ folder.
      fs: { allow: ['..'] },
    },
  },
  integrations: [hostingHeaders()],
});
