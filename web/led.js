import { ADVANCE, glyph, textWidth } from './font.js';

// ---- colour helpers: colours are [r, g, b] with channels 0-255 -----------

export function hsv(h, s = 1, v = 1) {
  h = ((h % 360) + 360) % 360;
  const c = v * s;
  const x = c * (1 - Math.abs(((h / 60) % 2) - 1));
  const m = v - c;
  const [r, g, b] =
    h < 60 ? [c, x, 0] : h < 120 ? [x, c, 0] : h < 180 ? [0, c, x] : h < 240 ? [0, x, c] : h < 300 ? [x, 0, c] : [c, 0, x];
  return [(r + m) * 255, (g + m) * 255, (b + m) * 255];
}

export function mix(a, b, t) {
  return [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t];
}

export function scale(c, k) {
  return [c[0] * k, c[1] * k, c[2] * k];
}

export function luma([r, g, b]) {
  return (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255;
}

// ---- the matrix ----------------------------------------------------------

const BG = '#040508';
const OFF_DOT = 'rgba(150, 165, 200, 0.075)';

/**
 * A grid of RGB LEDs drawn on a canvas. Callers paint into `buf` (RGBA, one
 * entry per dot) with the drawing helpers, then call `present()`.
 *
 * The grid is `cols` dots wide whatever the screen size; the row count follows
 * from the aspect ratio, so layouts stay proportional on any display.
 */
export class Matrix {
  constructor(canvas, cols = 128) {
    this.canvas = canvas;
    this.ctx = canvas.getContext('2d', { alpha: false });
    this.targetCols = cols;
    this.resize();
  }

  setCols(cols) {
    this.targetCols = cols;
    this.resize();
  }

  resize() {
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    const cssW = this.canvas.clientWidth || window.innerWidth;
    const cssH = this.canvas.clientHeight || window.innerHeight;
    this.canvas.width = Math.round(cssW * dpr);
    this.canvas.height = Math.round(cssH * dpr);

    // Never let dots get smaller than ~3 css px; narrow screens just get fewer columns.
    this.cols = Math.max(48, Math.min(this.targetCols, Math.floor(cssW / 3)));
    this.pitch = this.canvas.width / this.cols;
    this.rows = Math.max(16, Math.floor(this.canvas.height / this.pitch));
    this.offY = (this.canvas.height - this.rows * this.pitch) / 2;

    this.buf = new Uint8ClampedArray(this.cols * this.rows * 4);
    this.img = new ImageData(this.cols, this.rows);
    this.bloom = [1, 2, 4].map((div) => {
      const c = document.createElement('canvas');
      c.width = Math.max(1, Math.ceil(this.cols / div));
      c.height = Math.max(1, Math.ceil(this.rows / div));
      return c;
    });
    this.buildOffLayer();
    this.clear();
  }

  buildOffLayer() {
    const c = (this.off = document.createElement('canvas'));
    c.width = this.canvas.width;
    c.height = this.canvas.height;
    const g = c.getContext('2d');
    g.fillStyle = BG;
    g.fillRect(0, 0, c.width, c.height);
    g.fillStyle = OFF_DOT;
    for (let y = 0; y < this.rows; y++) for (let x = 0; x < this.cols; x++) this.dot(g, x, y);
  }

  dot(g, x, y) {
    const p = this.pitch;
    const cx = (x + 0.5) * p;
    const cy = this.offY + (y + 0.5) * p;
    const r = p * 0.4;
    if (p < 4) {
      g.fillRect(cx - r, cy - r, r * 2, r * 2);
    } else {
      g.beginPath();
      g.arc(cx, cy, r, 0, Math.PI * 2);
      g.fill();
    }
  }

  // ---- drawing -----------------------------------------------------------

  clear() {
    this.buf.fill(0);
  }

  px(x, y, c, a = 1) {
    x |= 0;
    y |= 0;
    if (x < 0 || y < 0 || x >= this.cols || y >= this.rows) return;
    const i = (y * this.cols + x) * 4;
    const b = this.buf;
    if (a >= 1) {
      b[i] = c[0];
      b[i + 1] = c[1];
      b[i + 2] = c[2];
    } else {
      b[i] += (c[0] - b[i]) * a;
      b[i + 1] += (c[1] - b[i + 1]) * a;
      b[i + 2] += (c[2] - b[i + 2]) * a;
    }
  }

  rect(x, y, w, h, c, a = 1) {
    for (let j = 0; j < h; j++) for (let i = 0; i < w; i++) this.px(x + i, y + j, c, a);
  }

  /**
   * Draw a string with its top-left dot at (x, y). `clip` is an [x0, x1)
   * range that dots must stay inside. `color` may be a colour or a function
   * (charIndex) => colour. Returns the width drawn.
   */
  text(str, x, y, color, { scale: s = 1, clip = null, gap = 0 } = {}) {
    const [x0, x1] = clip ?? [0, this.cols];
    let i = 0;
    for (const ch of str) {
      const cx = x + i * (ADVANCE * s + gap);
      if (cx + 5 * s > x0 && cx < x1) {
        const c = typeof color === 'function' ? color(i) : color;
        for (const [gx, gy] of glyph(ch)) {
          for (let dy = 0; dy < s; dy++) {
            for (let dx = 0; dx < s; dx++) {
              const px = cx + gx * s + dx;
              if (px >= x0 && px < x1) this.px(px, y + gy * s + dy, c);
            }
          }
        }
      }
      i++;
    }
    return textWidth(str, s);
  }

  // ---- output ------------------------------------------------------------

  /** Paint `buf` to the canvas. `brightness` is 0-1; `glow` scales the bloom. */
  present(brightness = 1, glow = 1) {
    const { ctx, canvas, buf, img, cols, rows } = this;
    const data = img.data;
    const k = brightness;

    ctx.globalCompositeOperation = 'source-over';
    ctx.drawImage(this.off, 0, 0);

    const p = this.pitch;
    const small = p < 4;
    const r = p * 0.4;
    for (let y = 0; y < rows; y++) {
      for (let x = 0; x < cols; x++) {
        const i = (y * cols + x) * 4;
        const R = buf[i] * k, G = buf[i + 1] * k, B = buf[i + 2] * k;
        data[i] = R;
        data[i + 1] = G;
        data[i + 2] = B;
        data[i + 3] = 255;
        if (R < 2 && G < 2 && B < 2) continue;
        ctx.fillStyle = `rgb(${R | 0},${G | 0},${B | 0})`;
        const cx = (x + 0.5) * p;
        const cy = this.offY + (y + 0.5) * p;
        if (small) {
          ctx.fillRect(cx - r, cy - r, r * 2, r * 2);
        } else {
          ctx.beginPath();
          ctx.arc(cx, cy, r, 0, Math.PI * 2);
          ctx.fill();
        }
      }
    }

    // Bloom: the dot image upscaled with smoothing gives a soft halo around each
    // LED; two successive halvings give a wider, fainter one.
    this.bloom[0].getContext('2d').putImageData(img, 0, 0);
    for (let n = 1; n < this.bloom.length; n++) {
      const g = this.bloom[n].getContext('2d');
      g.imageSmoothingEnabled = true;
      g.drawImage(this.bloom[n - 1], 0, 0, this.bloom[n].width, this.bloom[n].height);
    }
    ctx.globalCompositeOperation = 'lighter';
    ctx.imageSmoothingEnabled = true;
    const alphas = [0.5, 0.45, 0.4];
    this.bloom.forEach((c, n) => {
      ctx.globalAlpha = alphas[n] * glow;
      ctx.drawImage(c, 0, this.offY, canvas.width, this.rows * p);
    });
    ctx.globalAlpha = 1;
    ctx.globalCompositeOperation = 'source-over';
  }
}
