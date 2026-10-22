// What can be configured, and how each setting is parsed, saved and stepped. Settings
// start from the server's, then this browser's saved changes, then the URL.

import { THEMES } from './fx.js';
import { clamp } from './util.js';

const STORAGE_KEY = 'coaster-queue';
const DEFAULT_PARKS = ['alton-towers', 'thorpe-park']; // fallback if the server can't be asked
const SLUG = /^[a-z0-9_-]+$/;
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
  { key: 'show', label: 'RIDE GROUPS', kind: 'list', def: [], fmt: (v) => (v.length ? `${v.length} CHOSEN` : 'ALL RIDES') },
  { key: 'park', label: 'SHOW PARK', kind: 'enum', options: (s) => [null, ...s.parks], def: null, fmt: (v, s) => (v ? s.feed.nameOf(v).toUpperCase() : 'ALL SELECTED') },
  { key: 'weather', label: 'WEATHER', kind: 'bool', def: true, fmt: ON_OFF },
  { key: 'feelsLike', param: 'feels_like', label: 'FEELS LIKE', kind: 'bool', def: true, fmt: ON_OFF },
  { key: 'clock24', label: 'CLOCK', kind: 'bool', def: true, fmt: (v) => (v ? '24 HOUR' : '12 HOUR') },
  { key: 'flip', label: 'FLIP TEXT', kind: 'bool', def: true, fmt: ON_OFF },
  { key: 'shine', label: 'SHINE SWEEP', kind: 'bool', def: true, fmt: ON_OFF },
];

export const byKey = (key) => SCHEMA.find((i) => i.key === key);
export const format = (item, value, state) => item.fmt(value, state);
const param = (item) => item.param ?? item.key;

const same = (a, b) => (Array.isArray(a) ? Array.isArray(b) && a.join() === b.join() : a === b);
const round = (v) => Math.round(v * 100) / 100;

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
    // Only the groups list may be empty (meaning every ride); "all" in a URL spells that.
    if (!ids.length) return item.key === 'show' ? [] : undefined;
    return [...new Set(ids)];
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

/** The server's settings, under the names used here. */
const fromServer = (server) => ({ ...server, showClosed: server?.show_closed, feelsLike: server?.feels_like });

// What each setting is when the viewer hasn't changed it: the server's value, else the built-in default.
let baseline = {};

/**
 * Settings from the built-in defaults, then the server's settings, then this
 * browser's saved changes, then the query string.
 */
export function load(params, server = {}) {
  const saved = stored();
  const fromHost = fromServer(server);
  const out = {};
  baseline = {};
  for (const item of SCHEMA) {
    baseline[item.key] = parse(item, fromHost[item.key]) ?? item.def;
    out[item.key] = parse(item, params.get(param(item))) ?? parse(item, saved[item.key]) ?? baseline[item.key];
  }
  return out;
}

/**
 * Remember what this viewer has changed from the baseline, and mirror those
 * changes into the URL for bookmarking. Untouched settings keep following the server.
 */
export function save(state, demo) {
  const params = new URLSearchParams();
  const changed = {};
  for (const item of SCHEMA) {
    const v = state[item.key];
    if (same(v, baseline[item.key])) continue;
    changed[item.key] = v;
    const text = item.kind === 'bool' ? (v ? '1' : '0') : v === null || (Array.isArray(v) && !v.length) ? 'all' : String(v);
    params.set(param(item), text);
  }
  if (demo) params.set('demo', '1');
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(changed));
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

/** Drop chosen ride groups the server doesn't know about. */
export function sanitiseGroups(state, groups) {
  const known = new Set(groups.map((g) => g.id));
  state.show = state.show.filter((id) => known.has(id));
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
