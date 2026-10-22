import { THEMES } from './fx.js';

const DENSITIES = [120, 160, 200, 256, 320];
export const DEFAULTS = { theme: 'rgb', cols: 200, dwell: 8, brightness: 1, glow: 1, park: null };

const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v));
const mod = (n, m) => ((n % m) + m) % m;

/** Read settings from the query string, falling back to defaults. */
export function readSettings(params) {
  const num = (key, fallback, lo, hi) => {
    const v = parseFloat(params.get(key));
    return Number.isFinite(v) ? clamp(v, lo, hi) : fallback;
  };
  const theme = params.get('theme');
  return {
    theme: THEMES.includes(theme) ? theme : DEFAULTS.theme,
    cols: num('cols', DEFAULTS.cols, 64, 400),
    dwell: num('dwell', DEFAULTS.dwell, 2, 120),
    brightness: num('brightness', DEFAULTS.brightness, 0.1, 1),
    glow: num('glow', DEFAULTS.glow, 0, 1.5),
    showClosed: params.get('closed') === '1',
    park: params.get('park') || DEFAULTS.park,
  };
}

/** Keep the URL in step with the settings so a tuned board can be bookmarked. */
function writeSettings(state, demo) {
  const params = new URLSearchParams();
  for (const key of ['theme', 'cols', 'dwell', 'brightness', 'glow', 'park']) {
    const v = state[key];
    if (v === DEFAULTS[key] || v == null) continue;
    params.set(key, typeof v === 'number' ? String(Math.round(v * 100) / 100) : v);
  }
  if (state.showClosed) params.set('closed', '1');
  if (demo) params.set('demo', '1');
  const qs = params.toString();
  try {
    history.replaceState(null, '', qs ? `?${qs}` : location.pathname);
  } catch {}
}

export function setupControls({ state, matrix, demo }) {
  const toast = (text) => (state.toast = { text, until: performance.now() + 1600 });
  const save = () => writeSettings(state, demo);

  const turn = (delta) => {
    const n = Math.max(1, state.slideCount);
    const idx = mod(Math.floor(state.slideTime / state.dwell), n);
    state.slideTime = mod(idx + delta, n) * state.dwell + 0.001;
    state.pageAge = 0;
  };

  const actions = {
    ' ': () => {
      state.paused = !state.paused;
      toast(state.paused ? 'PAUSED' : 'RESUMED');
    },
    ArrowRight: () => turn(1),
    ArrowLeft: () => turn(-1),
    t: () => {
      state.theme = THEMES[(THEMES.indexOf(state.theme) + 1) % THEMES.length];
      toast(`THEME ${state.theme.toUpperCase()}`);
    },
    ArrowUp: () => {
      state.brightness = clamp(Math.round((state.brightness + 0.1) * 10) / 10, 0.1, 1);
      toast(`BRIGHTNESS ${Math.round(state.brightness * 100)}%`);
    },
    ArrowDown: () => {
      state.brightness = clamp(Math.round((state.brightness - 0.1) * 10) / 10, 0.1, 1);
      toast(`BRIGHTNESS ${Math.round(state.brightness * 100)}%`);
    },
    g: () => {
      state.glow = state.glow >= 1 ? 0 : state.glow + 0.5;
      toast(state.glow ? `GLOW ${Math.round(state.glow * 100)}%` : 'GLOW OFF');
    },
    c: () => {
      state.showClosed = !state.showClosed;
      toast(state.showClosed ? 'CLOSED RIDES SHOWN' : 'CLOSED RIDES HIDDEN');
    },
    d: () => {
      const next = DENSITIES.find((n) => n > matrix.targetCols) ?? DENSITIES[0];
      matrix.setCols(next);
      state.cols = next;
      toast(`${next} DOTS WIDE`);
    },
    0: () => {
      state.park = null;
      state.slideTime = 0;
      toast('ALL PARKS');
    },
    1: () => pickPark(0),
    2: () => pickPark(1),
    f: () => {
      if (document.fullscreenElement) document.exitFullscreen();
      else document.documentElement.requestFullscreen?.().catch(() => {});
    },
    h: () => (state.help = !state.help),
    '?': () => (state.help = !state.help),
    Escape: () => (state.help = false),
  };

  function pickPark(i) {
    const park = state.feed.parks[i];
    if (!park) return;
    state.park = park.id;
    state.slideTime = 0;
    state.pageAge = 0;
    toast(park.name.toUpperCase());
  }

  addEventListener('keydown', (e) => {
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    const act = actions[e.key.length === 1 ? e.key.toLowerCase() : e.key];
    if (!act) return;
    e.preventDefault();
    act();
    save();
  });

  // Touch and mouse: left third goes back, anywhere else goes forward.
  const board = matrix.canvas;
  board.addEventListener('click', (e) => {
    turn(e.clientX < board.clientWidth / 3 ? -1 : 1);
  });

  // Hide the cursor when idle so a kiosk screen stays clean.
  let idle;
  const wake = () => {
    document.body.classList.remove('idle');
    clearTimeout(idle);
    idle = setTimeout(() => document.body.classList.add('idle'), 2500);
  };
  addEventListener('pointermove', wake);
  wake();

  // Keep the screen on while the board is up.
  const hold = async () => {
    try {
      await navigator.wakeLock?.request('screen');
    } catch {}
  };
  hold();
  document.addEventListener('visibilitychange', () => document.visibilityState === 'visible' && hold());
}
