// Effects: split-flap text, eased values, colour themes and the boot wipe.

import { hsv, luma, mix } from './color.js';
import { hashString } from './util.js';

/** Characters the flap effect cycles through before settling. */
const SCRAMBLE = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789#*+=<>/%&';
const SCRAMBLE_COLOR = [120, 150, 190];
const FLASH_WHITE = [255, 255, 255];

const FLAP_START_MS = 120; // before the first character locks in
const FLAP_STAGGER_MS = 28; // extra delay per character, so they settle left to right
const FLAP_JITTER_MS = 120;
const FLAP_TICK_MS = 70; // how often the scrambled characters change
const FLASH_MS = 450; // white flash as a character locks in
const FORGET_MS = 5000; // text not drawn for this long is forgotten

/** A cheap deterministic hash of three integers. */
function hash3(a, b, c) {
  let h = (a * 374761393 + b * 668265263 + c * 2147483647) | 0;
  h = Math.imul(h ^ (h >>> 13), 1274126177);
  return (h ^ (h >>> 16)) >>> 0;
}

/**
 * Text that resolves like a split-flap board: when the string behind a key
 * changes, the characters that differ cycle through random glyphs and then
 * lock in left to right with a brief white flash.
 */
export class Flap {
  constructor() {
    this.cells = new Map(); // key -> { target text, per-character lock times, last drawn }
    this.sweptAt = 0;
    this.enabled = true; // false draws text plainly, with no scramble
  }

  /** Draw `text` for `key` (see Matrix.text for the other arguments); returns its width. */
  draw(m, key, text, x, y, color, now, opts = {}) {
    if (!this.enabled) return m.text(text, x, y, color, opts);
    const cell = this.#cellFor(key, text, now);

    const keyHash = hashString(key);
    const tick = Math.floor(now / FLAP_TICK_MS);
    const shown = [...text].map((ch, i) =>
      ch === ' ' || now >= cell.locks[i] ? ch : SCRAMBLE[hash3(i, tick, keyHash) % SCRAMBLE.length],
    );
    const colorAt = (i) => {
      const c = typeof color === 'function' ? color(i) : color;
      const t = now - cell.locks[i];
      if (t < 0) return SCRAMBLE_COLOR;
      return t < FLASH_MS ? mix(c, FLASH_WHITE, 0.85 * (1 - t / FLASH_MS)) : c;
    };
    const width = m.text(shown.join(''), x, y, colorAt, opts);

    this.#forgetOld(now);
    return width;
  }

  /** The cell for `key`, restarting the effect for any characters that changed. */
  #cellFor(key, text, now) {
    let cell = this.cells.get(key);
    if (!cell || cell.target !== text) {
      const previous = cell?.target ?? '';
      const locks = [...text].map((ch, i) =>
        previous[i] === ch
          ? (cell?.locks[i] ?? now - 1000) // unchanged: stays locked
          : now + FLAP_START_MS + i * FLAP_STAGGER_MS + (hash3(i, now | 0, 7) % FLAP_JITTER_MS),
      );
      cell = { target: text, locks, seen: now };
      this.cells.set(key, cell);
    }
    cell.seen = now;
    return cell;
  }

  #forgetOld(now) {
    if (now - this.sweptAt < FORGET_MS) return;
    this.sweptAt = now;
    for (const [key, cell] of this.cells) if (now - cell.seen > FORGET_MS) this.cells.delete(key);
  }
}

/** Values that glide toward a target instead of jumping, keyed by name. */
export class Ease {
  constructor() {
    this.values = new Map();
  }

  /** The value for `key` moving toward `target`; the first call starts at the target. */
  get(key, target, now, rate = 6) {
    const v = this.values.get(key);
    if (!v) {
      this.values.set(key, { value: target, at: now });
      return target;
    }
    const dt = Math.min(0.25, (now - v.at) / 1000);
    v.value += (target - v.value) * (1 - Math.exp(-rate * dt));
    v.at = now;
    return v.value;
  }
}

// One of each, shared by everything that draws.
export const flap = new Flap();
export const ease = new Ease();

// ---- themes -----------------------------------------------------------------

export const THEMES = ['rgb', 'amber', 'green', 'ice', 'rainbow'];

/** The single LED colour each monochrome theme maps brightness onto. */
const TINTS = {
  amber: [255, 176, 0],
  green: [70, 255, 110],
  ice: [90, 210, 255],
};

const SWEEP_PERIOD_MS = 22000;
const SWEEP_MS = 1600; // how long each sweep takes to cross the board
const SWEEP_WIDTH = 14; // dots

/**
 * Post-process the finished frame: monochrome themes remap brightness onto one
 * LED colour, rainbow repaints everything with a travelling hue, and every
 * so often a soft highlight sweeps across the whole board.
 */
export function shade(m, theme, now, shine = true) {
  const { buf, cols, rows } = m;
  const tint = TINTS[theme];
  const sweepT = (now % SWEEP_PERIOD_MS) / SWEEP_MS; // below 1 while a sweep is under way
  const sweepPos = sweepT * (cols + rows) * 1.4 - rows;
  const sweeping = shine && sweepT < 1;

  for (let y = 0; y < rows; y++) {
    for (let x = 0; x < cols; x++) {
      const i = (y * cols + x) * 4;
      let r = buf[i], g = buf[i + 1], b = buf[i + 2];
      if (r + g + b < 4) continue; // unlit
      if (tint) {
        const l = luma([r, g, b]);
        r = tint[0] * l;
        g = tint[1] * l;
        b = tint[2] * l;
      } else if (theme === 'rainbow') {
        [r, g, b] = hsv(x * 2.2 + now / 14, 0.9, luma([r, g, b]) ** 0.85);
      }
      if (sweeping) {
        const d = Math.abs(x + y * 0.6 - sweepPos);
        if (d < SWEEP_WIDTH) {
          const k = (1 - d / SWEEP_WIDTH) ** 2 * 0.55;
          r += (255 - r) * k;
          g += (255 - g) * k;
          b += (255 - b) * k;
        }
      }
      buf[i] = r;
      buf[i + 1] = g;
      buf[i + 2] = b;
    }
  }
}

// ---- boot -------------------------------------------------------------------

const BOOT_MS = 1800;

/** A bright rainbow bar wipes across the board, hiding whatever is right of it. */
export function bootWipe(m, now) {
  const t = now / BOOT_MS;
  if (t >= 1) return;
  const head = Math.floor(t * t * (3 - 2 * t) * (m.cols + 8)) - 4; // smoothstep across the board
  const { buf, cols, rows } = m;
  for (let y = 0; y < rows; y++) {
    for (let x = Math.max(0, head + 1); x < cols; x++) {
      const i = (y * cols + x) * 4;
      buf[i] = buf[i + 1] = buf[i + 2] = 0;
    }
    for (let k = 0; k < 4; k++) m.px(head - k, y, hsv(y * 5 + now / 6, 0.85, 1), 1 - k * 0.28);
  }
}
