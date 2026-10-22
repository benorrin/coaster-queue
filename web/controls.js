import { ROWS_Y, ROW_PITCH } from './board.js';
import { SCHEMA, byKey, change, format, save } from './settings.js';

const mod = (n, m) => ((n % m) + m) % m;

export function setupControls({ state, matrix, demo }) {
  const toast = (text) => (state.toast = { text, until: performance.now() + 1600 });

  /** Step a setting, apply any side effects and tell the viewer. */
  function nudge(key, dir = 1, quiet = false) {
    const item = byKey(key);
    const before = state[key];
    state[key] = change(item, before, dir);
    applied(key, before);
    if (!quiet) toast(`${item.label} ${format(item, state[key])}`);
  }

  function applied(key, before) {
    if (key === 'cols') matrix.setCols(state.cols);
    // Keep the current page and progress when the page time changes.
    if (key === 'dwell') state.slideTime *= state.dwell / before;
    if (key === 'park') {
      state.slideTime = 0;
      state.pageAge = 0;
    }
  }

  function pickPark(i) {
    const park = i == null ? null : state.feed.parks[i];
    if (i != null && !park) return;
    state.park = park ? park.id : null;
    applied('park');
    toast(`PARK ${park ? park.name.toUpperCase() : 'ALL'}`);
  }

  const turn = (delta) => {
    const n = Math.max(1, state.slideCount);
    const idx = mod(Math.floor(state.slideTime / state.dwell), n);
    state.slideTime = mod(idx + delta, n) * state.dwell + 0.001;
    state.pageAge = 0;
  };

  const menu = state.menu;
  const openMenu = (open = !menu.open) => {
    menu.open = open;
    if (open) state.help = false;
  };
  const step = (dir) => nudge(SCHEMA[menu.index].key, dir, true);

  // While the menu is open, arrows move and change values instead of paging.
  const menuActions = {
    ArrowUp: () => (menu.index = mod(menu.index - 1, SCHEMA.length)),
    ArrowDown: () => (menu.index = mod(menu.index + 1, SCHEMA.length)),
    ArrowLeft: () => step(-1),
    ArrowRight: () => step(1),
    Enter: () => step(1),
    ' ': () => step(1),
    s: () => openMenu(false),
    Escape: () => openMenu(false),
  };

  const actions = {
    s: () => openMenu(),
    ' ': () => {
      state.paused = !state.paused;
      toast(state.paused ? 'PAUSED' : 'RESUMED');
    },
    ArrowRight: () => turn(1),
    ArrowLeft: () => turn(-1),
    ArrowUp: () => nudge('brightness', 1),
    ArrowDown: () => nudge('brightness', -1),
    t: () => nudge('theme'),
    g: () => nudge('gradients'),
    c: () => nudge('showClosed'),
    d: () => nudge('cols'),
    w: () => nudge('weather'),
    0: () => pickPark(null),
    1: () => pickPark(0),
    2: () => pickPark(1),
    f: () => {
      if (document.fullscreenElement) document.exitFullscreen();
      else document.documentElement.requestFullscreen?.().catch(() => {});
    },
    h: () => {
      openMenu(false);
      state.help = !state.help;
    },
    '?': () => {
      openMenu(false);
      state.help = !state.help;
    },
    Escape: () => (state.help = false),
  };

  addEventListener('keydown', (e) => {
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    const key = e.key.length === 1 ? e.key.toLowerCase() : e.key;
    const act = (menu.open && menuActions[key]) || actions[key];
    if (!act) return;
    e.preventDefault();
    act();
    save(state, demo);
  });

  // Touch and mouse: left third goes back, anywhere else goes forward.
  const board = matrix.canvas;
  board.addEventListener('click', (e) => {
    if (!menu.open) return turn(e.clientX < board.clientWidth / 3 ? -1 : 1);
    // In the menu: tap above the list to close, or tap a row's left/right half to step it.
    const dot = matrix.dotAt(e.clientX, e.clientY);
    const row = Math.floor((dot.y - ROWS_Y) / ROW_PITCH);
    if (row < 0 || row >= menu.visible) return openMenu(false);
    menu.index = menu.offset + row;
    step(dot.x < matrix.cols / 2 ? -1 : 1);
    save(state, demo);
  });
  document.getElementById('settings-btn')?.addEventListener('click', () => {
    openMenu();
    save(state, demo);
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
