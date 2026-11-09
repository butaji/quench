/* C analogue of JetStream's SeaMonster/gaussian-blur.js: the same IIR blur over an
 * 800x450 RGBA image, double arithmetic, a float line buffer and uint32 pixels, so a
 * JS engine is compared with what a C programmer writes for the same program. */
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <time.h>

enum { WIDTH = 800, HEIGHT = 450, RADIUS = 15 };

static void gauss_coef(double sigma, float coeff[8]) {
  if (sigma < 0.5) sigma = 0.5;
  double a = exp(0.726 * 0.726) / sigma, g1 = exp(-a), g2 = exp(-2 * a);
  double k = (1 - g1) * (1 - g1) / (1 + 2 * a * g1 - g2);
  double a0 = k, a1 = k * (a - 1) * g1, a2 = k * (a + 1) * g1, a3 = -k * g2;
  double b1 = 2 * g1, b2 = -g2;
  double values[8] = {a0, a1, a2, a3, b1, b2, (a0 + a1) / (1 - b1 - b2), (a2 + a3) / (1 - b1 - b2)};
  for (int i = 0; i < 8; i++) coeff[i] = (float)values[i];
}

/* JS `x << n` on a double: ToInt32, then a 32-bit shift. */
static uint32_t shl(double x, int n) { return (uint32_t)(int32_t)x << n; }

static void convolve(const uint32_t *src, uint32_t *out, float *line, const float *coeff, int width, int height) {
  for (int i = 0; i < height; i++) {
    int src_index = i * width, out_index = i, line_index = 0;
    uint32_t rgba = src[src_index];
    double psr = rgba & 0xff, psg = (rgba >> 8) & 0xff, psb = (rgba >> 16) & 0xff, psa = (rgba >> 24) & 0xff;
    double ppr = psr * coeff[6], ppg = psg * coeff[6], ppb = psb * coeff[6], ppa = psa * coeff[6];
    double por = ppr, pog = ppg, pob = ppb, poa = ppa;
    double a0 = coeff[0], a1 = coeff[1], b1 = coeff[4], b2 = coeff[5];
    for (int j = 0; j < width; j++) {
      rgba = src[src_index];
      double csr = rgba & 0xff, csg = (rgba >> 8) & 0xff, csb = (rgba >> 16) & 0xff, csa = (rgba >> 24) & 0xff;
      double cor = csr * a0 + psr * a1 + por * b1 + ppr * b2;
      double cog = csg * a0 + psg * a1 + pog * b1 + ppg * b2;
      double cob = csb * a0 + psb * a1 + pob * b1 + ppb * b2;
      double coa = csa * a0 + psa * a1 + poa * b1 + ppa * b2;
      ppr = por; ppg = pog; ppb = pob; ppa = poa;
      por = cor; pog = cog; pob = cob; poa = coa;
      psr = csr; psg = csg; psb = csb; psa = csa;
      line[line_index] = (float)por; line[line_index + 1] = (float)pog;
      line[line_index + 2] = (float)pob; line[line_index + 3] = (float)poa;
      line_index += 4;
      src_index++;
    }
    src_index--;
    line_index -= 4;
    out_index += height * (width - 1);
    rgba = src[src_index];
    psr = rgba & 0xff; psg = (rgba >> 8) & 0xff; psb = (rgba >> 16) & 0xff; psa = (rgba >> 24) & 0xff;
    ppr = psr * coeff[7]; ppg = psg * coeff[7]; ppb = psb * coeff[7]; ppa = psa * coeff[7];
    por = ppr; pog = ppg; pob = ppb; poa = ppa;
    double csr = psr, csg = psg, csb = psb, csa = psa;
    a0 = coeff[2]; a1 = coeff[3];
    for (int j = width - 1; j >= 0; j--) {
      double cor = csr * a0 + psr * a1 + por * b1 + ppr * b2;
      double cog = csg * a0 + psg * a1 + pog * b1 + ppg * b2;
      double cob = csb * a0 + psb * a1 + pob * b1 + ppb * b2;
      double coa = csa * a0 + psa * a1 + poa * b1 + ppa * b2;
      ppr = por; ppg = pog; ppb = pob; ppa = poa;
      por = cor; pog = cog; pob = cob; poa = coa;
      psr = csr; psg = csg; psb = csb; psa = csa;
      rgba = src[src_index];
      csr = rgba & 0xff; csg = (rgba >> 8) & 0xff; csb = (rgba >> 16) & 0xff; csa = (rgba >> 24) & 0xff;
      out[out_index] = shl(line[line_index] + por, 0) + shl(line[line_index + 1] + pog, 8) +
                       shl(line[line_index + 2] + pob, 16) + shl(line[line_index + 3] + poa, 24);
      src_index--;
      line_index -= 4;
      out_index -= height;
    }
  }
}

static void blur(uint32_t *src, uint32_t *out, float *line) {
  float coeff[8];
  gauss_coef(RADIUS, coeff);
  convolve(src, out, line, coeff, WIDTH, HEIGHT);
  convolve(out, src, line, coeff, HEIGHT, WIDTH);
}

int main(int argc, char **argv) {
  int iterations = argc > 1 ? atoi(argv[1]) : 120;
  uint32_t *buffer = malloc(sizeof(uint32_t) * WIDTH * HEIGHT);
  int32_t seed = 49734321;
  for (int i = 0; i < WIDTH * HEIGHT; i++) {
    /* Robert Jenkins' 32-bit hash with JS int32 wraparound. */
    uint32_t s = (uint32_t)seed;
    s = (s + 0x7ed55d16u) + (s << 12);
    s = (s ^ 0xc761c23cu) ^ (s >> 19);
    s = (s + 0x165667b1u) + (s << 5);
    s = (s + 0xd3a2646cu) ^ (s << 9);
    s = (s + 0xfd7046c5u) + (s << 3);
    s = (s ^ 0xb55a4f09u) ^ (s >> 16);
    seed = (int32_t)s;
    buffer[i] = s & 0xfffffff;
  }
  struct timespec a, b;
  clock_gettime(CLOCK_MONOTONIC, &a);
  for (int i = 0; i < iterations; i++) {
    /* blurRGBA allocates its output and line buffers per call. */
    uint32_t *out = calloc(WIDTH * HEIGHT, sizeof(uint32_t));
    float *line = calloc((WIDTH > HEIGHT ? WIDTH : HEIGHT) * 4, sizeof(float));
    blur(buffer, out, line);
    free(out);
    free(line);
  }
  clock_gettime(CLOCK_MONOTONIC, &b);
  uint32_t checksum = 0;
  for (int i = 0; i < WIDTH * HEIGHT; i++) checksum = checksum * 31 + buffer[i];
  printf("gaussian-blur %d iterations %.3f ms checksum %u\n", iterations,
         (b.tv_sec - a.tv_sec) * 1e3 + (b.tv_nsec - a.tv_nsec) / 1e6, checksum);
  return 0;
}
