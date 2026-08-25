const width = 4096;
const rowSize = width + 2;
const cells = rowSize * 3;
const x = new Array(cells).fill(1.01);
const x0 = new Array(cells).fill(2.03);

const a = 0.1;
const invC = 0.5;
let checksum = 0;
for (let round = 0; round < 6104; round++) {
  let lastRow = 0;
  let currentRow = rowSize;
  let nextRow = rowSize * 2;
  let lastX = x[currentRow];
  ++currentRow;
  for (let i = 1; i <= width; i++)
    lastX = x[currentRow] =
      (x0[currentRow] +
        a * (lastX + x[++currentRow] + x[++lastRow] + x[++nextRow])) *
      invC;
  checksum += lastX;
}

if (!(checksum > 0)) throw new Error("jacobi result");
