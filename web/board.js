import { ADVANCE, normalise, textWidth } from './font.js';
import { hsv, mix, scale } from './led.js';
import { Flap, Ease } from './fx.js';
import { ICON_W, drawIcon, summarise, tempColor } from './weather.js';

export const flap = new Flap();
export const ease = new Ease();

// Vertical layout, in dots.
const HEADER_Y = 2;
const RULE_Y = 17;
const ROWS_Y = 20;
const ROW_PITCH = 9;
const FOOTER_H = 11;
const PAD = 3;

const WAIT_TEXT_W = 4 * ADVANCE - 1; // fits "WALK"
const ARROW_W = 5;
const SCROLL_SPEED = 30; // dots per second for names that don't fit
const SCROLL_HOLD = 1.4; // seconds parked at each end
const MAX_WAIT = 60;

const WHITE = [235, 240, 255];
const DIM = [90, 98, 120];
const RED = [255, 40, 40];
const ACCENTS = [
  [255, 170, 30],
  [40, 200, 255],
];

/** Green at no wait, through yellow, to red at an hour or more. */
export function waitColor(mins) {
  return hsv(130 * (1 - Math.min(mins, MAX_WAIT) / MAX_WAIT), 1, 1);
}

export function pageSize(matrix) {
  return Math.max(1, Math.floor((matrix.rows - FOOTER_H - ROWS_Y + 2) / ROW_PITCH));
}

/** Rides worth showing for a park: open ones, longest queue first, then (optionally) closed. */
export function visibleRides(park, showClosed) {
  const open = park.rides.filter((r) => r.is_open);
  return showClosed ? [...open, ...park.rides.filter((r) => !r.is_open)] : open;
}

/** One slide per page of rides, for every park in turn. */
export function buildSlides(parks, size, showClosed, only = null) {
  const slides = [];
  parks.forEach((park, parkIndex) => {
    if (only && park.id !== only) return;
    const rides = visibleRides(park, showClosed);
    const pages = Math.max(1, Math.ceil(rides.length / size));
    for (let page = 0; page < pages; page++) {
      slides.push({ parkIndex, page, pages, rides: rides.slice(page * size, (page + 1) * size) });
    }
  });
  return slides;
}

/** Horizontal offset for a name wider than its column: park, scroll, park. */
function scrollOffset(overflow, t) {
  if (overflow <= 0) return 0;
  const run = overflow / SCROLL_SPEED;
  const u = t % (run + 2 * SCROLL_HOLD);
  return -Math.min(overflow, Math.max(0, (u - SCROLL_HOLD) * SCROLL_SPEED));
}

function drawBar(m, key, x, y, w, mins, now) {
  const fill = ease.get(key, (Math.min(mins, MAX_WAIT) / MAX_WAIT) * w, now);
  for (let i = 0; i < w; i++) {
    const c = waitColor((i / (w - 1)) * MAX_WAIT);
    const on = Math.max(0, Math.min(1, fill - i));
    m.rect(x + i, y, 1, 3, mix(scale(c, 0.1), c, on));
  }
}

function drawRide(m, ride, key, x0, w, y, state) {
  const { now, pageAge } = state;
  const right = x0 + w - PAD;
  // Narrow screens drop the bar so the name keeps a useful width.
  const barW = w < 150 ? 0 : Math.max(10, Math.min(28, Math.round(w * 0.14)));
  const textX = right - WAIT_TEXT_W;
  const arrowX = textX - ARROW_W - 2;
  const barX = arrowX - (barW ? 4 + barW : 1);
  const nameX = x0 + PAD;
  const nameW = barX - nameX - 3;
  const name = normalise(ride.name);
  const offset = scrollOffset(textWidth(name) - nameW, pageAge);
  const nameOpts = { clip: [nameX, nameX + nameW] };

  if (!ride.is_open) {
    flap.draw(m, `${key}.n`, name, nameX + offset, y, DIM, now, nameOpts);
    const label = 'CLOSED';
    m.text(label, right - textWidth(label), y, scale(RED, 0.55));
    return;
  }

  // Long queues breathe so they catch the eye from across the concourse.
  const pulse = ride.wait_time >= 50 ? 0.75 + 0.25 * Math.sin(now / 170) : 1;
  const c = scale(waitColor(ride.wait_time), pulse);
  flap.draw(m, `${key}.n`, name, nameX + offset, y, WHITE, now, nameOpts);
  if (barW) drawBar(m, `${key}.b`, barX, y + 2, barW, ride.wait_time, now);
  if (ride.trend) m.text(ride.trend > 0 ? '▲' : '▼', arrowX, y, ride.trend > 0 ? RED : [40, 255, 90]);
  const label = ride.wait_time === 0 ? 'WALK' : `${ride.wait_time}M`;
  flap.draw(m, `${key}.w`, label, right - textWidth(label), y, c, now);
}

