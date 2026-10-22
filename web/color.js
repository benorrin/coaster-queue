// Colours are [r, g, b] arrays with channels 0-255.

import { hashString } from './util.js';

export function hsv(h, s = 1, v = 1) {
  h = ((h % 360) + 360) % 360;
  const c = v * s;
  const x = c * (1 - Math.abs(((h / 60) % 2) - 1));
  const m = v - c;
  const [r, g, b] =
    h < 60 ? [c, x, 0] : h < 120 ? [x, c, 0] : h < 180 ? [0, c, x] : h < 240 ? [0, x, c] : h < 300 ? [x, 0, c] : [c, 0, x];
  return [(r + m) * 255, (g + m) * 255, (b + m) * 255];
}

/** Blend from colour `a` to `b`; t is 0 to 1. */
export function mix(a, b, t) {
  return [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t];
}

/** Dim (or brighten) a colour by a factor. */
export function scale(c, k) {
  return [c[0] * k, c[1] * k, c[2] * k];
}

/** Perceived brightness, 0 to 1. */
export function luma([r, g, b]) {
  return (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255;
}

// The palette the display draws with.
export const WHITE = [235, 240, 255];
export const GREY = [150, 160, 185];
export const MUTED = [130, 138, 160];
export const DIM = [90, 98, 120];
export const RED = [255, 40, 40];
export const GREEN = [40, 255, 90];
export const CYAN = [40, 200, 255];
export const AMBER = [255, 170, 30];

/** Each park (or group) gets its own stable colour, picked from a hash of its id. */
export function accentFor(id) {
  return hsv(hashString(id) % 360, 0.85, 1);
}
