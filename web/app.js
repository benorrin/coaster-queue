import { Matrix } from './led.js';
import { Feed } from './data.js';
import { drawBoard } from './board.js';
import { bootWipe, shade } from './fx.js';
import { setupControls } from './controls.js';
import { load, sanitise, sanitiseGroups } from './settings.js';

const params = new URLSearchParams(location.search);
// The server's settings are the baseline; this browser's own changes and the URL sit on top.
const server = await fetch('/api/settings').then((r) => (r.ok ? r.json() : {})).catch(() => ({}));
const settings = load(params, server);
const demo = params.has('demo');

const matrix = new Matrix(document.getElementById('board'), settings.cols);
addEventListener('resize', () => matrix.resize());

const state = {
  ...settings,
  feed: null, // set below, once the state it reads from exists
  paused: false,
  help: false,
  menu: { open: false, index: 0, offset: 0, visible: 0 },
  picker: { open: false, key: 'parks', index: 0, offset: 0, visible: 0 },
  toast: null,
  slideTime: 0, // seconds of paging clock; stops while paused
  pageAge: 0, // seconds since the page last changed; drives name scrolling
  slideCount: 0,
  slideIndex: -1,
  now: 0, // ms since load
};
state.feed = new Feed({ demo, selected: () => state.parks, shown: () => state.show });
state.feed.onDirectory = (directory) => {
  sanitise(state, directory);
  state.feed.poll();
};
state.feed.onGroups = (groups) => {
  sanitiseGroups(state, groups);
  state.feed.poll();
};
setupControls({ state, matrix, demo });

let last = performance.now();
function frame(now) {
  const dt = Math.min(0.25, (now - last) / 1000);
  last = now;
  if (!state.paused && !state.menu.open && !state.picker.open) state.slideTime += dt;
  state.pageAge += dt;
  state.now = now;

  document.body.classList.toggle('flat', !state.gradients);
  matrix.clear();
  drawBoard(matrix, state);
  shade(matrix, state.theme, now, state.shine);
  bootWipe(matrix, now);
  matrix.present(state.brightness);
  requestAnimationFrame(frame);
}
requestAnimationFrame(frame);
