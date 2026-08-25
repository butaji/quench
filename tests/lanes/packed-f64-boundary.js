const width = 4096;
const height = 64;
const rowSize = width + 2;
const x = new Array(rowSize * (height + 2)).fill(1.25);

function copyBoundary(x) {
  for (var i = 1; i <= width; i++) {
    x[i] = x[i + rowSize];
    x[i + (height + 1) * rowSize] = x[i + height * rowSize];
  }
  for (var j = 1; j <= height; j++) {
    x[j * rowSize] = x[1 + j * rowSize];
    x[width + 1 + j * rowSize] = x[width + j * rowSize];
  }
}

function negateVerticalBoundary(x) {
  for (var i = 1; i <= width; i++) {
    x[i] = -x[i + rowSize];
    x[i + (height + 1) * rowSize] = -x[i + height * rowSize];
  }
}

for (let round = 0; round < 10000; round++) {
  copyBoundary(x);
  negateVerticalBoundary(x);
}
if (x[1] !== -1.25 || x[rowSize] !== 1.25) throw new Error("boundary result");
