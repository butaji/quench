// jetstream-row: segmentation
// jetstream-files: worker/segmentation.js
// iterations: 36
// jetstream-driver: class=Async preload=asyncTaskBlob=worker/async-task.js worker=true
/* C analogue of JetStream's worker/segmentation.js benchmark.
 *
 * The JetStream row does not segment text with Intl.Segmenter. Its iteration runs
 * Statistics.segmentTimeSeriesByMaximizingSchwarzCriterion(values, 2.5, 500) on two inline
 * time series (d1: 3700 values, d2: 2310 values) and checks the change points against
 * fixed expected arrays. The JS runs each call in a Web Worker through AsyncTask; the C
 * analogue runs the same two calls on the calling thread, since there is no worker.
 *
 * The algorithm is copied step for step: per 500-value grid, a Schwarz-criterion
 * penalised dynamic program over segment counts k = 1..min(50, n) (with the same
 * "maxK = min(maxK, k + 3)" early stop), a triangular matrix of segment costs, and the
 * same Float32 storage for the cost matrix and DP table that the JS uses (Float32Array).
 * The only libm calls are sqrt, log, log1p. No Unicode tables are involved.
 *
 * Inputs: this.d1 and this.d2 are parsed at startup from worker/segmentation.js (untimed).
 * Usage: segmentation [jetstream-root] [iterations]
 *   jetstream-root defaults to ../jetstream; iterations defaults to 36. */
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#define UNDEF (-2)          /* JS "undefined" in previousNode */
#define GRID_LENGTH 500
#define SEGMENT_COUNT_WEIGHT 2.5

typedef struct { double *v; int n; } Series;

static double now_ms(void) {
  struct timespec t;
  clock_gettime(CLOCK_MONOTONIC, &t);
  return t.tv_sec * 1e3 + t.tv_nsec / 1e6;
}

/* Parse "<key> = [a, b, ...]" from the JS source text. */
static Series parse_series(const char *src, const char *key) {
  Series s = {NULL, 0};
  const char *p = strstr(src, key);
  if (!p) { fprintf(stderr, "segmentation: %s not found\n", key); exit(2); }
  p += strlen(key);
  int cap = 1024;
  s.v = malloc(cap * sizeof *s.v);
  for (;;) {
    while (*p == ' ' || *p == ',' || *p == '\n' || *p == '\r' || *p == '\t') p++;
    if (*p == ']' || *p == '\0') break;
    char *end;
    double x = strtod(p, &end);
    if (end == p) { fprintf(stderr, "segmentation: bad number in %s\n", key); exit(2); }
    if (s.n == cap) { cap *= 2; s.v = realloc(s.v, cap * sizeof *s.v); }
    s.v[s.n++] = x;
    p = end;
  }
  return s;
}

/* Statistics.sampleStandardDeviation */
static double sample_stdev(double n, double sum, double squareSum) {
  if (n < 2) return 0;
  return sqrt(squareSum / (n - 1) - sum * sum / (n - 1) / n);
}

/* SampleVarianceUpperTriangularMatrix: row i holds costBetween(i, i+1+j) at index j. */
typedef struct { int n; float **row; } CostMatrix;

static void cost_matrix_build(CostMatrix *m, const double *values, int n) {
  m->n = n;
  m->row = malloc((n > 1 ? n - 1 : 1) * sizeof *m->row);
  for (int i = 0; i < n - 1; i++) {
    int remaining = n - i - 1;
    m->row[i] = malloc(remaining * sizeof(float));
    double sum = values[i];
    double squareSum = sum * sum;
    m->row[i][0] = 0;
    for (int j = i + 1; j < n; j++) {
      double cur = values[j];
      sum += cur;
      squareSum += cur * cur;
      int size = j - i + 1;
      double stdev = sample_stdev(size, sum, squareSum);
      m->row[i][j - i - 1] = stdev > 0 ? (float)(size * log(stdev * stdev)) : 0;
    }
  }
}

static void cost_matrix_free(CostMatrix *m) {
  for (int i = 0; i < m->n - 1; i++) free(m->row[i]);
  free(m->row);
}

static double cost_between(const CostMatrix *m, int from, int to) {
  if (from >= m->n - 1 || from == to) return 0;
  return m->row[from][to - from - 1];
}

/* findOptimalSegmentation: returns the optimal K-segment boundaries in seg[0..K]
 * (seg[K] = n) and the DP cost of the optimal segmentation. */
static double find_optimal_segmentation(int n, const CostMatrix *m, int K, int *seg) {
  int W = K + 1;
  float *cost = calloc((size_t)n * W, sizeof(float));
  int *prev = malloc((size_t)n * W * sizeof(int));
  for (size_t i = 0; i < (size_t)n * W; i++) prev[i] = UNDEF;
  prev[0 * W + 0] = -1; /* previousNode[0] = [-1] */

  for (int s = 0; s < n; s++) {
    for (int k = 0; k < K; k++) {
      if (prev[(size_t)s * W + k] == UNDEF) continue;
      for (int e = s + 1; e < n; e++) {
        double total = (double)cost[(size_t)s * W + k] + cost_between(m, s, e);
        size_t idx = (size_t)e * W + (k + 1);
        if (prev[idx] == UNDEF || total < cost[idx]) {
          cost[idx] = (float)total;
          prev[idx] = s;
        }
      }
    }
  }

  int e = n - 1;
  seg[K] = n;
  for (int k = K; k > 0; k--) {
    int start = prev[(size_t)e * W + k];
    if (start == UNDEF) { fprintf(stderr, "segmentation: no segmentation with %d segments\n", K); exit(3); }
    e = start;
    seg[k - 1] = e;
  }
  double result = cost[(size_t)(n - 1) * W + K];
  free(cost);
  free(prev);
  return result;
}

