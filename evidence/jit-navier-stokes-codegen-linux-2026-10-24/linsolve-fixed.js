var width = 128, height = 128, rowSize = width + 2;
var size = (width + 2) * (height + 2);
var x = new Array(size), x0 = new Array(size);
for (var n = 0; n < size; n++) { x[n] = n * 0.5; x0[n] = n * 0.25; }
function lin_solve(x, x0, a, c, iterations) {
    var invC = 1 / c;
    for (var k = 0; k < iterations; k++) {
        for (var j = 1; j <= height; j++) {
            var lastRow = (j - 1) * rowSize;
            var currentRow = j * rowSize;
            var nextRow = (j + 1) * rowSize;
            var lastX = x[currentRow];
            ++currentRow;
            for (var i = 1; i <= width; i++)
                lastX = x[currentRow] = (x0[currentRow] + a*(lastX+x[++currentRow]+x[++lastRow]+x[++nextRow])) * invC;
        }
    }
}
var t0=Date.now(); for (var r = 0; r < 200; r++) lin_solve(x, x0, 0.1, 1.4, 20);
var sum = 0; for (var n = 0; n < size; n++) sum += x[n];
console.log(sum.toFixed(6), Date.now()-t0, "ms");
