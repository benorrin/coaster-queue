import { DIM, WHITE, hsv } from './color.js';
import { textWidth } from './font.js';
import { flap } from './fx.js';

// WMO weather interpretation codes, as returned by Open-Meteo.
function describe(code) {
  if (code === 0) return ['CLEAR', 'sun'];
  if (code === 1) return ['MOSTLY CLEAR', 'sun'];
  if (code === 2) return ['PARTLY CLOUDY', 'partly'];
  if (code === 3) return ['OVERCAST', 'cloud'];
  if (code === 45 || code === 48) return ['FOG', 'fog'];
  if (code >= 51 && code <= 57) return ['DRIZZLE', 'rain'];
  if (code >= 61 && code <= 67) return [code >= 65 ? 'HEAVY RAIN' : 'RAIN', 'rain'];
  if (code >= 71 && code <= 77) return ['SNOW', 'snow'];
  if (code >= 80 && code <= 82) return ['SHOWERS', 'rain'];
  if (code === 85 || code === 86) return ['SNOW SHOWERS', 'snow'];
  if (code >= 95) return ['THUNDERSTORM', 'storm'];
  return ['UNKNOWN', 'cloud'];
}

const PALETTE = {
  Y: [255, 200, 30],
  W: [190, 200, 220],
  B: [60, 150, 255],
  S: [255, 255, 255],
  O: [255, 210, 0],
  G: [120, 130, 150],
};

// 9x7 icons; rain, snow and fog alternate between two frames.
const CLOUD = ['...WWW...', '..WWWWW..', '.WWWWWWWW', 'WWWWWWWWW'];
const ICONS = {
  sun: [['....Y....', '.Y..Y..Y.', '...YYY...', 'YY.YYY.YY', '...YYY...', '.Y..Y..Y.', '....Y....']],
  partly: [['.Y.......', 'YYYY.WW..', '.YYWWWWW.', '..WWWWWWW', '..WWWWWWW', '...WWWWW.', '.........']],
  cloud: [['.........', ...CLOUD.slice(0, 3), 'WWWWWWWWW', '.WWWWWWW.', '.........']],
  rain: [
    [...CLOUD, '.........', '.B..B..B.', '.........'],
    [...CLOUD, '.........', '.........', '..B..B..B'],
  ],
  snow: [
    [...CLOUD, '.........', '.S..S..S.', '.........'],
    [...CLOUD, '.........', '.........', '..S..S..S'],
  ],
  storm: [
    [...CLOUD, '....OO...', '...OO....', '....O....'],
    [...CLOUD, '.........', '...OO....', '....O....'],
  ],
  fog: [
    ['.........', 'GGGGGGG..', '.........', '..GGGGGGG', '.........', 'GGGGGGG..', '.........'],
    ['.........', '..GGGGGGG', '.........', 'GGGGGGG..', '.........', '..GGGGGGG', '.........'],
  ],
};

const ICON_W = 9;

/** Draw a weather icon with its top-left dot at (x, y). */
function drawIcon(m, name, x, y, now) {
  const frames = ICONS[name];
  const rows = frames[Math.floor(now / 450) % frames.length];
  rows.forEach((row, j) => [...row].forEach((ch, i) => ch !== '.' && m.px(x + i, y + j, PALETTE[ch])));
}

/** Cold is blue, mild green, hot red. */
function tempColor(c) {
  return hsv(220 - 220 * Math.min(1, Math.max(0, c / 30)), 0.9, 1);
}

/** Structured summary of a park's weather reading, or null if there isn't one. */
function summarise(w, { feelsLike = true } = {}) {
  if (!w) return null;
  const [text, icon] = describe(w.code);
  const t = (v) => `${Math.round(v)}C`;
  return {
    icon,
    text,
    temp: t(w.temp_c),
    tempC: w.temp_c,
    // Everything after the temperature; the footer scrolls it if it doesn't all fit.
    extras: [
      `WIND ${Math.round(w.wind_mph)}MPH`,
      ...(feelsLike ? [`FEELS ${t(w.feels_c)}`] : []),
      ...(w.rain_mm > 0 ? [`RAIN ${w.rain_mm.toFixed(1)}MM`] : []),
    ],
  };
}

const SCROLL_SPEED = 24; // dots per second when the weather is too long to fit
const LOOP_GAP = '      ';

/**
 * Draw a park's weather on the footer row `y` between `x0` and `x1`: an icon, then the
 * condition, temperature and extras. It sits still if it fits, otherwise it scrolls round.
 */
export function drawWeather(m, park, x0, x1, y, { now, feelsLike }) {
  const wx = summarise(park?.weather, { feelsLike });
  if (!wx) return;
  drawIcon(m, wx.icon, x0, y, now);
  const x = x0 + ICON_W + 4;

  // One string, coloured piece by piece, so it can scroll as a unit.
  const parts = [
    [wx.text, WHITE],
    [wx.temp, tempColor(wx.tempC)],
    ...wx.extras.map((e) => [e, DIM]),
  ];
  const colours = parts.flatMap(([text, colour], i) => [...text].map(() => colour).concat(i < parts.length - 1 ? [DIM, DIM] : []));
  const colourAt = (i) => colours[i] ?? DIM;
  const opts = { clip: [x, x1] };
  let text = parts.map(([t]) => t).join('  ');

  if (textWidth(text) <= x1 - x) {
    flap.draw(m, 'wx', text, x, y, colourAt, now, opts);
    return;
  }

  // Too long: loop it, with a gap before it comes round again.
  text += LOOP_GAP;
  const loop = textWidth(text) + 1;
  const shift = Math.floor(((now / 1000) * SCROLL_SPEED) % loop);
  flap.draw(m, 'wx', text, x - shift, y, colourAt, now, opts);
  m.text(text, x - shift + loop, y, colourAt, opts);
}
