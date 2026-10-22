// The departure board: pages of rides for each park, a header, and a footer with the
// clock, weather and page counter.

import { AMBER, DIM, GREEN, GREY, RED, WHITE, accentFor, hsv, mix, scale } from './color.js';
import { ADVANCE, normalise, textWidth } from './font.js';
import { ease, flap } from './fx.js';
import { HEADER_Y, PAD, ROWS_Y, ROW_PITCH, RULE_Y, footerY, scrollOffset, visibleRows } from './layout.js';
import { drawOverlay, drawToast } from './overlays.js';
import { drawWeather } from './weather.js';

const MAX_WAIT = 60; // minutes at which the bar is full and the colour is red
const LONG_QUEUE = 50; // minutes at which a wait starts to pulse
const WAIT_TEXT_W = 4 * ADVANCE - 1; // fits "WALK"
const ARROW_W = 5;
const BIG_TEXT_COLS = 160; // header text is double size at least this wide
const NO_BAR_COLS = 150; // narrower than this the wait bar is dropped to leave room for names

/** Green at no wait, through yellow, to red at an hour or more. */
function waitColor(mins) {
  return hsv(130 * (1 - Math.min(mins, MAX_WAIT) / MAX_WAIT), 1, 1);
}

// ---- pages ------------------------------------------------------------------

/**
 * One slide per page of rides, for every park in turn (or just the park `only`).
 * Open rides come first, longest wait first, then closed ones if they're wanted.
 */
function buildSlides(parks, pageSize, showClosed, only) {
  const slides = [];
  parks.forEach((park, parkIndex) => {
    if (only && park.id !== only) return;
    const open = park.rides.filter((r) => r.is_open);
    const rides = showClosed ? [...open, ...park.rides.filter((r) => !r.is_open)] : open;
    const pages = Math.max(1, Math.ceil(rides.length / pageSize));
    for (let page = 0; page < pages; page++) {
      slides.push({ parkIndex, rides: rides.slice(page * pageSize, (page + 1) * pageSize) });
    }
  });
  return slides;
}

// ---- rides ------------------------------------------------------------------

/** A bar of dots that fills (smoothly) in proportion to the wait. */
function drawBar(m, key, x, y, w, mins, now, gradient) {
  const fill = ease.get(key, (Math.min(mins, MAX_WAIT) / MAX_WAIT) * w, now);
  for (let i = 0; i < w; i++) {
    // With gradients off the whole bar is the colour of the current wait.
    const c = waitColor(gradient ? (i / (w - 1)) * MAX_WAIT : mins);
    const on = Math.max(0, Math.min(1, fill - i));
    m.rect(x + i, y, 1, 3, mix(scale(c, 0.1), c, on));
  }
}

/** Where each part of a ride row goes, for a panel `w` dots wide starting at `x0`. */
function rideColumns(x0, w) {
  const right = x0 + w - PAD;
  const barW = w < NO_BAR_COLS ? 0 : Math.max(10, Math.min(28, Math.round(w * 0.14)));
  const arrowX = right - WAIT_TEXT_W - ARROW_W - 2;
  const barX = arrowX - (barW ? 4 + barW : 1);
  const nameX = x0 + PAD;
  return { right, barW, arrowX, barX, nameX, nameW: barX - nameX - 3 };
}

function drawRide(m, ride, key, x0, w, y, state) {
  const { now, pageAge } = state;
  const col = rideColumns(x0, w);
  const name = normalise(ride.name);
  const nameX = col.nameX + scrollOffset(textWidth(name) - col.nameW, pageAge);
  const nameOpts = { clip: [col.nameX, col.nameX + col.nameW] };

  if (!ride.is_open) {
    flap.draw(m, `${key}.n`, name, nameX, y, DIM, now, nameOpts);
    m.text('CLOSED', col.right - textWidth('CLOSED'), y, scale(RED, 0.55));
    return;
  }

  // Long queues breathe so they catch the eye from across the concourse.
  const pulse = ride.wait_time >= LONG_QUEUE ? 0.85 + 0.15 * Math.sin(now / 170) : 1;
  const waitLabel = ride.wait_time === 0 ? 'WALK' : `${ride.wait_time}M`;
  flap.draw(m, `${key}.n`, name, nameX, y, WHITE, now, nameOpts);
  if (col.barW) drawBar(m, `${key}.b`, col.barX, y + 2, col.barW, ride.wait_time, now, state.gradients);
  if (ride.trend) m.text(ride.trend > 0 ? '▲' : '▼', col.arrowX, y, ride.trend > 0 ? RED : GREEN);
  flap.draw(m, `${key}.w`, waitLabel, col.right - textWidth(waitLabel), y, scale(waitColor(ride.wait_time), pulse), now);
}

// ---- header -----------------------------------------------------------------

/** The right-hand block: open count and average wait, or "CLOSED". Empty on narrow screens. */
function headerStats(park, big) {
  const open = park.rides.filter((r) => r.is_open);
  if (big && open.length) {
    const avg = Math.round(open.reduce((sum, r) => sum + r.wait_time, 0) / open.length);
    return { lines: [`${open.length} OPEN`, `AVG ${avg}M`] };
  }
  if (!open.length && park.fetched_at) return { lines: ['CLOSED'], closed: true };
  return { lines: [] };
}

