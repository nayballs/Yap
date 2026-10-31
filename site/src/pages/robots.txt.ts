// robots.txt from brand.ts: while the site is a preview it asks every crawler
// to stay out; once INDEXABLE, it welcomes them and points at the sitemap.
import { INDEXABLE, SITE_URL } from '../brand';

export function GET() {
  const body = INDEXABLE
    ? `User-agent: *\nAllow: /\n\nSitemap: ${SITE_URL}/sitemap.xml\n`
    : `# A preview: not for search engines yet.\nUser-agent: *\nDisallow: /\n`;
  return new Response(body, { headers: { 'Content-Type': 'text/plain; charset=utf-8' } });
}
