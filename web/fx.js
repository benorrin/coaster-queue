import { SCRAMBLE } from './font.js';
import { hsv, luma, mix } from './led.js';

const WHITE = [255, 255, 255];
const SCRAMBLE_COLOR = [120, 150, 190];

function hash(a, b, c) {
  let h = (a * 374761393 + b * 668265263 + c * 2147483647) | 0;
  h = Math.imul(h ^ (h >>> 13), 1274126177);
  return (h ^ (h >>> 16)) >>> 0;
}

function keyHash(key) {
  let h = 5381;
  for (let i = 0; i < key.length; i++) h = (Math.imul(h, 33) + key.charCodeAt(i)) | 0;
  return h >>> 0;
}

/**
 * Text that resolves like a split-flap board: when the string behind a key
 * changes, the characters that differ cycle through random glyphs and then
 * lock in left to right with a brief white flash.
 */
export class Flap {
  constructor() {
    this.cells = new Map();
    this.sweptAt = 0;
    this.enabled = true; // false draws text plainly, with no scramble
  }

  draw(m, key, text, x, y, color, now, opts = {}) {
    if (!this.enabled) return m.text(text, x, y, color, opts);
    let cell = this.cells.get(key);
    if (!cell || cell.target !== text) {
      const prev = cell?.target ?? '';
      const locks = [...text].map((ch, i) =>
        prev[i] === ch ? (cell?.locks[i] ?? now - 1000) : now + 120 + i * 28 + (hash(i, now | 0, 7) % 120),
      );
      cell = { target: text, locks, seen: now };
      this.cells.set(key, cell);
    }
    cell.seen = now;

    const kh = keyHash(key);
    const tick = Math.floor(now / 70);
    const chars = [...text];
    const shown = chars.map((ch, i) =>
      ch === ' ' || now >= cell.locks[i] ? ch : SCRAMBLE[hash(i, tick, kh) % SCRAMBLE.length],
    );
    const colorAt = (i) => {
      const c = typeof color === 'function' ? color(i) : color;
      const t = now - cell.locks[i];
      if (t < 0) return SCRAMBLE_COLOR;
      return t < 450 ? mix(c, WHITE, 0.85 * (1 - t / 450)) : c;
    };
    const w = m.text(shown.join(''), x, y, colorAt, opts);

    if (now - this.sweptAt > 5000) {
      this.sweptAt = now;
      for (const [k, v] of this.cells) if (now - v.seen > 5000) this.cells.delete(k);
    }
    return w;
  }
}

/** Values that glide toward a target instead of jumping, keyed by name. */
export class Ease {
  constructor() {
    this.values = new Map();
  }

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

// ---- themes ------------------------------------------------------------

export const THEMES = ['rgb', 'amber', 'green', 'ice', 'rainbow'];

const TINTS = {
  amber: [255, 176, 0],
  green: [70, 255, 110],
  ice: [90, 210, 255],
};

/**
 * Post-process the finished frame: monochrome themes remap brightness onto one
 * LED colour, rainbow repaints everything with a travelling hue, and every
 * so often a soft highlight sweeps across the whole board.
 */
export function shade(m, theme, now, shine = true) {
  const { buf, cols, rows } = m;
  const tint = TINTS[theme];
  const sweepPeriod = 22000;
  const sweepT = (now % sweepPeriod) / 1600; // sweep lasts 1.6s of each period
  const sweepPos = sweepT * (cols + rows) * 1.4 - rows;
  const sweeping = shine && sweepT < 1;

  for (let y = 0; y < rows; y++) {
    for (let x = 0; x < cols; x++) {
      const i = (y * cols + x) * 4;
      let r = buf[i], g = buf[i + 1], b = buf[i + 2];
      if (r + g + b < 4) continue;
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
        if (d < 14) {
          const k = (1 - d / 14) ** 2 * 0.55;
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

// ---- boot --------------------------------------------------------------

export const BOOT_MS = 1800;

/** A bright rainbow bar wipes across the board, hiding whatever is right of it. */
export function bootWipe(m, now) {
  const t = now / BOOT_MS;
  if (t >= 1) return;
  const head = Math.floor((t * t * (3 - 2 * t)) * (m.cols + 8)) - 4;
  const { buf, cols, rows } = m;
  for (let y = 0; y < rows; y++) {
    for (let x = Math.max(0, head + 1); x < cols; x++) {
      const i = (y * cols + x) * 4;
      buf[i] = buf[i + 1] = buf[i + 2] = 0;
    }
    for (let k = 0; k < 4; k++) {
      m.px(head - k, y, hsv(y * 5 + now / 6, 0.85, 1), 1 - k * 0.28);
    }
  }
}
