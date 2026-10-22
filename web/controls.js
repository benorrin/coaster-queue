// Keyboard, mouse and touch controls, and the kiosk niceties (idle cursor, wake lock).

import { ROWS_Y, ROW_PITCH } from './layout.js';
import { SCHEMA, byKey, change, format, save } from './settings.js';
import { mod } from './util.js';

const TOAST_MS = 1600;
const CURSOR_IDLE_MS = 2500;

export function setupControls({ state, matrix, demo }) {
  const { menu, picker } = state;
  const toast = (text) => (state.toast = { text, until: performance.now() + TOAST_MS });

  // ---- settings ----

  /** Step a setting, apply any side effects and (unless `quiet`) tell the viewer. */
  function nudge(key, dir = 1, quiet = false) {
    const item = byKey(key);
    const before = state[key];
    state[key] = change(item, before, dir, state);
    applied(key, before);
    if (!quiet) toast(`${item.label} ${format(item, state[key], state)}`);
  }

  /** The side effects of a setting having changed. */
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

  /** Show just the i-th chosen park, or all of them for null. */
  function pickPark(i) {
    const park = i == null ? null : state.feed.parks[i];
    if (i != null && !park) return;
    state.park = park ? park.id : null;
    applied('park');
    toast(park ? park.name.toUpperCase() : 'ALL SELECTED PARKS');
  }

  /** Move to the next (+1) or previous (-1) page. */
  const turn = (delta) => {
    const n = Math.max(1, state.slideCount);
    const index = mod(Math.floor(state.slideTime / state.dwell), n);
    state.slideTime = mod(index + delta, n) * state.dwell + 0.001; // just inside the new page
    state.pageAge = 0;
  };

  // ---- overlays: menu, picker and help ----

  const openMenu = (open = !menu.open) => {
    menu.open = open;
    if (open) state.help = false;
  };

  const toggleHelp = () => {
    openMenu(false);
    state.help = !state.help;
  };

  // The picker edits either the chosen parks or the chosen ride groups.
  const pickerItems = () => (picker.key === 'show' ? state.feed.groups : state.feed.directory);

  const openPicker = (open = !picker.open, key = picker.key) => {
    picker.open = open;
    if (!open) return;
    picker.key = key;
    state.help = false;
    picker.index = Math.max(0, pickerItems().findIndex((p) => state[key].includes(p.id)));
  };

  /** Tick or untick the entry under the cursor; at least one park always stays chosen. */
  const toggleItem = () => {
    const item = pickerItems()[picker.index];
    if (!item) return;
    const key = picker.key;
    const on = state[key].includes(item.id);
    if (key === 'parks' && on && state.parks.length === 1) return;
    const chosen = new Set(state[key]);
    on ? chosen.delete(item.id) : chosen.add(item.id);
    state[key] = pickerItems().map((p) => p.id).filter((id) => chosen.has(id)); // keep the list's order
    applied(key);
  };

  /** Change the selected menu entry, or open the picker if it's a list. */
  const step = (dir) => {
    const item = SCHEMA[menu.index];
    if (item.kind === 'list') openPicker(true, item.key);
    else nudge(item.key, dir, true);
  };

  /** Up and down move the cursor round a list. */
  const listKeys = (list, count) => ({
    ArrowUp: () => (list.index = mod(list.index - 1, Math.max(1, count()))),
    ArrowDown: () => (list.index = mod(list.index + 1, Math.max(1, count()))),
  });

  // ---- key bindings (the menu's and picker's take over while they're open) ----

  const pickerKeys = {
    ...listKeys(picker, () => pickerItems().length),
    ' ': toggleItem,
    Enter: toggleItem,
    p: () => openPicker(false),
    r: () => openPicker(false),
    Escape: () => openPicker(false),
  };

  const menuKeys = {
    ...listKeys(menu, () => SCHEMA.length),
    ArrowLeft: () => step(-1),
    ArrowRight: () => step(1),
    Enter: () => step(1),
    ' ': () => step(1),
    s: () => openMenu(false),
    Escape: () => openMenu(false),
  };

  const boardKeys = {
    s: () => openMenu(),
    p: () => openPicker(!picker.open, 'parks'),
    r: () => openPicker(!picker.open, 'show'),
    h: toggleHelp,
    '?': toggleHelp,
    Escape: () => (state.help = false),
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
  };

  addEventListener('keydown', (e) => {
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    const key = e.key.length === 1 ? e.key.toLowerCase() : e.key;
    const act = picker.open ? pickerKeys[key] : (menu.open && menuKeys[key]) || boardKeys[key];
    if (!act) return;
    e.preventDefault();
    act();
    save(state, demo);
  });

  // ---- touch and mouse ----

  /** The list entry under a tapped dot, or -1 if the tap was off the list. */
  const entryAt = (dot, list) => {
    const row = Math.floor((dot.y - ROWS_Y) / ROW_PITCH);
    return row >= 0 && row < list.visible ? list.offset + row : -1;
  };

  matrix.canvas.addEventListener('click', (e) => {
    // On the board: the left third goes back a page, anywhere else goes forward.
    if (!picker.open && !menu.open) {
      turn(e.clientX < matrix.canvas.clientWidth / 3 ? -1 : 1);
      return;
    }
    // In an overlay: tap an entry to use it (menu: its left half steps down, right half up),
    // or tap off the list to close.
    const dot = matrix.dotAt(e.clientX, e.clientY);
    const list = picker.open ? picker : menu;
    const index = entryAt(dot, list);
    if (index < 0) {
      picker.open ? openPicker(false) : openMenu(false);
      return;
    }
    list.index = index;
    if (picker.open) toggleItem();
    else step(dot.x < matrix.cols / 2 ? -1 : 1);
    save(state, demo);
  });

  document.getElementById('settings-btn')?.addEventListener('click', () => {
    openMenu();
    save(state, demo);
  });

  hideCursorWhenIdle();
  keepScreenOn();
}

/** Hide the mouse cursor when it's idle so a kiosk screen stays clean. */
function hideCursorWhenIdle() {
  let timer;
  const wake = () => {
    document.body.classList.remove('idle');
    clearTimeout(timer);
    timer = setTimeout(() => document.body.classList.add('idle'), CURSOR_IDLE_MS);
  };
  addEventListener('pointermove', wake);
  wake();
}

/** Ask the browser not to let the screen sleep while the board is up. */
function keepScreenOn() {
  const request = async () => {
    try {
      await navigator.wakeLock?.request('screen');
    } catch {}
  };
  request();
  document.addEventListener('visibilitychange', () => document.visibilityState === 'visible' && request());
}