function drawHeader(m, park, accent, x0, w, progress) {
  const name = normalise(park.name);
  const open = park.rides.filter((r) => r.is_open);
  // Double-height title only when it leaves room for the stats block beside it.
  const statsW = 7 * ADVANCE;
  const big = textWidth(name, 2) + statsW + 3 * PAD <= w;
  m.text(name, x0 + PAD, big ? HEADER_Y : HEADER_Y + 3, accent, { scale: big ? 2 : 1 });

  if (open.length && big) {
    const avg = Math.round(open.reduce((s, r) => s + r.wait_time, 0) / open.length);
    const stats = [`${open.length} OPEN`, `AVG ${avg}M`];
    stats.forEach((s, i) => m.text(s, x0 + w - PAD - textWidth(s), HEADER_Y + i * 8, i ? DIM : WHITE));
  } else if (!open.length && park.fetched_at) {
    const s = 'CLOSED';
    m.text(s, x0 + w - PAD - textWidth(s), HEADER_Y + 3, scale(RED, 0.8));
  }

  // Rule under the header doubles as the page-dwell progress bar.
  const lit = Math.round((w - 2 * PAD) * progress);
  for (let i = 0; i < w - 2 * PAD; i++) {
    const c = mix(accent, hsv(210, 0.5, 1), i / (w - 2 * PAD));
    m.px(x0 + PAD + i, RULE_Y, i < lit ? c : scale(c, 0.15));
  }
}

function drawPanel(m, park, slide, accent, x0, w, progress, state) {
  drawHeader(m, park, accent, x0, w, progress);
  if (!park.fetched_at) {
    m.text(park.error ? 'NO DATA' : 'LOADING', x0 + PAD, ROWS_Y + 2, park.error ? RED : DIM);
    return;
  }
  if (!slide.rides.length) {
    m.text('NO RIDES OPERATING', x0 + PAD, ROWS_Y + 2, DIM);
    return;
  }
  slide.rides.forEach((ride, i) => drawRide(m, ride, `${park.id}.${i}`, x0, w, ROWS_Y + i * ROW_PITCH, state));
}

function drawWeather(m, park, x0, x1, y, now) {
  const wx = summarise(park?.weather);
  if (!wx) return;
  let x = x0;
  drawIcon(m, wx.icon, x, y, now);
  x += ICON_W + 4;
  x += flap.draw(m, 'wx.c', wx.text, x, y, WHITE, now) + 6;
  x += flap.draw(m, 'wx.t', wx.temp, x, y, tempColor(wx.tempC), now) + 6;
  wx.extras.forEach((extra, i) => {
    if (x + textWidth(extra) > x1) return;
    x += flap.draw(m, `wx.e${i}`, extra, x, y, DIM, now) + 6;
  });
}

