// The sitemap: every page, from the pages folder, at the site's address.
import { SITE_URL } from '../brand';

const pages = import.meta.glob('./**/*.astro');

export function GET() {
  const paths = Object.keys(pages)
    .map((f) => f.replace(/^\.\//, '/').replace(/\.astro$/, '').replace(/\/index$/, '/'))
    .filter((p) => p !== '/404')
    .map((p) => (p.endsWith('/') ? p : `${p}/`))
    .sort();
  const urls = paths.map((p) => `  <url><loc>${new URL(p, SITE_URL).href}</loc></url>`).join('\n');
  const xml = `<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n${urls}\n</urlset>\n`;
  return new Response(xml, { headers: { 'Content-Type': 'application/xml; charset=utf-8' } });
}
