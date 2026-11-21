// gaussian-blur's left-to-right convolve loop, faithfully: `height` rows of `width` pixels,
// as in convolveRGBA (a `line` of width*4 floats is written per row).
const W = 800, H = 450;
const src = new Uint32Array(W * H);
for (let i = 0; i < src.length; i++) src[i] = (i * 2654435761) >>> 0;
const line = new Float32Array(W * 4);
const coeff = new Float64Array([0.25, 0.5, 0.1, 0.05, 0.3, 0.2, 0.1, 0.15]);
function convolve(src, line, coeff, width, height) {
  var src_index = 0;
  var a0 = coeff[0], a1 = coeff[1], b1 = coeff[4], b2 = coeff[5];
  for (var i = 0; i < height; i++) {
    var line_index = 0;
    var rgba = src[src_index];
    var prev_src_r = rgba & 0xff, prev_src_g = (rgba >> 8) & 0xff, prev_src_b = (rgba >> 16) & 0xff, prev_src_a = (rgba >> 24) & 0xff;
    var prev_prev_out_r = prev_src_r * 0.1, prev_prev_out_g = prev_src_g * 0.1, prev_prev_out_b = prev_src_b * 0.1, prev_prev_out_a = prev_src_a * 0.1;
    var prev_out_r = prev_prev_out_r, prev_out_g = prev_prev_out_g, prev_out_b = prev_prev_out_b, prev_out_a = prev_prev_out_a;
    for (var j = 0; j < width; j++) {
      var px = src[src_index];
      var csr = px & 0xff, csg = (px >> 8) & 0xff, csb = (px >> 16) & 0xff, csa = (px >> 24) & 0xff;
      var cor = csr * a0 + prev_src_r * a1 + prev_out_r * b1 + prev_prev_out_r * b2;
      var cog = csg * a0 + prev_src_g * a1 + prev_out_g * b1 + prev_prev_out_g * b2;
      var cob = csb * a0 + prev_src_b * a1 + prev_out_b * b1 + prev_prev_out_b * b2;
      var coa = csa * a0 + prev_src_a * a1 + prev_out_a * b1 + prev_prev_out_a * b2;
      prev_prev_out_r = prev_out_r; prev_prev_out_g = prev_out_g; prev_prev_out_b = prev_out_b; prev_prev_out_a = prev_out_a;
      prev_out_r = cor; prev_out_g = cog; prev_out_b = cob; prev_out_a = coa;
      prev_src_r = csr; prev_src_g = csg; prev_src_b = csb; prev_src_a = csa;
      line[line_index] = prev_out_r; line[line_index + 1] = prev_out_g;
      line[line_index + 2] = prev_out_b; line[line_index + 3] = prev_out_a;
      line_index += 4; src_index++;
    }
  }
}
const t0 = performance.now();
for (let pass = 0; pass < 25; pass++) convolve(src, line, coeff, W, H);
const t1 = performance.now();
console.log("gauss_inner " + ((t1 - t0) * 1e6 / (25 * W * H)).toFixed(3) + " ns/op " + line[0]);