function drawFooter(m, state, slide) {
  const { now } = state;
  const y = m.rows - 8;
  const date = new Date();
  const colon = date.getSeconds() % 2 === 0 ? ':' : ' ';
  const pad2 = (n) => String(n).padStart(2, '0');
  const clock = `${pad2(date.getHours())}${colon}${pad2(date.getMinutes())}`;
  m.text(clock, PAD, y, WHITE);

  const page = slide && slide.pages > 1 ? `${slide.page + 1}/${slide.pages}` : '';
  const pageX = m.cols - PAD - 6 - textWidth(page);
  m.text(page, pageX, y, DIM);

  // Status LED: green = live, amber = last poll failed, red = nothing loaded yet.
  const status = !state.feed.parks.length ? RED : state.feed.error ? [255, 170, 30] : [40, 255, 90];
  m.rect(m.cols - PAD - 3, y + 2, 3, 3, status);

  // Weather for whichever park is on screen fills the space in between.
  const park = slide ? state.feed.parks[slide.parkIndex] : null;
  drawWeather(m, park, PAD + textWidth(clock) + 10, pageX - 8, y, now);
}

function drawBox(m, lines, y, color) {
  const w = Math.max(...lines.map((l) => textWidth(l))) + 10;
  const h = lines.length * ROW_PITCH + 3;
  const x = Math.floor((m.cols - w) / 2);
  m.rect(x, y, w, h, [0, 0, 0]);
  for (let i = 0; i < w; i++) {
    m.px(x + i, y, color);
    m.px(x + i, y + h - 1, color);
  }
  for (let j = 0; j < h; j++) {
    m.px(x, y + j, color);
    m.px(x + w - 1, y + j, color);
  }
  lines.forEach((l, i) => m.text(l, x + 5, y + 3 + i * ROW_PITCH, WHITE));
}

const HELP = [
  ['SPACE', 'PAUSE'],
  ['LEFT RIGHT', 'PAGE'],
  ['T', 'THEME'],
  ['UP DOWN', 'BRIGHTNESS'],
  ['D', 'DOT DENSITY'],
  ['C', 'CLOSED RIDES'],
  ['1 2 0', 'ONE PARK OR ALL'],
  ['F', 'FULLSCREEN'],
];

function drawHelp(m, now) {
  m.text('CONTROLS', PAD, HEADER_Y, (i) => hsv(now / 12 + i * 20, 0.8, 1), { scale: 2 });
  const keyW = 11 * ADVANCE;
  const perCol = Math.max(1, Math.floor((m.rows - FOOTER_H - ROWS_Y) / ROW_PITCH));
  const colW = keyW + 22 * ADVANCE;
  HELP.forEach(([key, what], i) => {
    const col = Math.floor(i / perCol);
    if (PAD + col * colW + keyW + textWidth(what) > m.cols) return;
    const x = PAD + col * colW;
    const y = ROWS_Y + (i % perCol) * ROW_PITCH;
    m.text(key, x, y, [40, 200, 255]);
    m.text(what, x + keyW, y, WHITE);
  });
  m.text('H CLOSES - TAP TO PAGE', PAD, m.rows - 8, DIM);
}

/** Paint one frame of the board into the matrix. */
export function drawBoard(m, state) {
  const { feed, showClosed, slideTime, dwell, now } = state;
  if (state.help) return drawHelp(m, now);

  const size = pageSize(m);
  const slides = buildSlides(feed.parks, size, showClosed, state.park);
  state.slideCount = slides.length;
  const index = slides.length ? Math.floor(slideTime / dwell) % slides.length : -1;
  if (index !== state.slideIndex) {
    state.slideIndex = index;
    state.pageAge = 0;
  }
  const slide = slides[index] ?? null;
  const progress = (slideTime % dwell) / dwell;

  if (slide) {
    const park = feed.parks[slide.parkIndex];
    drawPanel(m, park, slide, ACCENTS[slide.parkIndex % ACCENTS.length], 0, m.cols, progress, state);
  } else {
    m.text(feed.error ? 'NO SIGNAL' : 'LOADING', PAD, ROWS_Y + 2, feed.error ? RED : DIM);
  }
  drawFooter(m, state, slide);

  if (state.toast && now < state.toast.until) {
    drawBox(m, [normalise(state.toast.text)], Math.floor(m.rows / 2) - 6, [40, 200, 255]);
  }
}
