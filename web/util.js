// Small helpers shared across modules.

export const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v));

/** Modulo that never goes negative, so indexes wrap in both directions. */
export const mod = (n, m) => ((n % m) + m) % m;

/** FNV-1a hash of a string, as an unsigned 32-bit number. */
export function hashString(s) {
  let h = 2166136261;
  for (const c of s) h = Math.imul(h ^ c.charCodeAt(0), 16777619);
  return h >>> 0;
}

/** Fetch a JSON endpoint from the server, throwing on a bad status. */
export async function getJson(path) {
  const res = await fetch(path, { cache: 'no-store' });
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  return res.json();
}
