import { THEMES } from './fx.js';

const STORAGE_KEY = 'coaster-queue';
const ON_OFF = (v) => (v ? 'ON' : 'OFF');

/**
 * Every user-tunable setting. Keys, the settings menu, the URL and saved
 * preferences are all driven from this one list.
 *   kind: 'num' steps between min and max, 'enum' cycles `options`, 'bool' toggles.
 *   param: query-string name when it differs from `key`.
 */
export const SCHEMA = [
  { key: 'dwell', label: 'PAGE TIME', kind: 'num', min: 3, max: 60, step: 1, def: 8, fmt: (v) => `${v} SEC` },
  { key: 'theme', label: 'THEME', kind: 'enum', options: THEMES, def: 'rgb', fmt: (v) => v.toUpperCase() },
  { key: 'brightness', label: 'BRIGHTNESS', kind: 'num', min: 0.1, max: 1, step: 0.1, def: 1, fmt: (v) => `${Math.round(v * 100)}%` },
  { key: 'cols', label: 'DOT DENSITY', kind: 'enum', options: [120, 160, 200, 256, 320], def: 200, fmt: (v) => `${v} WIDE` },
  { key: 'gradients', label: 'GRADIENTS', kind: 'bool', def: true, fmt: ON_OFF },
  { key: 'showClosed', param: 'closed', label: 'CLOSED RIDES', kind: 'bool', def: false, fmt: ON_OFF },
  { key: 'park', label: 'PARK', kind: 'enum', options: [null, 'alton', 'thorpe'], def: null, fmt: (v) => (v ? v.toUpperCase() : 'ALL') },
  { key: 'weather', label: 'WEATHER', kind: 'bool', def: true, fmt: ON_OFF },
  { key: 'clock24', label: 'CLOCK', kind: 'bool', def: true, fmt: (v) => (v ? '24 HOUR' : '12 HOUR') },
  { key: 'flip', label: 'FLIP TEXT', kind: 'bool', def: true, fmt: ON_OFF },
  { key: 'shine', label: 'SHINE SWEEP', kind: 'bool', def: true, fmt: ON_OFF },
];

export const byKey = (key) => SCHEMA.find((i) => i.key === key);
export const format = (item, value) => item.fmt(value);
const param = (item) => item.param ?? item.key;

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
  if (item.key === 'park' && String(raw).toLowerCase() === 'all') return null;
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
    if (v === item.def) continue;
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
export function change(item, value, dir) {
  if (item.kind === 'bool') return !value;
  if (item.kind === 'num') return clamp(round(value + dir * item.step), item.min, item.max);
  const i = item.options.findIndex((o) => o === value);
  const n = item.options.length;
  return item.options[i < 0 ? (dir > 0 ? 0 : n - 1) : (i + dir + n) % n];
}