/* splitIntoSegmentsUntilGoodEnough: writes the boundaries into out (capacity >= n + 1)
 * and returns how many boundaries were written. */
static int split_into_segments(const double *values, int n, double C, int *out) {
  if (n < 2) {
    out[0] = 0;
    out[1] = n;
    return 2;
  }
  CostMatrix m;
  cost_matrix_build(&m, values, n);
  double beta = log1p(n - 1) / n;
  int maxK = n < 50 ? n : 50;
  double minTotal = INFINITY;
  int best_k = 0;
  int *seg = malloc((n + 1) * sizeof(int));
  int *best = malloc((n + 1) * sizeof(int));
  for (int k = 1; k < maxK; k++) {
    double cost = find_optimal_segmentation(n, &m, k, seg) / n;
    double penalty = beta * (k * (1 + C * log1p((double)n / k - 1)));
    if (cost + penalty < minTotal) {
      minTotal = cost + penalty;
      memcpy(best, seg, (k + 1) * sizeof(int));
      best_k = k;
    } else {
      int lim = k + 3;
      if (lim < maxK) maxK = lim;
    }
  }
  memcpy(out, best, (best_k + 1) * sizeof(int));
  free(seg);
  free(best);
  cost_matrix_free(&m);
  return best_k + 1;
}

/* segmentTimeSeriesByMaximizingSchwarzCriterion(values, segmentCountWeight, gridSize).
 * Writes the change points into out and returns their count (the result array length). */
static int segment_time_series(const double *values, int n, double weight, int gridLength, int *out) {
  int total = 0;
  out[total++] = 0;
  int *segmentation = malloc((gridLength + 2) * sizeof(int));
  int grids = (n + gridLength - 1) / gridLength;
  for (int g = 0; g < grids; g++) {
    int start = g * gridLength;
    int len = n - start < gridLength ? n - start : gridLength;
    int cnt = split_into_segments(values + start, len, weight, segmentation);
    for (int i = 1; i < cnt - 1; i++) out[total++] = start + segmentation[i];
  }
  free(segmentation);
  out[total++] = n;
  return total;
}

static const int expected0[] = {0, 97, 174, 211, 301, 1270, 1278, 2696, 3613, 3700};
static const int expected1[] = {0, 175, 1194, 1237, 2310};

/* runIteration: the two segmentations, each compared with its expected result. */
static int run_iteration(const Series *d1, const Series *d2, int *out) {
  const Series *data[2] = {d1, d2};
  const int *expected[2] = {expected0, expected1};
  const int expectedLen[2] = {10, 5};
  for (int i = 0; i < 2; i++) {
    int cnt = segment_time_series(data[i]->v, data[i]->n, SEGMENT_COUNT_WEIGHT, GRID_LENGTH, out);
    if (cnt != expectedLen[i] || memcmp(out, expected[i], cnt * sizeof(int)) != 0) return 0;
  }
  return 1;
}

int main(int argc, char **argv) {
  const char *root = argc > 2 ? argv[2] : "../jetstream";
  int iterations = argc > 1 ? atoi(argv[1]) : 36;
  char path[4096];
  snprintf(path, sizeof path, "%s/worker/segmentation.js", root);
  FILE *f = fopen(path, "rb");
  if (!f) { perror(path); return 2; }
  fseek(f, 0, SEEK_END);
  long size = ftell(f);
  fseek(f, 0, SEEK_SET);
  char *src = malloc(size + 1);
  if (fread(src, 1, size, f) != (size_t)size) { fprintf(stderr, "read failed\n"); return 2; }
  src[size] = '\0';
  fclose(f);

  Series d1 = parse_series(src, "this.d1 = [");
  Series d2 = parse_series(src, "this.d2 = [");
  free(src);
  if (d1.n != 3700 || d2.n != 2310) { fprintf(stderr, "unexpected input lengths\n"); return 2; }

  int *out = malloc((d1.n + 2) * sizeof(int));
  int ok = 1;
  double t0 = now_ms();
  for (int i = 0; i < iterations; i++) {
    if (!run_iteration(&d1, &d2, out)) { ok = 0; break; }
  }
  double elapsed = now_ms() - t0;
  printf("segmentation %d iterations %.3f ms\n", iterations, elapsed);
  printf("validation: %s (d1 -> 10 change points, d2 -> 5 change points)\n", ok ? "ok" : "FAILED");
  free(out);
  free(d1.v);
  free(d2.v);
  return ok ? 0 : 1;
}
