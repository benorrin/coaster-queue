import { ROWS_Y, ROW_PITCH } from './board.js';
import { SCHEMA, byKey, change, format, save } from './settings.js';

const mod = (n, m) => ((n % m) + m) % m;

export function setupControls({ state, matrix, demo }) {
  const toast = (text) => (state.toast = { text, until: performance.now() + 1600 });

  /** Step a setting, apply any side effects and tell the viewer. */
  function nudge(key, dir = 1, quiet = false) {
    const item = byKey(key);
    const before = state[key];
    state[key] = change(item, before, dir, state);
    applied(key, before);
    if (!quiet) toast(`${item.label} ${format(item, state[key], state)}`);
  }

  function applied(key, before) {
    if (key === 'cols') matrix.setCols(state.cols);
    // Keep the current page and progress when the page time changes.
    if (key === 'dwell') state.slideTime *= state.dwell / before;
    if (key === 'park' || key === 'parks' || key === 'show') {
      if (state.park && !state.parks.includes(state.park)) state.park = null;
      state.slideTime = 0;
      state.pageAge = 0;
    }
    if (key === 'parks' || key === 'show') state.feed.poll();
  }

  function pickPark(i) {
    const park = i == null ? null : state.feed.parks[i];
    if (i != null && !park) return;
    state.park = park ? park.id : null;
    applied('park');
    toast(park ? park.name.toUpperCase() : 'ALL SELECTED PARKS');
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
  const picker = state.picker;
  // The picker edits either the chosen parks or the chosen ride groups.
  const pickerItems = () => (picker.key === 'show' ? state.feed.groups : state.feed.directory);
  const openPicker = (open = !picker.open, key = picker.key) => {
    picker.open = open;
    if (open) {
      picker.key = key;
      state.help = false;
      picker.index = Math.max(0, pickerItems().findIndex((p) => state[key].includes(p.id)));
    }
  };
  // Tick or untick the entry under the cursor; at least one park always stays chosen.
  const toggleItem = () => {
    const item = pickerItems()[picker.index];
    if (!item) return;
    const key = picker.key;
    const on = state[key].includes(item.id);
    if (key === 'parks' && on && state.parks.length === 1) return;
    const chosen = new Set(state[key]);
    on ? chosen.delete(item.id) : chosen.add(item.id);
    state[key] = pickerItems().map((p) => p.id).filter((id) => chosen.has(id));
    applied(key);
  };
  const pickerActions = {
    ArrowUp: () => (picker.index = mod(picker.index - 1, Math.max(1, pickerItems().length))),
    ArrowDown: () => (picker.index = mod(picker.index + 1, Math.max(1, pickerItems().length))),
    ' ': toggleItem,
    Enter: toggleItem,
    p: () => openPicker(false),
    r: () => openPicker(false),
    Escape: () => openPicker(false),
  };

  const step = (dir) => {
    const item = SCHEMA[menu.index];
    if (item.kind === 'list') openPicker(true, item.key);
    else nudge(item.key, dir, true);
  };

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
    p: () => openPicker(!picker.open, 'parks'),
    r: () => openPicker(!picker.open, 'show'),
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
    ...Object.fromEntries([...Array(9)].map((_, i) => [i + 1, () => pickPark(i)])),
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
    const act =
      (picker.open && pickerActions[key]) || (!picker.open && menu.open && menuActions[key]) || (!picker.open && actions[key]);
    if (!act) return;
    e.preventDefault();
    act();
    save(state, demo);
  });

  // Touch and mouse: left third goes back, anywhere else goes forward.
  const board = matrix.canvas;
  board.addEventListener('click', (e) => {
    if (picker.open) {
      // Tap a park to tick it, or tap above the list to close.
      const dot = matrix.dotAt(e.clientX, e.clientY);
      const row = Math.floor((dot.y - ROWS_Y) / ROW_PITCH);
      if (row < 0 || row >= picker.visible) return openPicker(false);
      picker.index = picker.offset + row;
      toggleItem();
      return save(state, demo);
    }
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
