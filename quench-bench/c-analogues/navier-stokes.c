/* C analogue of JetStream's Octane/navier-stokes.js: the same solver over double arrays
 * sized at run time, with the same frame callback, point schedule and checksum. */
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <time.h>

static int width, height, rowSize, size, iterations = 10;
static double dt = 0.1;
static double *dens, *dens_prev, *u, *u_prev, *v, *v_prev;
static int framesTillAddingPoints = 0, framesBetweenAddingPoints = 5, nsFrameCounter = 0;

static void addFields(double *x, const double *s, double dt) { for (int i = 0; i < size; i++) x[i] += dt * s[i]; }
static void set_bnd(int b, double *x) {
  for (int i = 1; i <= width; i++) {
    x[i] = b == 2 ? -x[i + rowSize] : x[i + rowSize];
    x[i + (height + 1) * rowSize] = b == 2 ? -x[i + height * rowSize] : x[i + height * rowSize];
  }
  for (int j = 1; j <= height; j++) {
    x[j * rowSize] = b == 1 ? -x[1 + j * rowSize] : x[1 + j * rowSize];
    x[(width + 1) + j * rowSize] = b == 1 ? -x[width + j * rowSize] : x[width + j * rowSize];
  }
  int maxEdge = (height + 1) * rowSize;
  x[0] = 0.5 * (x[1] + x[rowSize]);
  x[maxEdge] = 0.5 * (x[1 + maxEdge] + x[height * rowSize]);
  x[width + 1] = 0.5 * (x[width] + x[(width + 1) + rowSize]);
  x[(width + 1) + maxEdge] = 0.5 * (x[width + maxEdge] + x[(width + 1) + height * rowSize]);
}
static void lin_solve(int b, double *x, const double *x0, double a, double c) {
  if (a == 0 && c == 1) {
    for (int j = 1; j <= height; j++) { int r = j * rowSize + 1; for (int i = 0; i < width; i++, r++) x[r] = x0[r]; }
    set_bnd(b, x);
    return;
  }
  double invC = 1 / c;
  for (int k = 0; k < iterations; k++) {
    for (int j = 1; j <= height; j++) {
      int lastRow = (j - 1) * rowSize, currentRow = j * rowSize, nextRow = (j + 1) * rowSize;
      double lastX = x[currentRow];
      ++currentRow;
      for (int i = 1; i <= width; i++) {
        double t = x0[currentRow]; int cr = currentRow;
        lastX = x[cr] = (t + a * (lastX + x[++currentRow] + x[++lastRow] + x[++nextRow])) * invC;
      }
    }
    set_bnd(b, x);
  }
}
static void lin_solve2(double *x, const double *x0, double *y, const double *y0, double a, double c) {
  if (a == 0 && c == 1) {
    for (int j = 1; j <= height; j++) { int r = j * rowSize + 1; for (int i = 0; i < width; i++, r++) { x[r] = x0[r]; y[r] = y0[r]; } }
    set_bnd(1, x); set_bnd(2, y);
    return;
  }
  double invC = 1 / c;
  for (int k = 0; k < iterations; k++) {
    for (int j = 1; j <= height; j++) {
      int lastRow = (j - 1) * rowSize, currentRow = j * rowSize, nextRow = (j + 1) * rowSize;
      double lastX = x[currentRow], lastY = y[currentRow];
      ++currentRow;
      for (int i = 1; i <= width; i++) {
        lastX = x[currentRow] = (x0[currentRow] + a * (lastX + x[currentRow] + x[lastRow] + x[nextRow])) * invC;
        int cr = currentRow;
        lastY = y[cr] = (y0[cr] + a * (lastY + y[++currentRow] + y[++lastRow] + y[++nextRow])) * invC;
      }
    }
    set_bnd(1, x); set_bnd(2, y);
  }
}
static void advect(int b, double *d, const double *d0, const double *u, const double *v, double dt) {
  double Wdt0 = dt * width, Hdt0 = dt * height, Wp5 = width + 0.5, Hp5 = height + 0.5;
  for (int j = 1; j <= height; j++) {
    int pos = j * rowSize;
    for (int i = 1; i <= width; i++) {
      double x = i - Wdt0 * u[++pos], y = j - Hdt0 * v[pos];
      if (x < 0.5) x = 0.5; else if (x > Wp5) x = Wp5;
      int i0 = (int)x, i1 = i0 + 1;
      if (y < 0.5) y = 0.5; else if (y > Hp5) y = Hp5;
      int j0 = (int)y, j1 = j0 + 1;
      double s1 = x - i0, s0 = 1 - s1, t1 = y - j0, t0 = 1 - t1;
      int row1 = j0 * rowSize, row2 = j1 * rowSize;
      d[pos] = s0 * (t0 * d0[i0 + row1] + t1 * d0[i0 + row2]) + s1 * (t0 * d0[i1 + row1] + t1 * d0[i1 + row2]);
    }
  }
  set_bnd(b, d);
}
static void project(double *u, double *v, double *p, double *div) {
  double h = -0.5 / sqrt(width * height);
  for (int j = 1; j <= height; j++) {
    int row = j * rowSize, previousRow = (j - 1) * rowSize, prevValue = row - 1, currentRow = row, nextValue = row + 1, nextRow = (j + 1) * rowSize;
    for (int i = 1; i <= width; i++) {
      div[++currentRow] = h * (u[++nextValue] - u[++prevValue] + v[++nextRow] - v[++previousRow]);
      p[currentRow] = 0;
    }
  }
  set_bnd(0, div); set_bnd(0, p);
  lin_solve(0, p, div, 1, 4);
  double wScale = 0.5 * width, hScale = 0.5 * height;
  for (int j = 1; j <= height; j++) {
    int prevPos = j * rowSize - 1, currentPos = j * rowSize, nextPos = j * rowSize + 1, prevRow = (j - 1) * rowSize, nextRow = (j + 1) * rowSize;
    for (int i = 1; i <= width; i++) {
      u[++currentPos] -= wScale * (p[++nextPos] - p[++prevPos]);
      v[currentPos] -= hScale * (p[++nextRow] - p[++prevRow]);
    }
  }
  set_bnd(1, u); set_bnd(2, v);
}
static void vel_step(double *u, double *v, double *u0, double *v0, double dt) {
  addFields(u, u0, dt); addFields(v, v0, dt);
  double *t = u0; u0 = u; u = t; t = v0; v0 = v; v = t;
  lin_solve2(u, u0, v, v0, 0, 1);
  project(u, v, u0, v0);
  t = u0; u0 = u; u = t; t = v0; v0 = v; v = t;
  advect(1, u, u0, u0, v0, dt);
  advect(2, v, v0, u0, v0, dt);
  project(u, v, u0, v0);
}
static void dens_step(double *x, double *x0, double *u, double *v, double dt) {
  addFields(x, x0, dt);
  lin_solve(0, x0, x, 0, 1);
  advect(0, x, x0, u, v, dt);
}
static void setVelocity(double *uf, double *vf, int x, int y, double xv, double yv) { uf[(x + 1) + (y + 1) * rowSize] = xv; vf[(x + 1) + (y + 1) * rowSize] = yv; }
static void setDensity(double *d, int x, int y, double value) { d[(x + 1) + (y + 1) * rowSize] = value; }
static void prepareFrame(double *d, double *uf, double *vf) {
  if (framesTillAddingPoints == 0) {
    int n = 64;
    for (int i = 1; i <= n; i++) {
      setVelocity(uf, vf, i, i, n, n); setDensity(d, i, i, 5);
      setVelocity(uf, vf, i, n - i, -n, -n); setDensity(d, i, n - i, 20);
      setVelocity(uf, vf, 128 - i, n + i, -n, -n); setDensity(d, 128 - i, n + i, 30);
    }
    framesTillAddingPoints = framesBetweenAddingPoints++;
  } else framesTillAddingPoints--;
}
static void reset(void) {
  rowSize = width + 2; size = (width + 2) * (height + 2);
  double **fields[] = {&dens, &dens_prev, &u, &u_prev, &v, &v_prev};
  for (int f = 0; f < 6; f++) { free(*fields[f]); *fields[f] = calloc(size, sizeof(double)); }
}
static void update(void) {
  for (int i = 0; i < size; i++) u_prev[i] = v_prev[i] = dens_prev[i] = 0.0;
  prepareFrame(dens_prev, u_prev, v_prev);
  vel_step(u, v, u_prev, v_prev, dt);
  dens_step(dens, dens_prev, u, v, dt);
}
int main(int argc, char **argv) {
  int frames = argc > 1 ? atoi(argv[1]) : 120;
  width = 128; height = 128; reset(); iterations = 20;
  struct timespec a, b;
  clock_gettime(CLOCK_MONOTONIC, &a);
  for (int f = 0; f < frames; f++) {
    update();
    if (++nsFrameCounter == 15) {
      int result = 0;
      for (int i = 7000; i < 7100; i++) result += (int)(dens[i] * 10);
      if (result != 77) { fprintf(stderr, "checksum failed %d\n", result); return 1; }
    }
  }
  clock_gettime(CLOCK_MONOTONIC, &b);
  printf("navier-stokes %d iterations %.3f ms\n", frames, (b.tv_sec - a.tv_sec) * 1e3 + (b.tv_nsec - a.tv_nsec) / 1e6);
  return 0;
}
