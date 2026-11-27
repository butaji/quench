// Four Float32Array stores + a u32 read per iteration, as gaussian-blur's inner loop does,
// with the line index bounded (one row of `width` pixels).
const width = 800, rows = 450;
const src = new Uint32Array(width * rows);
for (let i = 0; i < src.length; i++) src[i] = (i * 2654435761) >>> 0;
const line = new Float32Array(width * 4);
function row(src, line, width, base) {
  var li = 0;
  var a = 1.5, b = 2.5, c = 3.5, d = 4.5;
  for (var j = 0; j < width; j++) {
    var px = src[base + j];
    a = (px & 0xff) * 0.25 + a * 0.5;
    b = ((px >> 8) & 0xff) * 0.25 + b * 0.5;
    c = ((px >> 16) & 0xff) * 0.25 + c * 0.5;
    d = ((px >> 24) & 0xff) * 0.25 + d * 0.5;
    line[j * 4] = a; line[j * 4 + 1] = b; line[j * 4 + 2] = c; line[j * 4 + 3] = d;
    li += 4;
  }
}
const t0 = performance.now();
for (let pass = 0; pass < 25; pass++)
  for (let r = 0; r < rows; r++) row(src, line, width, r * width);
const t1 = performance.now();
console.log("ta_f32 " + ((t1 - t0) * 1e6 / (25 * width * rows)).toFixed(3) + " ns/op " + line[0]);
