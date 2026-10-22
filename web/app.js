import { Matrix } from './led.js';
import { Feed } from './data.js';
import { drawBoard } from './board.js';
import { bootWipe, shade } from './fx.js';
import { readSettings, setupControls } from './controls.js';

const params = new URLSearchParams(location.search);
const settings = readSettings(params);
const demo = params.has('demo');

const matrix = new Matrix(document.getElementById('board'), settings.cols);
addEventListener('resize', () => matrix.resize());

const state = {
  ...settings,
  feed: new Feed({ demo }),
  paused: false,
  help: false,
  toast: null,
  slideTime: 0, // seconds of paging clock; stops while paused
  pageAge: 0, // seconds since the page last changed; drives name scrolling
  slideCount: 0,
  slideIndex: -1,
  now: 0, // ms since load
};
setupControls({ state, matrix, demo });

let last = performance.now();
function frame(now) {
  const dt = Math.min(0.25, (now - last) / 1000);
  last = now;
  if (!state.paused) state.slideTime += dt;
  state.pageAge += dt;
  state.now = now;

  matrix.clear();
  drawBoard(matrix, state);
  shade(matrix, state.theme, now);
  bootWipe(matrix, now);
  matrix.present(state.brightness, state.glow);
  requestAnimationFrame(frame);
}
requestAnimationFrame(frame);
