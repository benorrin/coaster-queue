import { THEMES } from './fx.js';

const STORAGE_KEY = 'coaster-queue';
const DEFAULT_PARKS = ['alton-towers', 'thorpe-park'];
const SLUG = /^[a-z0-9-]+$/;
const ON_OFF = (v) => (v ? 'ON' : 'OFF');

/**
 * Every user-tunable setting. Keys, the settings menu, the URL and saved
 * preferences are all driven from this one list.
 *   kind: 'num' steps between min and max, 'enum' cycles `options` (or options(state)),
 *   'bool' toggles, 'list' is edited on its own screen.
 *   param: query-string name when it differs from `key`.
 */
export const SCHEMA = [
  { key: 'dwell', label: 'PAGE TIME', kind: 'num', min: 3, max: 60, step: 1, def: 8, fmt: (v) => `${v} SEC` },
  { key: 'theme', label: 'THEME', kind: 'enum', options: THEMES, def: 'rgb', fmt: (v) => v.toUpperCase() },
  { key: 'brightness', label: 'BRIGHTNESS', kind: 'num', min: 0.1, max: 1, step: 0.1, def: 1, fmt: (v) => `${Math.round(v * 100)}%` },
  { key: 'cols', label: 'DOT DENSITY', kind: 'enum', options: [120, 160, 200, 256, 320], def: 200, fmt: (v) => `${v} WIDE` },
  { key: 'gradients', label: 'GRADIENTS', kind: 'bool', def: true, fmt: ON_OFF },
  { key: 'showClosed', param: 'closed', label: 'CLOSED RIDES', kind: 'bool', def: false, fmt: ON_OFF },
  { key: 'parks', label: 'CHOOSE PARKS', kind: 'list', def: DEFAULT_PARKS, fmt: (v) => `${v.length} SELECTED` },
  { key: 'park', label: 'SHOW PARK', kind: 'enum', options: (s) => [null, ...s.parks], def: null, fmt: (v, s) => (v ? s.feed.nameOf(v).toUpperCase() : 'ALL SELECTED') },
  { key: 'weather', label: 'WEATHER', kind: 'bool', def: true, fmt: ON_OFF },
  { key: 'clock24', label: 'CLOCK', kind: 'bool', def: true, fmt: (v) => (v ? '24 HOUR' : '12 HOUR') },
  { key: 'flip', label: 'FLIP TEXT', kind: 'bool', def: true, fmt: ON_OFF },
  { key: 'shine', label: 'SHINE SWEEP', kind: 'bool', def: true, fmt: ON_OFF },
];

export const byKey = (key) => SCHEMA.find((i) => i.key === key);
export const format = (item, value, state) => item.fmt(value, state);
const param = (item) => item.param ?? item.key;

const same = (a, b) => (Array.isArray(a) ? Array.isArray(b) && a.join() === b.join() : a === b);
const round = (v) => Math.round(v * 100) / 100;
const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v));

/** Turn a stored or query-string value into a valid setting, or undefined. */
function parse(item, raw) {
  if (raw == null) return undefined;
  if (item.kind === 'bool') {
    if (typeof raw === 'boolean') return raw;
    return { 1: true, true: true, on: true, 0: false, false: false, off: false }[String(raw).toLowerCase()];
  }
  if (item.kind === 'num') {
    const v = parseFloat(raw);
    return Number.isFinite(v) ? clamp(round(v), item.min, item.max) : undefined;
  }
  if (item.key === 'cols') {
    // Any sensible width is allowed from the URL even if the menu only offers presets.
    const v = parseFloat(raw);
    return Number.isFinite(v) ? clamp(Math.round(v), 64, 400) : undefined;
  }
  if (item.kind === 'list') {
    const ids = (Array.isArray(raw) ? raw : String(raw).split(',')).filter((id) => SLUG.test(id));
    return ids.length ? [...new Set(ids)] : undefined;
  }
  if (item.key === 'park') {
    const v = String(raw).toLowerCase();
    return v === 'all' ? null : SLUG.test(v) ? v : undefined;
  }
  return item.options.find((o) => String(o) === String(raw));
}

function stored() {
  try {
    return JSON.parse(localStorage.getItem(STORAGE_KEY)) ?? {};
  } catch {
    return {};
  }
}

/** Settings from defaults, then saved preferences, then the query string. */
export function load(params) {
  const saved = stored();
  const out = {};
  for (const item of SCHEMA) {
    out[item.key] = parse(item, params.get(param(item))) ?? parse(item, saved[item.key]) ?? item.def;
  }
  return out;
}

/** Remember the settings, and mirror the non-default ones into the URL for bookmarking. */
export function save(state, demo) {
  const params = new URLSearchParams();
  const all = {};
  for (const item of SCHEMA) {
    const v = state[item.key];
    all[item.key] = v;
    if (same(v, item.def)) continue;
    params.set(param(item), item.kind === 'bool' ? (v ? '1' : '0') : v === null ? 'all' : String(v));
  }
  if (demo) params.set('demo', '1');
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(all));
  } catch {}
  const qs = params.toString();
  try {
    history.replaceState(null, '', qs ? `?${qs}` : location.pathname);
  } catch {}
}

/** The value one step from `value` in direction `dir` (+1 or -1). */
export function change(item, value, dir, state) {
  if (item.kind === 'bool') return !value;
  if (item.kind === 'num') return clamp(round(value + dir * item.step), item.min, item.max);
  if (item.kind === 'list') return value;
  const options = typeof item.options === 'function' ? item.options(state) : item.options;
  const i = options.findIndex((o) => o === value);
  const n = options.length;
  return options[i < 0 ? (dir > 0 ? 0 : n - 1) : (i + dir + n) % n];
}

/** Drop chosen parks the server doesn't know about; never leave the selection empty. */
export function sanitise(state, directory) {
  const known = new Set(directory.map((p) => p.id));
  let parks = state.parks.filter((id) => known.has(id));
  if (!parks.length) parks = DEFAULT_PARKS.filter((id) => known.has(id));
  if (!parks.length && directory.length) parks = [directory[0].id];
  state.parks = parks;
  if (state.park && !parks.includes(state.park)) state.park = null;
}
