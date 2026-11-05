/* C analogue of JetStream's SeaMonster/gaussian-blur.js: the same recursive Gaussian
 * convolution over Uint32Array pixels and a Float32Array line, the same seeded input, and
 * JavaScript's ToInt32/ToUint32 conversions where the source applies them. */
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <time.h>

static int32_t to_int32(double x) { return (int32_t)(uint32_t)(int64_t)x; }
static uint32_t to_uint32(double x) { return (uint32_t)(int64_t)x; }

static void gauss_coef(double sigma, float coeff[8]) {
  if (sigma < 0.5) sigma = 0.5;
  double a = exp(0.726 * 0.726) / sigma, g1 = exp(-a), g2 = exp(-2 * a);
  double k = (1 - g1) * (1 - g1) / (1 + 2 * a * g1 - g2);
  double a0 = k, a1 = k * (a - 1) * g1, a2 = k * (a + 1) * g1, a3 = -k * g2, b1 = 2 * g1, b2 = -g2;
  double values[8] = {a0, a1, a2, a3, b1, b2, (a0 + a1) / (1 - b1 - b2), (a2 + a3) / (1 - b1 - b2)};
  for (int i = 0; i < 8; i++) coeff[i] = (float)values[i];
}

static void convolve_rgba(const uint32_t *src, uint32_t *out, float *line, const float *coeff, int width, int height) {
  for (int i = 0; i < height; i++) {
    int src_index = i * width, out_index = i, line_index = 0;
    int32_t rgba = (int32_t)src[src_index];
    double prev_src[4], prev_out[4], prev_prev_out[4], curr_src[4], curr_out[4];
    for (int c = 0; c < 4; c++) {
      prev_src[c] = (rgba >> (8 * c)) & 0xff;
      prev_prev_out[c] = prev_src[c] * coeff[6];
      prev_out[c] = prev_prev_out[c];
    }
    double a0 = coeff[0], a1 = coeff[1], b1 = coeff[4], b2 = coeff[5];
    for (int j = 0; j < width; j++) {
      rgba = (int32_t)src[src_index];
      for (int c = 0; c < 4; c++) {
        curr_src[c] = (rgba >> (8 * c)) & 0xff;
        curr_out[c] = curr_src[c] * a0 + prev_src[c] * a1 + prev_out[c] * b1 + prev_prev_out[c] * b2;
        prev_prev_out[c] = prev_out[c];
        prev_out[c] = curr_out[c];
        prev_src[c] = curr_src[c];
        line[line_index + c] = (float)prev_out[c];
      }
      line_index += 4;
      src_index++;
    }
    src_index--;
    line_index -= 4;
    out_index += height * (width - 1);
    rgba = (int32_t)src[src_index];
    for (int c = 0; c < 4; c++) {
      prev_src[c] = (rgba >> (8 * c)) & 0xff;
      prev_prev_out[c] = prev_src[c] * coeff[7];
      prev_out[c] = prev_prev_out[c];
      curr_src[c] = prev_src[c];
    }
    a0 = coeff[2]; a1 = coeff[3];
    for (int j = width - 1; j >= 0; j--) {
      for (int c = 0; c < 4; c++) {
        curr_out[c] = curr_src[c] * a0 + prev_src[c] * a1 + prev_out[c] * b1 + prev_prev_out[c] * b2;
        prev_prev_out[c] = prev_out[c];
        prev_out[c] = curr_out[c];
        prev_src[c] = curr_src[c];
      }
      rgba = (int32_t)src[src_index];
      for (int c = 0; c < 4; c++) curr_src[c] = (rgba >> (8 * c)) & 0xff;
      double sum = 0;
      for (int c = 0; c < 4; c++) sum += (int32_t)((uint32_t)to_int32(line[line_index + c] + prev_out[c]) << (8 * c));
      out[out_index] = to_uint32(sum);
      src_index--;
      line_index -= 4;
      out_index -= height;
    }
  }
}

static void blur_rgba(uint32_t *src, int width, int height, double radius, uint32_t *out, float *line) {
  float coeff[8];
  gauss_coef(radius, coeff);
  convolve_rgba(src, out, line, coeff, width, height);
  convolve_rgba(out, src, line, coeff, height, width);
}

int main(int argc, char **argv) {
  int iterations = argc > 1 ? atoi(argv[1]) : 120;
  enum { WIDTH = 800, HEIGHT = 450, RADIUS = 15 };
  uint32_t *buffer = malloc(sizeof(uint32_t) * WIDTH * HEIGHT);
  int32_t seed = 49734321;
  for (int i = 0; i < WIDTH * HEIGHT; ++i) {
    seed = to_int32((double)seed + 0x7ed55d16 + (int32_t)((uint32_t)seed << 12));
    seed = (seed ^ (int32_t)0xc761c23c) ^ (int32_t)((uint32_t)seed >> 19);
    seed = to_int32((double)seed + 0x165667b1 + (int32_t)((uint32_t)seed << 5));
    seed = to_int32((double)seed + 0xd3a2646c) ^ (int32_t)((uint32_t)seed << 9);
    seed = to_int32((double)seed + 0xfd7046c5 + (int32_t)((uint32_t)seed << 3));
    seed = (seed ^ (int32_t)0xb55a4f09) ^ (int32_t)((uint32_t)seed >> 16);
    buffer[i] = (uint32_t)(seed & 0xfffffff);
  }
  /* The JS source allocates the output and line per blur; both are fully overwritten. */
  struct timespec a, b;
  clock_gettime(CLOCK_MONOTONIC, &a);
  for (int i = 0; i < iterations; i++) {
    uint32_t *out = malloc(sizeof(uint32_t) * WIDTH * HEIGHT);
    float *line = calloc((WIDTH > HEIGHT ? WIDTH : HEIGHT) * 4, sizeof(float));
    blur_rgba(buffer, WIDTH, HEIGHT, RADIUS, out, line);
    free(out); free(line);
  }
  clock_gettime(CLOCK_MONOTONIC, &b);
  double checksum = 0;
  for (int i = 0; i < WIDTH * HEIGHT; i++) checksum += buffer[i];
  printf("gaussian-blur %d iterations %.3f ms checksum %.0f\n", iterations, (b.tv_sec - a.tv_sec) * 1e3 + (b.tv_nsec - a.tv_nsec) / 1e6, checksum);
  return 0;
}
