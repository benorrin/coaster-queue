import { Matrix } from './led.js';
import { Feed } from './data.js';
import { drawBoard } from './board.js';

const params = new URLSearchParams(location.search);
const matrix = new Matrix(document.getElementById('board'), 200);
addEventListener('resize', () => matrix.resize());

const state = {
  feed: new Feed({ demo: params.has('demo') }),
  showClosed: false,
  dwell: 8, // seconds per page
  slideTime: 0,
};

let last = performance.now();
function frame(now) {
  state.slideTime += (now - last) / 1000;
  last = now;
  matrix.clear();
  drawBoard(matrix, state);
  matrix.present();
  requestAnimationFrame(frame);
}
requestAnimationFrame(frame);
