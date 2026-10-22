import { Matrix, hsv } from './led.js';

const matrix = new Matrix(document.getElementById('board'));
addEventListener('resize', () => matrix.resize());

// Placeholder pattern until the board is wired to live data.
function frame(t) {
  matrix.clear();
  const { cols, rows } = matrix;
  for (let x = 0; x < cols; x++) {
    matrix.rect(x, rows - 3, 1, 2, hsv(x * 3 + t / 20));
  }
  const title = 'MERLIN QUEUE';
  matrix.text(title, 4, 4, (i) => hsv(i * 28 + t / 10), { scale: 2 });
  matrix.text('LED MATRIX TEST 0123456789', 4, 24, [255, 255, 255]);
  matrix.present();
  requestAnimationFrame(frame);
}
requestAnimationFrame(frame);
