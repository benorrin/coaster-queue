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

const VERT = `
attribute vec2 p;
varying vec2 uv;
void main() {
  uv = vec2(p.x * 0.5 + 0.5, 0.5 - p.y * 0.5);
  gl_Position = vec4(p, 0.0, 1.0);
}`;

// One quad covers the board; each fragment finds its dot, looks the colour up
// in the buffer texture and draws a round LED over a faint unlit one.
const FRAG = `
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif
uniform sampler2D tex;
uniform vec2 grid;
uniform float bright;
uniform float edge;
varying vec2 uv;
void main() {
  vec2 g = uv * grid;
  vec2 cell = floor(g);
  float mask = 1.0 - smoothstep(0.4 - edge, 0.4 + edge, length(fract(g) - 0.5));
  vec3 lit = texture2D(tex, (cell + 0.5) / grid).rgb * bright;
  vec3 col = vec3(0.016, 0.02, 0.031) + vec3(0.59, 0.65, 0.78) * 0.075 * mask + lit * mask;
  gl_FragColor = vec4(col, 1.0);
}`;

function compile(gl, type, src) {
  const sh = gl.createShader(type);
  gl.shaderSource(sh, src);
  gl.compileShader(sh);
  if (!gl.getShaderParameter(sh, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(sh));
  return sh;
}

/**
 * A grid of RGB LEDs drawn on a canvas. Callers paint into `buf` (RGBA, one
 * entry per dot) with the drawing helpers, then call `present()`, which
 * uploads the buffer as a texture and lets the GPU draw every dot at once.
 *
 * The grid is `cols` dots wide whatever the screen size; the row count follows
 * from the aspect ratio, so layouts stay proportional on any display.
 */
export class Matrix {
  constructor(canvas, cols = 128) {
    this.canvas = canvas;
    this.targetCols = cols;
    this.gl = canvas.getContext('webgl', { alpha: false, antialias: false, powerPreference: 'low-power' });
    if (!this.gl) throw new Error('WebGL is not available');
    this.initGl();
    canvas.addEventListener('webglcontextlost', (e) => e.preventDefault());
    canvas.addEventListener('webglcontextrestored', () => {
      this.initGl();
      this.resize();
    });
    this.resize();
  }

  initGl() {
    const gl = this.gl;
    const prog = gl.createProgram();
    gl.attachShader(prog, compile(gl, gl.VERTEX_SHADER, VERT));
    gl.attachShader(prog, compile(gl, gl.FRAGMENT_SHADER, FRAG));
    gl.linkProgram(prog);
    if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(prog));
    gl.useProgram(prog);

    gl.bindBuffer(gl.ARRAY_BUFFER, gl.createBuffer());
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 1, -1, -1, 1, 1, 1]), gl.STATIC_DRAW);
    const loc = gl.getAttribLocation(prog, 'p');
    gl.enableVertexAttribArray(loc);
    gl.vertexAttribPointer(loc, 2, gl.FLOAT, false, 0, 0);

    this.u = {
      grid: gl.getUniformLocation(prog, 'grid'),
      bright: gl.getUniformLocation(prog, 'bright'),
      edge: gl.getUniformLocation(prog, 'edge'),
    };
    this.tex = gl.createTexture();
    gl.bindTexture(gl.TEXTURE_2D, this.tex);
    for (const [k, v] of [
      [gl.TEXTURE_MIN_FILTER, gl.NEAREST],
      [gl.TEXTURE_MAG_FILTER, gl.NEAREST],
      [gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE],
      [gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE],
    ]) gl.texParameteri(gl.TEXTURE_2D, k, v);
    gl.clearColor(0.016, 0.02, 0.031, 1);
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

    this.buf = new Uint8ClampedArray(this.cols * this.rows * 4);
    this.bytes = new Uint8Array(this.buf.buffer);
    this.clear();
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

  /** Paint `buf` to the canvas. `brightness` is 0-1. */
  present(brightness = 1) {
    const { gl, u, canvas, cols, rows, pitch } = this;
    if (gl.isContextLost()) return;
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, cols, rows, 0, gl.RGBA, gl.UNSIGNED_BYTE, this.bytes);
    gl.uniform2f(u.grid, cols, rows);
    gl.uniform1f(u.bright, brightness);
    gl.uniform1f(u.edge, Math.min(0.25, 0.7 / pitch));

    // Rows are a whole number of dots, so centre the grid in any leftover height.
    const used = Math.round(rows * pitch);
    gl.viewport(0, 0, canvas.width, canvas.height);
    gl.clear(gl.COLOR_BUFFER_BIT);
    gl.viewport(0, Math.floor((canvas.height - used) / 2), canvas.width, used);
    gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
  }
}
