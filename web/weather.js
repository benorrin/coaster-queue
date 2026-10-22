import { hsv } from './led.js';

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

export const ICON_W = 9;

/** Draw a weather icon with its top-left dot at (x, y). */
export function drawIcon(m, name, x, y, now) {
  const frames = ICONS[name];
  const rows = frames[Math.floor(now / 450) % frames.length];
  rows.forEach((row, j) => [...row].forEach((ch, i) => ch !== '.' && m.px(x + i, y + j, PALETTE[ch])));
}

/** Cold is blue, mild green, hot red. */
export function tempColor(c) {
  return hsv(220 - 220 * Math.min(1, Math.max(0, c / 30)), 0.9, 1);
}

/** Structured summary of a park's weather reading, or null if there isn't one. */
export function summarise(w) {
  if (!w) return null;
  const [text, icon] = describe(w.code);
  const t = (v) => `${Math.round(v)}C`;
  return {
    icon,
    text,
    temp: t(w.temp_c),
    tempC: w.temp_c,
    // Optional extras, most useful first; the footer shows as many as fit.
    extras: [
      `WIND ${Math.round(w.wind_mph)}MPH`,
      `FEELS ${t(w.feels_c)}`,
      ...(w.rain_mm > 0 ? [`RAIN ${w.rain_mm.toFixed(1)}MM`] : []),
    ],
  };
}
