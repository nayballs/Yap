// The favicon: the amber keycap with the name's initial, built from brand.ts
// so the rename changes it too. (A real icon waits for the logo, A18.)
import { NAME_INITIAL } from '../brand';

export function GET() {
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32">
  <rect x="1" y="3" width="30" height="28" rx="8" fill="#d78a17"/>
  <rect x="1" y="1" width="30" height="27" rx="8" fill="#f6a93c"/>
  <text x="16" y="20.5" text-anchor="middle" font-family="Segoe UI, system-ui, sans-serif" font-size="16" font-weight="700" fill="#3d2c07">${NAME_INITIAL}</text>
</svg>
`;
  return new Response(svg, { headers: { 'Content-Type': 'image/svg+xml' } });
}
