import { ADVANCE, normalise, textWidth } from './font.js';
import { hsv, mix, scale } from './led.js';

// Vertical layout, in dots.
const HEADER_Y = 2;
const RULE_Y = 17;
const ROWS_Y = 20;
const ROW_PITCH = 9;
const FOOTER_H = 11;
const PAD = 3;

const WAIT_TEXT_W = 4 * ADVANCE - 1; // fits "WALK"
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
export function buildSlides(parks, size, showClosed) {
  const slides = [];
  parks.forEach((park, parkIndex) => {
    const rides = visibleRides(park, showClosed);
    const pages = Math.max(1, Math.ceil(rides.length / size));
    for (let page = 0; page < pages; page++) {
      slides.push({ parkIndex, page, pages, rides: rides.slice(page * size, (page + 1) * size) });
    }
  });
  return slides;
}

function fitText(text, width) {
  const max = Math.floor((width + 1) / ADVANCE);
  const chars = [...text];
  return chars.length <= max ? text : chars.slice(0, Math.max(0, max - 1)).join('') + '.';
}

function drawBar(m, x, y, w, mins) {
  const lit = Math.round((Math.min(mins, MAX_WAIT) / MAX_WAIT) * w);
  for (let i = 0; i < w; i++) {
    const c = waitColor((i / (w - 1)) * MAX_WAIT);
    m.rect(x + i, y, 1, 3, i < lit ? c : scale(c, 0.1));
  }
}

function drawRide(m, ride, x0, w, y) {
  const right = x0 + w - PAD;
  const barW = Math.max(10, Math.min(28, Math.round(w * 0.14)));
  const textX = right - WAIT_TEXT_W;
  const barX = textX - 4 - barW;
  const nameX = x0 + PAD;
  const name = fitText(normalise(ride.name), barX - nameX - 3);

  if (!ride.is_open) {
    m.text(name, nameX, y, DIM);
    const label = 'CLOSED';
    m.text(label, right - textWidth(label), y, scale(RED, 0.55));
    return;
  }

  const c = waitColor(ride.wait_time);
  m.text(name, nameX, y, WHITE);
  drawBar(m, barX, y + 2, barW, ride.wait_time);
  const label = ride.wait_time === 0 ? 'WALK' : `${ride.wait_time}M`;
  m.text(label, right - textWidth(label), y, c);
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

function drawPanel(m, park, slide, accent, x0, w, progress, showClosed) {
  drawHeader(m, park, accent, x0, w, progress);
  if (!park.fetched_at) {
    m.text(park.error ? 'NO DATA' : 'LOADING', x0 + PAD, ROWS_Y + 2, park.error ? RED : DIM);
    return;
  }
  if (!slide.rides.length) {
    m.text('NO RIDES OPERATING', x0 + PAD, ROWS_Y + 2, DIM);
    return;
  }
  slide.rides.forEach((ride, i) => drawRide(m, ride, x0, w, ROWS_Y + i * ROW_PITCH));
}

function drawFooter(m, state, slide, slides) {
  const y = m.rows - 8;
  const now = new Date();
  const colon = now.getSeconds() % 2 === 0 ? ':' : ' ';
  const pad2 = (n) => String(n).padStart(2, '0');
  m.text(`${pad2(now.getHours())}${colon}${pad2(now.getMinutes())}`, PAD, y, WHITE);

  const page = slide && slide.pages > 1 ? `${slide.page + 1}/${slide.pages}` : '';
  m.text(page, m.cols - PAD - 6 - textWidth(page), y, DIM);

  // Status LED: green = live, amber = last poll failed, red = nothing loaded yet.
  const status = !state.feed.parks.length ? RED : state.feed.error ? [255, 170, 30] : [40, 255, 90];
  m.rect(m.cols - PAD - 3, y + 2, 3, 3, status);
}

/** Paint one frame of the board into the matrix. */
export function drawBoard(m, state) {
  const { feed, showClosed, slideTime, dwell } = state;
  const size = pageSize(m);
  const slides = buildSlides(feed.parks, size, showClosed);
  const slide = slides.length ? slides[Math.floor(slideTime / dwell) % slides.length] : null;
  const progress = (slideTime % dwell) / dwell;

  if (slide) {
    const park = feed.parks[slide.parkIndex];
    drawPanel(m, park, slide, ACCENTS[slide.parkIndex % ACCENTS.length], 0, m.cols, progress, showClosed);
  } else {
    m.text(feed.error ? 'NO SIGNAL' : 'LOADING', PAD, ROWS_Y + 2, feed.error ? RED : DIM);
  }
  drawFooter(m, state, slide, slides);
}