function drawHeader(m, park, accent, x0, w, progress, state) {
  const name = normalise(park.name);
  // Every park gets the same type size (set by the screen width, not the name); a name
  // too long for its space scrolls instead of shrinking.
  const big = w >= BIG_TEXT_COLS;
  const textScale = big ? 2 : 1;
  const stats = headerStats(park, big);
  const statsW = stats.lines.length ? textWidth(stats.lines[0]) + PAD : 0;
  const room = w - 2 * PAD - statsW - (statsW ? PAD : 0);
  const left = x0 + PAD;
  const offset = scrollOffset(textWidth(name, textScale) - room, state.pageAge);
  m.text(name, left + offset, big ? HEADER_Y : HEADER_Y + 3, accent, { scale: textScale, clip: [left, left + room] });

  stats.lines.forEach((line, i) => {
    const colour = stats.closed ? scale(RED, 0.8) : i ? DIM : WHITE;
    m.text(line, x0 + w - PAD - textWidth(line), HEADER_Y + (stats.closed ? 3 : i * 8), colour);
  });

  // The rule under the header doubles as the page-dwell progress bar.
  const ruleW = w - 2 * PAD;
  const lit = Math.round(ruleW * progress);
  for (let i = 0; i < ruleW; i++) {
    const c = state.gradients ? mix(accent, hsv(210, 0.5, 1), i / ruleW) : accent;
    m.px(left + i, RULE_Y, i < lit ? c : scale(c, 0.15));
  }
}

function drawPanel(m, park, slide, accent, x0, w, progress, state) {
  drawHeader(m, park, accent, x0, w, progress, state);
  if (!park.fetched_at) {
    m.text(park.error ? 'NO DATA' : 'LOADING', x0 + PAD, ROWS_Y + 2, park.error ? RED : DIM);
  } else if (!slide.rides.length) {
    m.text('NO RIDES OPERATING', x0 + PAD, ROWS_Y + 2, DIM);
  } else {
    slide.rides.forEach((ride, i) => drawRide(m, ride, `${park.id}.${i}`, x0, w, ROWS_Y + i * ROW_PITCH, state));
  }
}

// ---- footer -----------------------------------------------------------------

/** "23:07" (colon blinking), or "11:07P" on a 12-hour clock. */
function formatClock(date, clock24) {
  const h = date.getHours();
  const colon = date.getSeconds() % 2 === 0 ? ':' : ' ';
  const mins = String(date.getMinutes()).padStart(2, '0');
  return clock24 ? `${String(h).padStart(2, '0')}${colon}${mins}` : `${h % 12 || 12}${colon}${mins}${h < 12 ? 'A' : 'P'}`;
}

function drawFooter(m, state, slide) {
  const y = footerY(m);
  const clock = formatClock(new Date(), state.clock24);
  m.text(clock, PAD, y, WHITE);

  // Position among all the pages being cycled through. Always shown, so the weather
  // gets whatever space is left rather than the other way round.
  const page = slide ? `${state.slideIndex + 1}/${state.slideCount}` : '';
  const pageX = m.cols - PAD - 6 - textWidth(page);
  m.text(page, pageX, y, GREY);

  // Status LED: green = live, amber = last poll failed, red = nothing loaded yet.
  const status = !state.feed.parks.length ? RED : state.feed.error ? AMBER : GREEN;
  m.rect(m.cols - PAD - 3, y + 2, 3, 3, status);

  // Weather for whichever park is on screen fills the space in between.
  if (state.weather) {
    const park = slide ? state.feed.parks[slide.parkIndex] : null;
    drawWeather(m, park, PAD + textWidth(clock) + 10, pageX - 8, y, state);
  }
}

// ---- the frame --------------------------------------------------------------

/** Paint one frame of the board (or whichever overlay is open) into the matrix. */
export function drawBoard(m, state) {
  const { feed, slideTime, dwell, now } = state;
  flap.enabled = state.flip;
  if (drawOverlay(m, state)) return;

  const slides = buildSlides(feed.parks, visibleRows(m), state.showClosed, state.park);
  state.slideCount = slides.length;
  const index = slides.length ? Math.floor(slideTime / dwell) % slides.length : -1;
  if (index !== state.slideIndex) {
    state.slideIndex = index;
    state.pageAge = 0; // restart the name scrolling on each new page
  }
  const slide = slides[index] ?? null;

  if (slide) {
    const park = feed.parks[slide.parkIndex];
    drawPanel(m, park, slide, accentFor(park.id), 0, m.cols, (slideTime % dwell) / dwell, state);
  } else {
    m.text(feed.error ? 'NO SIGNAL' : 'LOADING', PAD, ROWS_Y + 2, feed.error ? RED : DIM);
  }
  drawFooter(m, state, slide);

  if (state.toast && now < state.toast.until) drawToast(m, state.toast.text);
}
