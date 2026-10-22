// Where things go on the matrix, in dots, and the motion shared by scrolling text.

export const PAD = 3;
export const HEADER_Y = 2;
export const RULE_Y = 17;
/** The first row of rides (and of menu and picker entries), and the spacing between rows. */
export const ROWS_Y = 20;
export const ROW_PITCH = 9;
const FOOTER_H = 11;

const SCROLL_SPEED = 30; // dots per second for text that doesn't fit
const SCROLL_HOLD = 1.4; // seconds parked at each end

/** How many rows of rides, menu entries or picker entries fit above the footer. */
export function visibleRows(m) {
  return Math.max(1, Math.floor((m.rows - FOOTER_H - ROWS_Y + 2) / ROW_PITCH));
}

/** The row the footer text sits on. */
export const footerY = (m) => m.rows - 8;

/**
 * Horizontal offset for text `overflow` dots wider than its space, `t` seconds
 * after it started: parked, scrolling to the end, parked, then back to the start.
 */
export function scrollOffset(overflow, t) {
  if (overflow <= 0) return 0;
  const run = overflow / SCROLL_SPEED;
  const u = t % (run + 2 * SCROLL_HOLD);
  return -Math.min(overflow, Math.max(0, (u - SCROLL_HOLD) * SCROLL_SPEED));
}
