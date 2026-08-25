const width = 128;
const height = 128;
const rowSize = width + 2;
const cells = rowSize * (height + 2);
const x = new Array(cells).fill(1.01);
const x0 = new Array(cells).fill(2.03);

const a = 0.1;
const invC = 0.5;
for (var run = 0; run < 100; run++) {
  for (var k = 0; k < 20; k++) {
    for (var j = 1; j <= height; j++) {
      var lastRow = (j - 1) * rowSize;
      var currentRow = j * rowSize;
      var nextRow = (j + 1) * rowSize;
      var lastX = x[currentRow];
      ++currentRow;
      for (var i = 1; i <= width; i++)
        lastX = x[currentRow] =
          (x0[currentRow] +
            a * (lastX + x[++currentRow] + x[++lastRow] + x[++nextRow])) *
          invC;
    }
  }
}

const checksum = x[height * rowSize + width];
if (!(checksum > 0)) throw new Error("jacobi result");
