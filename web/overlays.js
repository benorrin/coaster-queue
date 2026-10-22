// Screens drawn in place of the board: the settings menu, the park and group picker,
// the help page, plus the pop-up that confirms a change.

import { AMBER, CYAN, DIM, GREEN, GREY, MUTED, WHITE, accentFor, hsv, scale } from './color.js';
import { ADVANCE, normalise, textWidth } from './font.js';
import { HEADER_Y, PAD, ROWS_Y, ROW_PITCH, footerY, scrollOffset, visibleRows } from './layout.js';
import { SCHEMA, format } from './settings.js';
import { clamp } from './util.js';

const NARROW_COLS = 160; // below this, hints talk about tapping rather than keys

// ---- shared pieces ----------------------------------------------------------

/** A large heading in a slowly shifting rainbow. */
function drawTitle(m, text, now) {
  m.text(text, PAD, HEADER_Y, (i) => hsv(now / 12 + i * 20, 0.8, 1), { scale: 2 });
}

/** A line of hint text on the footer row. */
function drawHint(m, text) {
  m.text(text, PAD, footerY(m), DIM, { clip: [0, m.cols - PAD] });
}

/**
 * Draw the entries of a scrolling list, keeping the one at `list.index` in view. `list`
 * is the overlay's state ({ index, offset, visible }), which this updates so that taps
 * can be matched to rows. `draw(i, y, selected)` paints entry `i` on row `y`.
 */
function drawList(m, list, count, draw) {
  const visible = visibleRows(m);
  list.visible = visible;
  list.offset = clamp(list.index - Math.floor(visible / 2), 0, Math.max(0, count - visible));
  for (let row = 0; row < Math.min(visible, count); row++) {
    const i = list.offset + row;
    draw(i, ROWS_Y + row * ROW_PITCH, i === list.index);
  }
  // Arrows show when there's more above or below.
  const x = m.cols - PAD - 5;
  if (list.offset > 0) m.text('▲', x, ROWS_Y, DIM);
  if (list.offset + visible < count) m.text('▼', x, ROWS_Y + (visible - 1) * ROW_PITCH, DIM);
}

// ---- settings menu ----------------------------------------------------------

function drawMenu(m, state) {
  const { menu, now } = state;
  drawTitle(m, 'SETTINGS', now);

  const valueX = PAD + 6 + 14 * ADVANCE;
  drawList(m, menu, SCHEMA.length, (i, y, selected) => {
    const item = SCHEMA[i];
    const value = state[item.key];
    const text = format(item, value, state);
    // Grey for off rather than red: red nearly vanishes in the monochrome themes.
    const colour = item.kind !== 'bool' ? CYAN : value ? GREEN : GREY;

    if (selected) m.text('>', PAD, y, AMBER);
    m.text(item.label, PAD + 6, y, selected ? WHITE : GREY);
    if (selected) m.text('<', valueX - 7, y, DIM);
    m.text(text, valueX, y, selected ? colour : scale(colour, 0.7), { clip: [0, m.cols - PAD - 8] });
    if (selected && valueX + textWidth(text) + 7 < m.cols) m.text('>', valueX + textWidth(text) + 1, y, DIM);
  });
  drawHint(m, m.cols < NARROW_COLS ? 'TAP TO CHANGE' : 'ARROWS ADJUST - S CLOSES');
}

// ---- park and group picker --------------------------------------------------

/** A 5x5 tick box. */
function drawCheck(m, x, y, on, colour) {
  m.outline(x, y, 5, 5, colour);
  if (on) m.rect(x + 1, y + 1, 3, 3, colour);
}

/** Checklist for choosing parks or ride groups, whichever the picker is set to. */
function drawPicker(m, state) {
  const { picker, now, feed } = state;
  const groups = picker.key === 'show';
  drawTitle(m, groups ? 'GROUPS' : 'PARKS', now);
  const items = groups ? feed.groups : feed.directory;
  if (!items.length) {
    m.text('LOADING', PAD, ROWS_Y + 2, DIM);
    return;
  }
  const chosen = state[picker.key];

  drawList(m, picker, items.length, (i, y, selected) => {
    const item = items[i];
    const on = chosen.includes(item.id);
    const accent = accentFor(item.id);
    if (selected) m.text('>', PAD, y, AMBER);
    drawCheck(m, PAD + 8, y + 1, on, on ? accent : GREY);

    // A name too long for the row is cut off, except under the cursor where it scrolls.
    const name = normalise(item.label ?? item.name);
    const x = PAD + 18;
    const room = m.cols - PAD - 8 - x;
    const offset = selected ? scrollOffset(textWidth(name) - room, now / 1000) : 0;
    m.text(name, x + offset, y, on ? accent : selected ? WHITE : MUTED, { clip: [x, x + room] });
  });

  const count = groups && !chosen.length ? 'ALL RIDES' : `${chosen.length} CHOSEN`;
  const hint = m.cols < NARROW_COLS ? 'TAP TO TICK' : groups ? 'NONE SHOWS ALL' : 'SPACE TICKS - P DONE';
  drawHint(m, `${count} - ${hint}`);
}

// ---- help -------------------------------------------------------------------

const HELP = [
  ['SPACE', 'PAUSE'],
  ['LEFT RIGHT', 'PAGE'],
  ['UP DOWN', 'BRIGHTNESS'],
  ['T', 'THEME'],
  ['S P R', 'SETTINGS PARKS GROUPS'],
  ['G C D W', 'QUICK TOGGLES'], // the settings menu names them
  ['1-9 0', 'SHOW ONE PARK OR ALL'],
  ['F', 'FULLSCREEN'],
];

function drawHelp(m, now) {
  drawTitle(m, 'CONTROLS', now);
  const keyW = 11 * ADVANCE;
  const colW = keyW + 22 * ADVANCE;
  const perCol = visibleRows(m);
  HELP.forEach(([key, what], i) => {
    const x = PAD + Math.floor(i / perCol) * colW;
    const y = ROWS_Y + (i % perCol) * ROW_PITCH;
    m.text(key, x, y, CYAN, { clip: [0, m.cols - PAD] });
    m.text(what, x + keyW, y, WHITE, { clip: [0, m.cols - PAD] });
  });
  drawHint(m, 'H CLOSES - TAP TO PAGE');
}

// ---- entry points -----------------------------------------------------------

/** Draw whichever full-screen overlay is open and return true, or return false if none is. */
export function drawOverlay(m, state) {
  if (state.picker.open) drawPicker(m, state);
  else if (state.menu.open) drawMenu(m, state);
  else if (state.help) drawHelp(m, state.now);
  else return false;
  return true;
}

/** A small boxed message in the middle of the board, e.g. "THEME AMBER". */
export function drawToast(m, text) {
  const line = normalise(text);
  const w = textWidth(line) + 10;
  const h = ROW_PITCH + 3;
  const x = Math.floor((m.cols - w) / 2);
  const y = Math.floor(m.rows / 2) - 6;
  m.rect(x, y, w, h, [0, 0, 0]);
  m.outline(x, y, w, h, CYAN);
  m.text(line, x + 5, y + 3, WHITE);
}
