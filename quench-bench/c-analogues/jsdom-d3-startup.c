// jetstream-row: jsdom-d3-startup
// jetstream-files: utils/StartupBenchmark.js jsdom-d3-startup/benchmark.js
// iterations: 15
// jetstream-driver: class=Async preload=BUNDLE=jsdom-d3-startup/dist/bundle.min.js,US_DATA=jsdom-d3-startup/data/counties-albers-10m.json,AIRPORTS=jsdom-d3-startup/data/airports.csv
/* C analogue of the JetStream jsdom-d3-startup row (jsdom-d3-startup/dist/bundle.min.js and benchmark.js).
 *
 * What the JS does per iteration (StartupBenchmark + benchmark.js):
 *   1. eval the iteration's copy of the bundle (the cache-busting comments replaced, two iterations per
 *      copy). The bundle is a webpack build of src/test.mjs with jsdom, d3 and topojson-client. Copies are
 *      made in init, outside the timed part, as in the JS.
 *   2. D3Test.runTest(airportsCsv, usData): csvParse with autoType over the airports CSV, a jsdom
 *      document, then the scene: the US state mesh (topojson.mesh with a !== filter), a Delaunay
 *      triangulation and Voronoi diagram of the airports (x = longitude, y = latitude, bounds 975 x 610)
 *      with one <path> per airport and a <title>, the state outlines (topojson.feature), and the airport
 *      labels (a <text> with two <tspan>s each). Output is body.html().
 *   3. quickHash(html), timed as part of the iteration.
 * usData is parsed once in init (JSON.parse), not per iteration.
 *
 * What the C does, with the same algorithms and arithmetic:
 *   - the bundle copy is parsed with js-parser-analogue.h (the compile work of eval);
 *   - the airports CSV is parsed with RFC 4180 quoting and d3's autoType rules (numbers, null for empty,
 *     trimmed numeric fields, untrimmed strings);
 *   - topojson.feature and topojson.mesh are ported from topojson-client (arc decoding with the transform,
 *     extractArcs, stitch with its string-keyed fragment maps in insertion order);
 *   - geoPath with no projection and digits 3 (d3-geo's PathString: Math.round(v * 1000) / 1000 per
 *     coordinate, Z for polygon rings);
 *   - d3-delaunay: Delaunator (exact orient2d with the Shewchuk fallback, the same quicksort and hash) and
 *     Voronoi (circumcenters, infinite-cell clipping, renderCell with d3-delaunay's Path, which formats
 *     coordinates with JS Number::toString);
 *   - a DOM element tree (elements, attributes with setAttribute replace-or-append, text nodes with
 *     textContent semantics) and the HTML serializer's escaping, giving the same bytes as body.html();
 *   - quickHash over UTF-16 code units, as the JS does.
 *
 * Gaps, stated plainly: the bundle's top-level code is not executed (the webpack module graph of jsdom,
 * d3 and topojson is only parsed, not run), and the jsdom window (its interfaces and the document
 * machinery) is not created. The DOM is a plain tree, not jsdom's wrapper and impl objects. So the C does
 * the scene work and the parse, not the module initialisation or jsdom's per-call overhead.
 *
 * Validation (tools/jsdom-d3-startup-validate.cjs, run with node 22 on the real bundle): the output is
 * 691366 UTF-16 units with quickHash 144487595 (the row's own EXPECTED_LAST_RESULT_LENGTH and
 * EXPECTED_LAST_RESULT_HASH). Element counts: svg 1, path 1895 (9 with a d attribute, 1886 without),
 * g 2, title 1893, text 1893, tspan 3786. Path commands in the d attributes: M 264, L 13006, Z 205.
 * The C checks the length and hash every iteration, and the counts every iteration after the timer.
 *
 * Timing (15 iterations, cc -O2, this machine): the C takes about 600-670 ms, 40-45 ms per iteration, of which
 * about 32 ms is the bundle parse (js_parse of the 3.4 MB copy) and about 10 ms the scene. The composed row under
 * node 22 (compose_row.py --run) takes about 0.51 s per iteration (eval about 190 ms, runTest about 300 ms with
 * jsdom), so the C is about 12x faster. Most of that gap is the parse, and the C leaves out jsdom's window
 * creation and the bundle's module initialisation (see the gaps above).
 * Usage: jsdom-d3-startup [iterations] [jetstream-root]; the default is the row's 15 iterations. */
#define _DEFAULT_SOURCE
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
/* the shared headers define helpers this row does not call */
#pragma GCC diagnostic push
#pragma GCC diagnostic ignored "-Wunused-function"
#include "js-parser-analogue.h"
#include "json-analogue.h"
#pragma GCC diagnostic pop

#define WIDTH 975
#define HEIGHT 610
#define EXPECTED_LEN 691366
#define EXPECTED_HASH 144487595

/* ================= output buffer ================= */
typedef struct { char *p; size_t n, cap; } Out;

static void ob_reserve(Out *o, size_t k) {
  if (o->n + k + 1 <= o->cap) return;
  size_t c = o->cap ? o->cap : 1 << 16;
  while (c < o->n + k + 1) c *= 2;
  o->p = realloc(o->p, c);
  if (!o->p) die("out of memory");
  o->cap = c;
}
static void ob_put(Out *o, const char *s, size_t n) {
  ob_reserve(o, n);
  memcpy(o->p + o->n, s, n);
  o->n += n;
}
static void ob_str(Out *o, const char *s) { ob_put(o, s, strlen(s)); }
static void ob_chr(Out *o, char c) {
  ob_reserve(o, 1);
  o->p[o->n++] = c;
}
/* ToString of a double, as JS string concatenation does it. */
static void ob_num(Out *o, double x) {
  char b[32];
  size_t k = js_number_to_string(x, b);
  ob_put(o, b, k);
}
/* d3-geo's PathString with digits 3: Math.round(v * k) / k, Math.round rounds ties toward +Infinity. */
static void ob_num3(Out *o, double x) {
  const double k = 1000.0;
  double v = x * k;
  double f = floor(v);
  double r = (v - f >= 0.5) ? f + 1 : f;
  ob_num(o, r / k);
}
static void ob_free(Out *o) {
  free(o->p);
  o->p = NULL;
  o->n = o->cap = 0;
}

/* ================= Delaunator (delaunator 5, as bundled in d3-delaunay) ================= */
static double dist2(double ax, double ay, double bx, double by) {
  double dx = ax - bx, dy = ay - by;
  return dx * dx + dy * dy;
}
static double circumradius(double ax, double ay, double bx, double by, double cx, double cy) {
  double dx = bx - ax, dy = by - ay, ex = cx - ax, ey = cy - ay;
  double bl = dx * dx + dy * dy, cl = ex * ex + ey * ey;
  double d = 0.5 / (dx * ey - dy * ex);
  double x = (ey * bl - dy * cl) * d, y = (dx * cl - ex * bl) * d;
  return x * x + y * y;
}
static int in_circle(double ax, double ay, double bx, double by, double cx, double cy, double px, double py) {
  double dx = ax - px, dy = ay - py, ex = bx - px, ey = by - py, fx = cx - px, fy = cy - py;
  double bp = ex * ex + ey * ey, cp = fx * fx + fy * fy;
  return dx * (ey * cp - bp * fy) - dy * (ex * cp - bp * fx) + (dx * dx + dy * dy) * (ex * fy - ey * fx) < 0;
}
/* Shewchuk's two-product (Dekker split), as delaunator's orient2d uses it. */
static void two_product(double x, double y, double *hi, double *lo) {
  double s1 = x * y;
  double c = 134217729.0 * x, ahi = c - (c - x), alo = x - ahi;
  c = 134217729.0 * y;
  double bhi = c - (c - y), blo = y - bhi;
  *hi = s1;
  *lo = alo * blo - (s1 - ahi * bhi - alo * bhi - ahi * blo);
}
/* (s1 + s0) - (t1 + t0) as a four-component expansion, smallest first. */
static void two_two_diff(double s1, double s0, double t1, double t0, double *B) {
  double _i = s0 - t0, bvirt = s0 - _i;
  B[0] = s0 - (_i + bvirt) + (bvirt - t0);
  double _j = s1 + _i;
  bvirt = _j - s1;
  double _0 = s1 - (_j - bvirt) + (_i - bvirt);
  _i = _0 - t1;
  bvirt = _0 - _i;
  B[1] = _0 - (_i + bvirt) + (bvirt - t1);
  double u3 = _j + _i;
  bvirt = u3 - _j;
  B[2] = _j - (u3 - bvirt) + (_i - bvirt);
  B[3] = u3;
}
static double expansion_at(const double *e, size_t len, size_t i) { return i < len ? e[i] : 0.0; }
/* util_sum of delaunator: adds two expansions, returns the length of h. */
static size_t util_sum(size_t elen, const double *e, size_t flen, const double *f, double *h) {
  double Q, Qnew, hh, bvirt;
  size_t eindex = 0, findex = 0, hindex = 0;
  double enow = expansion_at(e, elen, 0), fnow = expansion_at(f, flen, 0);
  if ((fnow > enow) == (fnow > -enow)) { Q = enow; enow = expansion_at(e, elen, ++eindex); }
  else { Q = fnow; fnow = expansion_at(f, flen, ++findex); }
  if (eindex < elen && findex < flen) {
    if ((fnow > enow) == (fnow > -enow)) {
      Qnew = enow + Q; hh = Q - (Qnew - enow); enow = expansion_at(e, elen, ++eindex);
    } else {
      Qnew = fnow + Q; hh = Q - (Qnew - fnow); fnow = expansion_at(f, flen, ++findex);
    }
    Q = Qnew;
    if (hh != 0) h[hindex++] = hh;
    while (eindex < elen && findex < flen) {
      if ((fnow > enow) == (fnow > -enow)) {
        Qnew = Q + enow; bvirt = Qnew - Q; hh = Q - (Qnew - bvirt) + (enow - bvirt);
        enow = expansion_at(e, elen, ++eindex);
      } else {
        Qnew = Q + fnow; bvirt = Qnew - Q; hh = Q - (Qnew - bvirt) + (fnow - bvirt);
        fnow = expansion_at(f, flen, ++findex);
      }
      Q = Qnew;
      if (hh != 0) h[hindex++] = hh;
    }
  }
  while (eindex < elen) {
    Qnew = Q + enow; bvirt = Qnew - Q; hh = Q - (Qnew - bvirt) + (enow - bvirt);
    enow = expansion_at(e, elen, ++eindex);
    Q = Qnew;
    if (hh != 0) h[hindex++] = hh;
  }
  while (findex < flen) {
    Qnew = Q + fnow; bvirt = Qnew - Q; hh = Q - (Qnew - bvirt) + (fnow - bvirt);
    fnow = expansion_at(f, flen, ++findex);
    Q = Qnew;
    if (hh != 0) h[hindex++] = hh;
  }
  if (!(Q == 0 && hindex != 0)) h[hindex++] = Q;
  return hindex;
}
/* The exact fallback of orient2d (adaptive, Shewchuk). */
static double orient2d_adapt(double ax, double ay, double bx, double by, double cx, double cy, double detsum) {
  double acx = ax - cx, bcx = bx - cx, acy = ay - cy, bcy = by - cy;
  double s1, s0, t1, t0, B[4];
  two_product(acx, bcy, &s1, &s0);
  two_product(acy, bcx, &t1, &t0);
  two_two_diff(s1, s0, t1, t0, B);
  double det = B[0] + B[1] + B[2] + B[3];
  double errbound = 22204460492503146e-32 * detsum;
  if (det >= errbound || -det >= errbound) return det;
  double bvirt = ax - acx;
  double acxtail = ax - (acx + bvirt) + (bvirt - cx);
  bvirt = bx - bcx;
  double bcxtail = bx - (bcx + bvirt) + (bvirt - cx);
  bvirt = ay - acy;
  double acytail = ay - (acy + bvirt) + (bvirt - cy);
  bvirt = by - bcy;
  double bcytail = by - (bcy + bvirt) + (bvirt - cy);
  if (acxtail == 0 && acytail == 0 && bcxtail == 0 && bcytail == 0) return det;
  errbound = 11093356479670487e-47 * detsum + 33306690738754706e-32 * fabs(det);
  det += acx * bcytail + bcy * acxtail - (acy * bcxtail + bcx * acytail);
  if (det >= errbound || -det >= errbound) return det;
  double U[4], C1[8], C2[12], D[16];
  two_product(acxtail, bcy, &s1, &s0);
  two_product(acytail, bcx, &t1, &t0);
  two_two_diff(s1, s0, t1, t0, U);
  size_t C1len = util_sum(4, B, 4, U, C1);
  two_product(acx, bcytail, &s1, &s0);
  two_product(acy, bcxtail, &t1, &t0);
  two_two_diff(s1, s0, t1, t0, U);
  size_t C2len = util_sum(C1len, C1, 4, U, C2);
  two_product(acxtail, bcytail, &s1, &s0);
  two_product(acytail, bcxtail, &t1, &t0);
  two_two_diff(s1, s0, t1, t0, U);
  size_t Dlen = util_sum(C2len, C2, 4, U, D);
  return D[Dlen - 1];
}
static double orient2d(double ax, double ay, double bx, double by, double cx, double cy) {
  double detleft = (ay - cy) * (bx - cx), detright = (ax - cx) * (by - cy);
  double det = detleft - detright, detsum = fabs(detleft + detright);
  if (fabs(det) >= 33306690738754716e-32 * detsum) return det;
  return -orient2d_adapt(ax, ay, bx, by, cx, cy, detsum);
}

static void swap_id(uint32_t *a, long i, long j) {
  uint32_t t = a[i];
  a[i] = a[j];
  a[j] = t;
}
/* delaunator's quicksort on ids by dists (a hybrid of insertion sort and Hoare partition). */
static void quicksort(uint32_t *ids, const double *dists, long left, long right) {
  if (right - left <= 20) {
    for (long i = left + 1; i <= right; i++) {
      uint32_t temp = ids[i];
      double tempDist = dists[temp];
      long j = i - 1;
      while (j >= left && dists[ids[j]] > tempDist) {
        ids[j + 1] = ids[j];
        j--;
      }
      ids[j + 1] = temp;
    }
    return;
  }
  long i = left + 1, j = right;
  swap_id(ids, (left + right) >> 1, i);
  if (dists[ids[left]] > dists[ids[right]]) swap_id(ids, left, right);
  if (dists[ids[i]] > dists[ids[right]]) swap_id(ids, i, right);
  if (dists[ids[left]] > dists[ids[i]]) swap_id(ids, left, i);
  uint32_t temp = ids[i];
  double tempDist = dists[temp];
  for (;;) {
    do { i++; } while (dists[ids[i]] < tempDist);
    do { j--; } while (dists[ids[j]] > tempDist);
    if (j < i) break;
    swap_id(ids, i, j);
  }
  ids[left + 1] = ids[j];
  ids[j] = temp;
  if (right - i + 1 >= j - left) {
    quicksort(ids, dists, i, right);
    quicksort(ids, dists, left, j - 1);
  } else {
    quicksort(ids, dists, left, j - 1);
    quicksort(ids, dists, i, right);
  }
}

typedef struct {
  size_t n;                /* number of points */
  double *coords;          /* 2n */
  size_t hashSize;
  uint32_t *hullPrev, *hullNext, *hullTri, *ids;
  int32_t *hullHash;
  double *dists;
  double cx, cy;
  uint32_t hullStart;
  uint32_t *triangles;     /* maxTriangles * 3 storage, trianglesLen used */
  int32_t *halfedges;
  size_t trianglesLen;
  uint32_t *hull;
  size_t hullSize;
  uint32_t edgeStack[512];
} Delaunator;

static void dl_link(Delaunator *d, size_t a, int32_t b) {
  d->halfedges[a] = b;
  if (b != -1) d->halfedges[b] = (int32_t)a;
}
static size_t dl_add_triangle(Delaunator *d, uint32_t i0, uint32_t i1, uint32_t i2, int32_t a, int32_t b, int32_t c) {
  size_t t = d->trianglesLen;
  d->triangles[t] = i0;
  d->triangles[t + 1] = i1;
  d->triangles[t + 2] = i2;
  dl_link(d, t, a);
  dl_link(d, t + 1, b);
  dl_link(d, t + 2, c);
  d->trianglesLen += 3;
  return t;
}
static int64_t dl_hash_key(const Delaunator *d, double x, double y) {
  double dx = x - d->cx, dy = y - d->cy;
  double p = dx / (fabs(dx) + fabs(dy));
  double a = (dy > 0 ? 3 - p : 1 + p) / 4;
  if (a != a) die("jsdom-d3-startup: NaN hash key (point at the centre)");
  return (int64_t)floor(a * (double)d->hashSize) % (int64_t)d->hashSize;
}
static size_t dl_legalize(Delaunator *d, size_t a) {
  size_t i = 0, ar = 0;
  const double *c = d->coords;
  for (;;) {
    int32_t b = d->halfedges[a];
    size_t a0 = a - a % 3;
    ar = a0 + (a + 2) % 3;
    if (b == -1) {
      if (i == 0) break;
      a = d->edgeStack[--i];
      continue;
    }
    size_t b0 = (size_t)b - (size_t)b % 3;
    size_t al = a0 + (a + 1) % 3, bl = b0 + ((size_t)b + 2) % 3;
    uint32_t p0 = d->triangles[ar], pr = d->triangles[a], pl = d->triangles[al], p1 = d->triangles[bl];
    if (in_circle(c[2 * p0], c[2 * p0 + 1], c[2 * pr], c[2 * pr + 1], c[2 * pl], c[2 * pl + 1], c[2 * p1], c[2 * p1 + 1])) {
      d->triangles[a] = p1;
      d->triangles[b] = (int32_t)p0;
      int32_t hbl = d->halfedges[bl];
      if (hbl == -1) {
        uint32_t e = d->hullStart;
        do {
          if (d->hullTri[e] == bl) {
            d->hullTri[e] = (uint32_t)a;
            break;
          }
          e = d->hullPrev[e];
        } while (e != d->hullStart);
      }
      dl_link(d, a, hbl);
      dl_link(d, b, d->halfedges[ar]);
      dl_link(d, ar, (int32_t)bl);
      size_t br = b0 + ((size_t)b + 1) % 3;
      if (i < 512) d->edgeStack[i++] = (uint32_t)br;
    } else {
      if (i == 0) break;
      a = d->edgeStack[--i];
    }
  }
  return ar;
}

static void dl_update(Delaunator *d) {
  const double *coords = d->coords;
  size_t n = d->n;
  double minX = INFINITY, minY = INFINITY, maxX = -INFINITY, maxY = -INFINITY;
  for (size_t i = 0; i < n; i++) {
    double x = coords[2 * i], y = coords[2 * i + 1];
    if (x < minX) minX = x;
    if (y < minY) minY = y;
    if (x > maxX) maxX = x;
    if (y > maxY) maxY = y;
    d->ids[i] = (uint32_t)i;
  }
  double cx = (minX + maxX) / 2, cy = (minY + maxY) / 2;
  size_t i0 = 0, i1 = 0, i2 = 0;
  double minDist = INFINITY;
  for (size_t i = 0; i < n; i++) {
    double dd = dist2(cx, cy, coords[2 * i], coords[2 * i + 1]);
    if (dd < minDist) { i0 = i; minDist = dd; }
  }
  double i0x = coords[2 * i0], i0y = coords[2 * i0 + 1];
  minDist = INFINITY;
  for (size_t i = 0; i < n; i++) {
    if (i == i0) continue;
    double dd = dist2(i0x, i0y, coords[2 * i], coords[2 * i + 1]);
    if (dd < minDist && dd > 0) { i1 = i; minDist = dd; }
  }
  double i1x = coords[2 * i1], i1y = coords[2 * i1 + 1];
  double minRadius = INFINITY;
  for (size_t i = 0; i < n; i++) {
    if (i == i0 || i == i1) continue;
    double r = circumradius(i0x, i0y, i1x, i1y, coords[2 * i], coords[2 * i + 1]);
    if (r < minRadius) { i2 = i; minRadius = r; }
  }
  if (minRadius == INFINITY) die("jsdom-d3-startup: degenerate (collinear) airport set is not supported");
  double i2x = coords[2 * i2], i2y = coords[2 * i2 + 1];
  if (orient2d(i0x, i0y, i1x, i1y, i2x, i2y) < 0) {
    size_t i = i1;
    double x = i1x, y = i1y;
    i1 = i2; i1x = i2x; i1y = i2y;
    i2 = i; i2x = x; i2y = y;
  }
  /* circumcenter of the seed triangle */
  {
    double dx = i1x - i0x, dy = i1y - i0y, ex = i2x - i0x, ey = i2y - i0y;
    double bl = dx * dx + dy * dy, cl = ex * ex + ey * ey, dd = 0.5 / (dx * ey - dy * ex);
    d->cx = i0x + (ey * bl - dy * cl) * dd;
    d->cy = i0y + (dx * cl - ex * bl) * dd;
  }
  for (size_t i = 0; i < n; i++) d->dists[i] = dist2(coords[2 * i], coords[2 * i + 1], d->cx, d->cy);
  quicksort(d->ids, d->dists, 0, (long)n - 1);
  d->hullStart = (uint32_t)i0;
  size_t hullSize = 3;
  d->hullNext[i0] = d->hullPrev[i2] = (uint32_t)i1;
  d->hullNext[i1] = d->hullPrev[i0] = (uint32_t)i2;
  d->hullNext[i2] = d->hullPrev[i1] = (uint32_t)i0;
  d->hullTri[i0] = 0;
  d->hullTri[i1] = 1;
  d->hullTri[i2] = 2;
  for (size_t h = 0; h < d->hashSize; h++) d->hullHash[h] = -1;
  d->hullHash[dl_hash_key(d, i0x, i0y)] = (int32_t)i0;
  d->hullHash[dl_hash_key(d, i1x, i1y)] = (int32_t)i1;
  d->hullHash[dl_hash_key(d, i2x, i2y)] = (int32_t)i2;
  d->trianglesLen = 0;
  dl_add_triangle(d, (uint32_t)i0, (uint32_t)i1, (uint32_t)i2, -1, -1, -1);
  double xp = 0, yp = 0;
  for (size_t k = 0; k < n; k++) {
    size_t i = d->ids[k];
    double x = coords[2 * i], y = coords[2 * i + 1];
    if (k > 0 && fabs(x - xp) <= 2.220446049250313e-16 && fabs(y - yp) <= 2.220446049250313e-16) continue;
    xp = x;
    yp = y;
    if (i == i0 || i == i1 || i == i2) continue;
    size_t start = 0;
    {
      int64_t key = dl_hash_key(d, x, y);
      for (size_t j = 0; j < d->hashSize; j++) {
        int32_t s = d->hullHash[(key + (int64_t)j) % (int64_t)d->hashSize];
        start = (size_t)(int64_t)s;
        if (s == -1 || (uint32_t)s == d->hullNext[(size_t)s]) continue;
        break;
      }
    }
    start = d->hullPrev[start];
    size_t e = start;
    uint32_t q;
    for (;;) {
      q = d->hullNext[e];
      if (orient2d(x, y, coords[2 * e], coords[2 * e + 1], coords[2 * q], coords[2 * q + 1]) < 0) break;
      e = q;
      if (e == start) { e = (size_t)-1; break; }
    }
    if (e == (size_t)-1) continue;
    size_t t = dl_add_triangle(d, (uint32_t)e, (uint32_t)i, d->hullNext[e], -1, -1, (int32_t)d->hullTri[e]);
    d->hullTri[i] = (uint32_t)dl_legalize(d, t + 2);
    d->hullTri[e] = (uint32_t)t;
    hullSize++;
    size_t nn = d->hullNext[e];
    for (;;) {
      q = d->hullNext[nn];
      if (!(orient2d(x, y, coords[2 * nn], coords[2 * nn + 1], coords[2 * q], coords[2 * q + 1]) < 0)) break;
      t = dl_add_triangle(d, (uint32_t)nn, (uint32_t)i, q, (int32_t)d->hullTri[i], -1, (int32_t)d->hullTri[nn]);
      d->hullTri[i] = (uint32_t)dl_legalize(d, t + 2);
      d->hullNext[nn] = (uint32_t)nn;
      hullSize--;
      nn = q;
    }
    if (e == start) {
      for (;;) {
        q = d->hullPrev[e];
        if (!(orient2d(x, y, coords[2 * q], coords[2 * q + 1], coords[2 * e], coords[2 * e + 1]) < 0)) break;
        t = dl_add_triangle(d, (uint32_t)q, (uint32_t)i, (uint32_t)e, -1, (int32_t)d->hullTri[e], (int32_t)d->hullTri[q]);
        dl_legalize(d, t + 2);
        d->hullTri[q] = (uint32_t)t;
        d->hullNext[e] = (uint32_t)e;
        hullSize--;
        e = q;
      }
    }
    d->hullStart = d->hullPrev[i] = (uint32_t)e;
    d->hullNext[e] = d->hullPrev[nn] = (uint32_t)i;
    d->hullNext[i] = (uint32_t)nn;
    d->hullHash[dl_hash_key(d, x, y)] = (int32_t)i;
    d->hullHash[dl_hash_key(d, coords[2 * e], coords[2 * e + 1])] = (int32_t)e;
  }
  d->hullSize = hullSize;
  for (size_t i = 0, e = d->hullStart; i < hullSize; i++) {
    d->hull[i] = (uint32_t)e;
    e = d->hullNext[e];
  }
}

/* ---- d3-delaunay's Delaunay (points, halfedges, inedges, hull index) ---- */
typedef struct {
  Delaunator dl;
  size_t n;
  int32_t *inedges, *hullIndex;
} Delaunay;

static void delaunay_init_arrays(Delaunay *D, const double *coords, size_t n) {
  Delaunator *d = &D->dl;
  memset(d, 0, sizeof *d);
  d->n = n;
  d->coords = (double *)coords;
  size_t maxTriangles = n >= 3 ? 2 * n - 5 : 0;
  d->triangles = malloc((3 * maxTriangles + 3) * sizeof(uint32_t));
  d->halfedges = malloc((3 * maxTriangles + 3) * sizeof(int32_t));
  d->hashSize = (size_t)ceil(sqrt((double)n));
  d->hullPrev = malloc(n * sizeof(uint32_t));
  d->hullNext = malloc(n * sizeof(uint32_t));
  d->hullTri = malloc(n * sizeof(uint32_t));
  d->hullHash = malloc(d->hashSize * sizeof(int32_t));
  d->ids = malloc(n * sizeof(uint32_t));
  d->dists = malloc(n * sizeof(double));
  d->hull = malloc(n * sizeof(uint32_t));
  D->inedges = malloc(n * sizeof(int32_t));
  D->hullIndex = malloc(n * sizeof(int32_t));
  if (!d->triangles || !d->halfedges || !d->hullPrev || !d->hullNext || !d->hullTri || !d->hullHash ||
      !d->ids || !d->dists || !d->hull || !D->inedges || !D->hullIndex)
    die("out of memory");
  D->n = n;
  dl_update(d);
}

static void delaunay_free(Delaunay *D) {
  Delaunator *d = &D->dl;
  free(d->triangles);
  free(d->halfedges);
  free(d->hullPrev);
  free(d->hullNext);
  free(d->hullTri);
  free(d->hullHash);
  free(d->ids);
  free(d->dists);
  free(d->hull);
  free(D->inedges);
  free(D->hullIndex);
}

/* Delaunay._init: inedges, hull index, and the collinear check (the jitter branch is not needed here). */
static void delaunay_init(Delaunay *D) {
  Delaunator *d = &D->dl;
  const double *points = d->coords;
  if (d->hullSize > 2) {
    int collinear = 1;
    for (size_t i = 0; i < d->trianglesLen; i += 3) {
      size_t a = 2 * d->triangles[i], b = 2 * d->triangles[i + 1], c = 2 * d->triangles[i + 2];
      if ((points[c] - points[a]) * (points[b + 1] - points[a + 1]) - (points[b] - points[a]) * (points[c + 1] - points[a + 1]) > 1e-10) {
        collinear = 0;
        break;
      }
    }
    if (collinear) die("jsdom-d3-startup: collinear airport set is not supported");
  }
  for (size_t i = 0; i < D->n; i++) { D->inedges[i] = -1; D->hullIndex[i] = -1; }
  for (size_t e = 0; e < d->trianglesLen; e++) {
    size_t p = d->triangles[e % 3 == 2 ? e - 2 : e + 1];
    if (d->halfedges[e] == -1 || D->inedges[p] == -1) D->inedges[p] = (int32_t)e;
  }
  for (size_t i = 0; i < d->hullSize; i++) D->hullIndex[d->hull[i]] = (int32_t)i;
  if (d->hullSize <= 2 && d->hullSize > 0) die("jsdom-d3-startup: hull of two or fewer points is not supported");
}

/* ---- d3-delaunay's Voronoi with bounds [xmin, ymin, xmax, ymax] ---- */
typedef struct { double *v; size_t n, cap; } Poly; /* n counts coordinates (x and y each count) */

static Poly *poly_new(void) {
  Poly *p = calloc(1, sizeof *p);
  if (!p) die("out of memory");
  return p;
}
static void poly_free(Poly *p) {
  if (!p) return;
  free(p->v);
  free(p);
}
static void poly_reserve(Poly *p, size_t k) {
  if (p->n + k <= p->cap) return;
  size_t c = p->cap ? p->cap * 2 : 16;
  while (c < p->n + k) c *= 2;
  p->v = realloc(p->v, c * sizeof(double));
  if (!p->v) die("out of memory");
  p->cap = c;
}
static void poly_push(Poly *p, double x, double y) {
  poly_reserve(p, 2);
  p->v[p->n++] = x;
  p->v[p->n++] = y;
}
/* splice(j, 0, x, y) */
static void poly_insert(Poly *p, size_t j, double x, double y) {
  poly_reserve(p, 2);
  memmove(p->v + j + 2, p->v + j, (p->n - j) * sizeof(double));
  p->v[j] = x;
  p->v[j + 1] = y;
  p->n += 2;
}
/* splice(j, 2) */
static void poly_remove2(Poly *p, size_t j) {
  memmove(p->v + j, p->v + j + 2, (p->n - j - 2) * sizeof(double));
  p->n -= 2;
}
static Poly *poly_copy(const Poly *src) {
  Poly *p = poly_new();
  poly_reserve(p, src->n);
  memcpy(p->v, src->v, src->n * sizeof(double));
  p->n = src->n;
  return p;
}

typedef struct {
  Delaunay *del;
  double xmin, ymin, xmax, ymax;
  double *circumcenters;   /* 2 per triangle */
  double *vectors;         /* 4 per point */
} Voronoi;

static void voronoi_init(Voronoi *V) {
  Delaunay *D = V->del;
  const Delaunator *d = &D->dl;
  const double *points = d->coords;
  size_t ntri = d->trianglesLen / 3;
  double bx = 0, by = 0;
  int haveB = 0;
  for (size_t i = 0, j = 0; i < d->trianglesLen; i += 3, j += 2) {
    size_t t1 = 2 * d->triangles[i], t2 = 2 * d->triangles[i + 1], t3 = 2 * d->triangles[i + 2];
    double x1 = points[t1], y1 = points[t1 + 1], x2 = points[t2], y2 = points[t2 + 1];
    double x3 = points[t3], y3 = points[t3 + 1];
    double dx = x2 - x1, dy = y2 - y1, ex = x3 - x1, ey = y3 - y1, ab = 2 * (dx * ey - dy * ex);
    double x, y;
    if (fabs(ab) < 1e-9) {
      if (!haveB) {
        bx = by = 0;
        for (size_t h = 0; h < d->hullSize; h++) {
          bx += points[2 * d->hull[h]];
          by += points[2 * d->hull[h] + 1];
        }
        bx /= (double)d->hullSize;
        by /= (double)d->hullSize;
        haveB = 1;
      }
      double s = (bx - x1) * ey - (by - y1) * ex;
      double a = 1e9 * (s > 0 ? 1 : s < 0 ? -1 : s);
      x = (x1 + x3) / 2 - a * ey;
      y = (y1 + y3) / 2 + a * ex;
    } else {
      double dd = 1 / ab, bl = dx * dx + dy * dy, cl = ex * ex + ey * ey;
      x = x1 + (ey * bl - dy * cl) * dd;
      y = y1 + (dx * cl - ex * bl) * dd;
    }
    V->circumcenters[j] = x;
    V->circumcenters[j + 1] = y;
  }
  (void)ntri;
  size_t np = 4 * D->n;
  for (size_t i = 0; i < np; i++) V->vectors[i] = 0;
  size_t h = d->hull[d->hullSize - 1];
  size_t p1 = 4 * h;
  double x1 = points[2 * h], y1 = points[2 * h + 1];
  for (size_t i = 0; i < d->hullSize; i++) {
    h = d->hull[i];
    size_t p0 = p1;
    double x0 = x1, y0 = y1;
    p1 = 4 * h;
    x1 = points[2 * h];
    y1 = points[2 * h + 1];
    V->vectors[p0 + 2] = V->vectors[p1] = y0 - y1;
    V->vectors[p0 + 3] = V->vectors[p1 + 1] = x1 - x0;
  }
}

static int regioncode(const Voronoi *V, double x, double y) {
  return (x < V->xmin ? 1 : x > V->xmax ? 2 : 0) | (y < V->ymin ? 4 : y > V->ymax ? 8 : 0);
}
static int edgecode(const Voronoi *V, double x, double y) {
  return (x == V->xmin ? 1 : x == V->xmax ? 2 : 0) | (y == V->ymin ? 4 : y == V->ymax ? 8 : 0);
}

/* _step: the nearest point found by walking the triangulation from i (used by contains). */
static size_t delaunay_step(const Delaunay *D, size_t i, double x, double y) {
  const Delaunator *d = &D->dl;
  const double *points = d->coords;
  if (D->inedges[i] == -1 || d->n == 0) return (i + 1) % d->n;
  size_t c = i;
  double dc = dist2(x, y, points[2 * i], points[2 * i + 1]);
  size_t e0 = (size_t)D->inedges[i], e = e0;
  do {
    size_t t = d->triangles[e];
    double dt = dist2(x, y, points[2 * t], points[2 * t + 1]);
    if (dt < dc) { dc = dt; c = t; }
    e = e % 3 == 2 ? e - 2 : e + 1;
    if (d->triangles[e] != i) break;
    int32_t he = d->halfedges[e];
    if (he == -1) {
      e = d->hull[(D->hullIndex[i] + 1) % d->hullSize];
      if (e != t && dist2(x, y, points[2 * e], points[2 * e + 1]) < dc) return e;
      break;
    }
    e = (size_t)he;
  } while (e != e0);
  return c;
}
static int voronoi_contains(const Voronoi *V, size_t i, double x, double y) {
  return delaunay_step(V->del, i, x, y) == i;
}

/* _cell: the circumcenters around point i, or NULL when i has no triangle. */
static Poly *voronoi_cell(const Voronoi *V, size_t i) {
  const Delaunator *d = &V->del->dl;
  int32_t e0 = V->del->inedges[i];
  if (e0 == -1) return NULL;
  Poly *p = poly_new();
  int32_t e = e0;
  do {
    size_t t = (size_t)e / 3;
    poly_push(p, V->circumcenters[2 * t], V->circumcenters[2 * t + 1]);
    e = (size_t)e % 3 == 2 ? e - 2 : e + 1;
    if (d->triangles[e] != i) break;
    e = d->halfedges[e];
  } while (e != e0 && e != -1);
  return p;
}

static int clip_segment(const Voronoi *V, double x0, double y0, double x1, double y1, int c0, int c1, double *out) {
  int flip = c0 < c1;
  if (flip) {
    double tx = x0, ty = y0;
    x0 = x1; y0 = y1; x1 = tx; y1 = ty;
    int tc = c0; c0 = c1; c1 = tc;
  }
  for (;;) {
    if (c0 == 0 && c1 == 0) {
      if (flip) { out[0] = x1; out[1] = y1; out[2] = x0; out[3] = y0; }
      else { out[0] = x0; out[1] = y0; out[2] = x1; out[3] = y1; }
      return 1;
    }
    if (c0 & c1) return 0;
    double x, y;
    int c = c0 ? c0 : c1;
    if (8 & c) { x = x0 + (x1 - x0) * (V->ymax - y0) / (y1 - y0); y = V->ymax; }
    else if (4 & c) { x = x0 + (x1 - x0) * (V->ymin - y0) / (y1 - y0); y = V->ymin; }
    else if (2 & c) { y = y0 + (y1 - y0) * (V->xmax - x0) / (x1 - x0); x = V->xmax; }
    else { y = y0 + (y1 - y0) * (V->xmin - x0) / (x1 - x0); x = V->xmin; }
    if (c0) { x0 = x; y0 = y; c0 = regioncode(V, x0, y0); }
    else { x1 = x; y1 = y; c1 = regioncode(V, x1, y1); }
  }
}

/* _edge: inserts the bounding-box corners between edge codes e0 and e1 into P at j; returns the new j. */
static size_t voronoi_edge(const Voronoi *V, size_t i, int e0, int e1, Poly *P, size_t j) {
  while (e0 != e1) {
    double x = 0, y = 0;
    switch (e0) {
      case 5: e0 = 4; continue;
      case 4: e0 = 6; x = V->xmax; y = V->ymin; break;
      case 6: e0 = 2; continue;
      case 2: e0 = 10; x = V->xmax; y = V->ymax; break;
      case 10: e0 = 8; continue;
      case 8: e0 = 9; x = V->xmin; y = V->ymax; break;
      case 9: e0 = 1; continue;
      case 1: e0 = 5; x = V->xmin; y = V->ymin; break;
      default: die("jsdom-d3-startup: bad edge code");
    }
    if (P->v[j] == x && P->v[j + 1] == y) continue;
    if (!voronoi_contains(V, i, x, y)) continue;
    poly_insert(P, j, x, y);
    j += 2;
  }
  return j;
}

/* _clipFinite: NULL is JS null. */
static Poly *voronoi_clip_finite(const Voronoi *V, size_t i, const Poly *pts) {
  size_t n = pts->n;
  Poly *P = NULL;
  double x0, y0, x1 = pts->v[n - 2], y1 = pts->v[n - 1];
  int c0, c1 = regioncode(V, x1, y1), e0 = 0, e1 = 0;
  for (size_t j = 0; j < n; j += 2) {
    x0 = x1;
    y0 = y1;
    x1 = pts->v[j];
    y1 = pts->v[j + 1];
    c0 = c1;
    c1 = regioncode(V, x1, y1);
    if (c0 == 0 && c1 == 0) {
      e0 = e1;
      e1 = 0;
      if (!P) P = poly_new();
      poly_push(P, x1, y1);
    } else {
      double S[4], sx0, sy0, sx1, sy1;
      if (c0 == 0) {
        if (!clip_segment(V, x0, y0, x1, y1, c0, c1, S)) continue;
        sx0 = S[0]; sy0 = S[1]; sx1 = S[2]; sy1 = S[3];
      } else {
        if (!clip_segment(V, x1, y1, x0, y0, c1, c0, S)) continue;
        sx1 = S[0]; sy1 = S[1]; sx0 = S[2]; sy0 = S[3];
        e0 = e1;
        e1 = edgecode(V, sx0, sy0);
        if (e0 && e1) voronoi_edge(V, i, e0, e1, P, P ? P->n : 0);
        if (!P) P = poly_new();
        poly_push(P, sx0, sy0);
      }
      e0 = e1;
      e1 = edgecode(V, sx1, sy1);
      if (e0 && e1) voronoi_edge(V, i, e0, e1, P, P ? P->n : 0);
      if (!P) P = poly_new();
      poly_push(P, sx1, sy1);
    }
  }
  if (P) {
    e0 = e1;
    e1 = edgecode(V, P->v[0], P->v[1]);
    if (e0 && e1) voronoi_edge(V, i, e0, e1, P, P->n);
  } else if (voronoi_contains(V, i, (V->xmin + V->xmax) / 2, (V->ymin + V->ymax) / 2)) {
    P = poly_new();
    poly_push(P, V->xmax, V->ymin);
    poly_push(P, V->xmax, V->ymax);
    poly_push(P, V->xmin, V->ymax);
    poly_push(P, V->xmin, V->ymin);
  }
  return P;
}

/* _project: where the ray from (x0, y0) along (vx, vy) leaves the box; returns 0 for null. */
static int voronoi_project(const Voronoi *V, double x0, double y0, double vx, double vy, double *ox, double *oy) {
  double c, x = NAN, y = NAN, t = INFINITY;
  if (vy < 0) {
    if (y0 <= V->ymin) return 0;
    c = (V->ymin - y0) / vy;
    if (c < t) { y = V->ymin; t = c; x = x0 + t * vx; }
  } else if (vy > 0) {
    if (y0 >= V->ymax) return 0;
    c = (V->ymax - y0) / vy;
    if (c < t) { y = V->ymax; t = c; x = x0 + t * vx; }
  }
  if (vx > 0) {
    if (x0 >= V->xmax) return 0;
    c = (V->xmax - x0) / vx;
    if (c < t) { x = V->xmax; t = c; y = y0 + t * vy; }
  } else if (vx < 0) {
    if (x0 <= V->xmin) return 0;
    c = (V->xmin - x0) / vx;
    if (c < t) { x = V->xmin; t = c; y = y0 + t * vy; }
  }
  *ox = x;
  *oy = y;
  return 1;
}

/* _clipInfinite: the cell of a point on the hull, closed by the two rays. */
static Poly *voronoi_clip_infinite(const Voronoi *V, size_t i, const Poly *points, double vx0, double vy0, double vxn, double vyn) {
  Poly *P = poly_copy(points);
  double px, py;
  if (voronoi_project(V, P->v[0], P->v[1], vx0, vy0, &px, &py)) poly_insert(P, 0, px, py);
  if (voronoi_project(V, P->v[P->n - 2], P->v[P->n - 1], vxn, vyn, &px, &py)) poly_push(P, px, py);
  Poly *Q = voronoi_clip_finite(V, i, P);
  poly_free(P);
  P = Q;
  if (P) {
    size_t j = 0, n = P->n;
    int c1 = edgecode(V, P->v[n - 2], P->v[n - 1]);
    while (j < n) {
      int c0 = c1;
      c1 = edgecode(V, P->v[j], P->v[j + 1]);
      if (c0 && c1) {
        j = voronoi_edge(V, i, c0, c1, P, j);
        n = P->n;
      }
      j += 2;
    }
  } else if (voronoi_contains(V, i, (V->xmin + V->xmax) / 2, (V->ymin + V->ymax) / 2)) {
    P = poly_new();
    poly_push(P, V->xmin, V->ymin);
    poly_push(P, V->xmax, V->ymin);
    poly_push(P, V->xmax, V->ymax);
    poly_push(P, V->xmin, V->ymax);
  }
  return P;
}

/* _simplify: drops collinear and duplicate runs; NULL when nothing is left. */
static Poly *voronoi_simplify(Poly *P) {
  if (P && P->n > 4) {
    for (long i = 0; i < (long)P->n; i += 2) {
      size_t j = ((size_t)i + 2) % P->n, k = ((size_t)i + 4) % P->n;
      if ((P->v[i] == P->v[j] && P->v[j] == P->v[k]) || (P->v[i + 1] == P->v[j + 1] && P->v[j + 1] == P->v[k + 1])) {
        poly_remove2(P, j);
        i -= 2;
      }
    }
    if (P->n == 0) { poly_free(P); P = NULL; }
  }
  return P;
}

/* _clip: the cell of point i clipped to the bounds, or NULL (JS null). */
static Poly *voronoi_clip(const Voronoi *V, size_t i) {
  if (i == 0 && V->del->dl.hullSize == 1) {
    Poly *p = poly_new();
    poly_push(p, V->xmax, V->ymin);
    poly_push(p, V->xmax, V->ymax);
    poly_push(p, V->xmin, V->ymax);
    poly_push(p, V->xmin, V->ymin);
    return p;
  }
  Poly *points = voronoi_cell(V, i);
  if (!points) return NULL;
  size_t v = 4 * i;
  Poly *r;
  if (V->vectors[v] || V->vectors[v + 1])
    r = voronoi_clip_infinite(V, i, points, V->vectors[v], V->vectors[v + 1], V->vectors[v + 2], V->vectors[v + 3]);
  else
    r = voronoi_clip_finite(V, i, points);
  poly_free(points);
  return voronoi_simplify(r);
}

/* renderCell(i) with d3-delaunay's Path: "M x,y L x,y ... Z" with JS number formatting. Returns 0 for
 * undefined (the attribute is then not set). */
static int voronoi_render_cell(const Voronoi *V, size_t i, Out *o) {
  Poly *points = voronoi_clip(V, i);
  if (!points || points->n == 0) {
    poly_free(points);
    return 0;
  }
  const double *v = points->v;
  size_t n = points->n;
  ob_str(o, "M");
  ob_num(o, v[0]);
  ob_chr(o, ',');
  ob_num(o, v[1]);
  while (n > 1 && v[0] == v[n - 2] && v[1] == v[n - 1]) n -= 2;
  for (size_t k = 2; k < n; k += 2) {
    if (v[k] == v[k - 2] && v[k + 1] == v[k - 1]) continue;
    ob_str(o, "L");
    ob_num(o, v[k]);
    ob_chr(o, ',');
    ob_num(o, v[k + 1]);
  }
  ob_chr(o, 'Z');
  poly_free(points);
  return 1;
}

/* ================= string-keyed fragment maps (JS objects keyed by "x,y") ================= */
typedef struct Frag Frag;
struct Frag {
  long *arcs;
  size_t n, cap;
  char *start, *end;   /* keys; NULL is JS undefined after delete */
};
typedef struct MEnt {
  char *key;
  Frag *val;
  struct MEnt *prev, *next;  /* insertion order, as for-in visits */
  struct MEnt *hnext;        /* hash chain */
} MEnt;
typedef struct {
  MEnt **buckets;
  size_t nb;
  MEnt *head, *tail;
} FMap;

static uint32_t str_hash(const char *s) {
  uint32_t h = 2166136261u;
  for (; *s; s++) h = (h ^ (unsigned char)*s) * 16777619u;
  return h;
}
static void fmap_init(FMap *m) {
  m->nb = 1024;
  m->buckets = calloc(m->nb, sizeof *m->buckets);
  if (!m->buckets) die("out of memory");
  m->head = m->tail = NULL;
}
static void fmap_free(FMap *m) {
  for (MEnt *e = m->head; e;) {
    MEnt *nx = e->next;
    free(e->key);
    free(e);
    e = nx;
  }
  free(m->buckets);
}
static MEnt *fmap_find(const FMap *m, const char *key) {
  if (!key) return NULL;
  for (MEnt *e = m->buckets[str_hash(key) & (m->nb - 1)]; e; e = e->hnext)
    if (strcmp(e->key, key) == 0) return e;
  return NULL;
}
static Frag *fmap_get(const FMap *m, const char *key) {
  MEnt *e = fmap_find(m, key);
  return e ? e->val : NULL;
}
/* obj[key] = val: an existing key keeps its position. */
static void fmap_set(FMap *m, const char *key, Frag *val) {
  MEnt *e = fmap_find(m, key);
  if (e) { e->val = val; return; }
  e = calloc(1, sizeof *e);
  if (!e) die("out of memory");
  e->key = strdup(key);
  e->val = val;
  size_t b = str_hash(key) & (m->nb - 1);
  e->hnext = m->buckets[b];
  m->buckets[b] = e;
  e->prev = m->tail;
  if (m->tail) m->tail->next = e;
  else m->head = e;
  m->tail = e;
}
/* delete obj[key]: the key goes away; a later set puts it at the end. */
static void fmap_del(FMap *m, const char *key) {
  if (!key) return;
  size_t b = str_hash(key) & (m->nb - 1);
  MEnt **pp = &m->buckets[b];
  while (*pp && strcmp((*pp)->key, key) != 0) pp = &(*pp)->hnext;
  MEnt *e = *pp;
  if (!e) return;
  *pp = e->hnext;
  if (e->prev) e->prev->next = e->next;
  else m->head = e->next;
  if (e->next) e->next->prev = e->prev;
  else m->tail = e->prev;
  free(e->key);
  free(e);
}

static void frag_push(Frag *f, long v) {
  if (f->n == f->cap) {
    f->cap = f->cap ? f->cap * 2 : 4;
    f->arcs = realloc(f->arcs, f->cap * sizeof *f->arcs);
    if (!f->arcs) die("out of memory");
  }
  f->arcs[f->n++] = v;
}
static void frag_unshift(Frag *f, long v) {
  frag_push(f, 0);
  memmove(f->arcs + 1, f->arcs, (f->n - 1) * sizeof *f->arcs);
  f->arcs[0] = v;
}

/* ================= TopoJSON decoding (topojson-client) ================= */
typedef struct {
  const JValue *arcs;     /* array of arcs; each arc is an array of [dx, dy] */
  double kx, ky, dx, dy;  /* transform.scale and transform.translate */
} Topo;

typedef struct { double *xy; size_t n, cap; } Pts; /* n points */

static void pts_push(Pts *p, double x, double y) {
  if (p->n == p->cap) {
    p->cap = p->cap ? p->cap * 2 : 64;
    p->xy = realloc(p->xy, p->cap * 2 * sizeof(double));
    if (!p->xy) die("out of memory");
  }
  p->xy[2 * p->n] = x;
  p->xy[2 * p->n + 1] = y;
  p->n++;
}

static const JValue *jobj_get(const JValue *o, const char *key) {
  size_t kl = strlen(key);
  if (!o || o->type != J_OBJECT) die("jsdom-d3-startup: expected an object");
  for (uint32_t i = 0; i < o->count; i++) {
    const JMember *m = &o->u.members[i];
    if (m->klen == kl && memcmp(m->key, key, kl) == 0) return m->value;
  }
  die("jsdom-d3-startup: missing key");
  return NULL;
}
static double jnum(const JValue *v) {
  if (v->type != J_NUMBER) die("jsdom-d3-startup: expected a number");
  return v->u.number;
}

/* arc(i, points) of topojson-client: appends the decoded arc, reversed for a negative index. */
static void topo_arc(const Topo *T, long i, Pts *out) {
  size_t idx = i < 0 ? (size_t)(~i) : (size_t)i;
  const JValue *a = T->arcs->u.items[idx];
  if (out->n) out->n--;   /* points.pop() */
  double x0 = 0, y0 = 0;
  for (uint32_t k = 0; k < a->count; k++) {
    const JValue *q = a->u.items[k];
    if (k == 0) { x0 = 0; y0 = 0; }
    x0 += jnum(q->u.items[0]);
    y0 += jnum(q->u.items[1]);
    pts_push(out, x0 * T->kx + T->dx, y0 * T->ky + T->dy);
  }
  if (i < 0) {
    size_t lo = out->n - a->count;
    for (size_t p = lo, q = out->n - 1; p < q; p++, q--) {
      double tx = out->xy[2 * p], ty = out->xy[2 * p + 1];
      out->xy[2 * p] = out->xy[2 * q];
      out->xy[2 * p + 1] = out->xy[2 * q + 1];
      out->xy[2 * q] = tx;
      out->xy[2 * q + 1] = ty;
    }
  }
}

/* line(arcs): the arcs of one line, padded to two points. */
static void topo_line(const Topo *T, const long *arcs, size_t na, Pts *out) {
  out->n = 0;
  for (size_t k = 0; k < na; k++) topo_arc(T, arcs[k], out);
  if (out->n < 2) {
    if (out->n == 0) die("jsdom-d3-startup: empty line");
    pts_push(out, out->xy[0], out->xy[1]);
  }
}
/* ring(arcs): a line padded to at least four points. */
static void topo_ring(const Topo *T, const long *arcs, size_t na, Pts *out) {
  topo_line(T, arcs, na, out);
  while (out->n < 4) pts_push(out, out->xy[0], out->xy[1]);
}

/* A list of arc indices from a JSON array. */
static long *jlist(const JValue *a, size_t *n) {
  long *r = malloc((a->count ? a->count : 1) * sizeof *r);
  if (!r) die("out of memory");
  for (uint32_t k = 0; k < a->count; k++) r[k] = (long)jnum(a->u.items[k]);
  *n = a->count;
  return r;
}

/* ================= geoPath (no projection, digits 3) ================= */
/* A polygon ring or line: M for the first point, L for the others, Z per ring when in a polygon. */
static void geo_stream_line(Out *o, const Pts *p, int closed, int polygon) {
  size_t n = p->n - (size_t)closed;
  for (size_t i = 0; i < n; i++) {
    ob_chr(o, i == 0 ? 'M' : 'L');
    ob_num3(o, p->xy[2 * i]);
    ob_chr(o, ',');
    ob_num3(o, p->xy[2 * i + 1]);
  }
  if (polygon) ob_chr(o, 'Z');
}

/* feature(topology, states) for a GeometryCollection of MultiPolygons, streamed through geoPath. */
static void states_path(const Topo *T, const JValue *states, Out *o) {
  const JValue *geoms = jobj_get(states, "geometries");
  Pts ring = {0};
  for (uint32_t g = 0; g < geoms->count; g++) {
    const JValue *geom = geoms->u.items[g];
    const JValue *type = jobj_get(geom, "type");
    if (type->type != J_STRING || type->u.str.n != 12 || memcmp(type->u.str.p, "MultiPolygon", 12) != 0)
      die("jsdom-d3-startup: only MultiPolygon states are supported");
    const JValue *polys = jobj_get(geom, "arcs");
    for (uint32_t p = 0; p < polys->count; p++) {
      const JValue *rings = polys->u.items[p];
      for (uint32_t r = 0; r < rings->count; r++) {
        size_t na;
        long *arcs = jlist(rings->u.items[r], &na);
        topo_ring(T, arcs, na, &ring);
        free(arcs);
        geo_stream_line(o, &ring, 1, 1);
      }
    }
  }
  free(ring.xy);
}

/* ================= topojson.mesh(topology, states, (a, b) => a !== b) ================= */
/* extractArcs: arcs used by two different geometries, in arc-index order (the first use's index). */
static size_t mesh_arcs(const Topo *T, const JValue *states, long **outArcs) {
  size_t narcs = T->arcs->count;
  long *firstI = calloc(narcs, sizeof *firstI);
  size_t *firstG = calloc(narcs, sizeof *firstG), *lastG = calloc(narcs, sizeof *lastG);
  unsigned char *used = calloc(narcs, 1);
  if (!firstI || !firstG || !lastG || !used) die("out of memory");
  const JValue *geoms = jobj_get(states, "geometries");
  for (uint32_t g = 0; g < geoms->count; g++) {
    const JValue *geom = geoms->u.items[g];
    const JValue *polys = jobj_get(geom, "arcs");
    for (uint32_t p = 0; p < polys->count; p++) {
      const JValue *rings = polys->u.items[p];
      for (uint32_t r = 0; r < rings->count; r++) {
        const JValue *ring = rings->u.items[r];
        for (uint32_t k = 0; k < ring->count; k++) {
          long i = (long)jnum(ring->u.items[k]);
          size_t j = i < 0 ? (size_t)(~i) : (size_t)i;
          if (!used[j]) { used[j] = 1; firstI[j] = i; firstG[j] = g; }
          lastG[j] = g;
        }
      }
    }
  }
  long *arcs = malloc((narcs ? narcs : 1) * sizeof *arcs);
  if (!arcs) die("out of memory");
  size_t na = 0;
  for (size_t j = 0; j < narcs; j++)
    if (used[j] && firstG[j] != lastG[j]) arcs[na++] = firstI[j];
  free(firstI);
  free(firstG);
  free(lastG);
  free(used);
  *outArcs = arcs;
  return na;
}

/* Fragments and their key strings live in a pool for the duration of one stitch. */
typedef struct {
  char **strs;
  size_t ns, cs;
  Frag **frs;
  size_t nf, cf;
} Pool;
static char *pool_str(Pool *p, const char *s) {
  if (p->ns == p->cs) {
    p->cs = p->cs ? p->cs * 2 : 256;
    p->strs = realloc(p->strs, p->cs * sizeof *p->strs);
    if (!p->strs) die("out of memory");
  }
  p->strs[p->ns++] = strdup(s);
  return p->strs[p->ns - 1];
}
static Frag *pool_frag(Pool *p) {
  if (p->nf == p->cf) {
    p->cf = p->cf ? p->cf * 2 : 256;
    p->frs = realloc(p->frs, p->cf * sizeof *p->frs);
    if (!p->frs) die("out of memory");
  }
  Frag *f = calloc(1, sizeof *f);
  if (!f) die("out of memory");
  p->frs[p->nf++] = f;
  return f;
}
static void pool_free(Pool *p) {
  for (size_t i = 0; i < p->ns; i++) free(p->strs[i]);
  for (size_t i = 0; i < p->nf; i++) {
    free(p->frs[i]->arcs);
    free(p->frs[i]);
  }
  free(p->strs);
  free(p->frs);
}
/* f.concat(g) */
static Frag *frag_concat(Pool *p, const Frag *a, const Frag *b) {
  Frag *f = pool_frag(p);
  for (size_t i = 0; i < a->n; i++) frag_push(f, a->arcs[i]);
  for (size_t i = 0; i < b->n; i++) frag_push(f, b->arcs[i]);
  return f;
}

/* Endpoint keys of arc i: start and end as "x,y" of the quantized coordinates (String of the pair). */
static void arc_ends(const Topo *T, long i, char *b0, char *b1, size_t bn) {
  size_t idx = i < 0 ? (size_t)(~i) : (size_t)i;
  const JValue *a = T->arcs->u.items[idx];
  long p0x = (long)jnum(a->u.items[0]->u.items[0]), p0y = (long)jnum(a->u.items[0]->u.items[1]);
  long p1x = 0, p1y = 0;
  for (uint32_t k = 0; k < a->count; k++) {
    p1x += (long)jnum(a->u.items[k]->u.items[0]);
    p1y += (long)jnum(a->u.items[k]->u.items[1]);
  }
  char s0[64], s1[64];
  snprintf(s0, sizeof s0, "%ld,%ld", p0x, p0y);
  snprintf(s1, sizeof s1, "%ld,%ld", p1x, p1y);
  const char *start = i < 0 ? s1 : s0, *end = i < 0 ? s0 : s1;
  snprintf(b0, bn, "%s", start);
  snprintf(b1, bn, "%s", end);
}

/* stitchArcs of topojson-client: joins the arcs into fragments by their end points. The two maps are
 * JS objects keyed by "x,y": they iterate in insertion order, and a set on an existing key keeps its place. */
static size_t stitch(const Topo *T, long *arcs, size_t na, Pool *pool, Frag ***outFrags) {
  size_t narcs = T->arcs->count;
  unsigned char *stitched = calloc(narcs, 1);
  FMap byStart, byEnd;
  fmap_init(&byStart);
  fmap_init(&byEnd);
  size_t cap = 64, nf = 0;
  Frag **frags = malloc(cap * sizeof *frags);
  if (!frags || !stitched) die("out of memory");
  /* degenerate arcs (a zero second point) are swapped to the front */
  long emptyIndex = -1;
  for (size_t j = 0; j < na; j++) {
    long i = arcs[j];
    size_t idx = i < 0 ? (size_t)(~i) : (size_t)i;
    const JValue *a = T->arcs->u.items[idx];
    if (a->count < 3 && a->count >= 2 && jnum(a->u.items[1]->u.items[0]) == 0 && jnum(a->u.items[1]->u.items[1]) == 0) {
      emptyIndex++;
      if ((size_t)emptyIndex >= na) die("jsdom-d3-startup: degenerate arc without a slot");
      long t = arcs[emptyIndex];
      arcs[emptyIndex] = i;
      arcs[j] = t;
    }
  }
  for (size_t j = 0; j < na; j++) {
    long i = arcs[j];
    char kb[64], ke[64];
    arc_ends(T, i, kb, ke, sizeof kb);
    char *start = pool_str(pool, kb), *end = pool_str(pool, ke);
    Frag *f = fmap_get(&byEnd, start);
    if (f) {
      fmap_del(&byEnd, f->end);
      frag_push(f, i);
      f->end = end;
      Frag *g = fmap_get(&byStart, end);
      if (g) {
        fmap_del(&byStart, g->start);
        Frag *fg = (g == f) ? f : frag_concat(pool, f, g);
        fg->start = f->start;
        fg->end = g->end;
        fmap_set(&byStart, fg->start, fg);
        fmap_set(&byEnd, fg->end, fg);
      } else {
        fmap_set(&byStart, f->start, f);
        fmap_set(&byEnd, f->end, f);
      }
    } else if ((f = fmap_get(&byStart, end))) {
      fmap_del(&byStart, f->start);
      frag_unshift(f, i);
      f->start = start;
      Frag *g = fmap_get(&byEnd, start);
      if (g) {
        fmap_del(&byEnd, g->end);
        Frag *gf = (g == f) ? f : frag_concat(pool, g, f);
        gf->start = g->start;
        gf->end = f->end;
        fmap_set(&byStart, gf->start, gf);
        fmap_set(&byEnd, gf->end, gf);
      } else {
        fmap_set(&byStart, f->start, f);
        fmap_set(&byEnd, f->end, f);
      }
    } else {
      f = pool_frag(pool);
      frag_push(f, i);
      f->start = start;
      f->end = end;
      fmap_set(&byStart, start, f);
      fmap_set(&byEnd, end, f);
    }
  }
  /* flush: the fragments registered in each map, in its order; their keys become undefined */
  for (int pass = 0; pass < 2; pass++) {
    FMap *m1 = pass == 0 ? &byEnd : &byStart;
    FMap *m2 = pass == 0 ? &byStart : &byEnd;
    for (MEnt *e = m1->head; e; e = e->next) {
      Frag *f = e->val;
      fmap_del(m2, f->start);
      f->start = f->end = NULL;
      for (size_t k = 0; k < f->n; k++) stitched[f->arcs[k] < 0 ? (size_t)(~f->arcs[k]) : (size_t)f->arcs[k]] = 1;
      if (nf == cap) {
        cap *= 2;
        frags = realloc(frags, cap * sizeof *frags);
        if (!frags) die("out of memory");
      }
      frags[nf++] = f;
    }
  }
  for (size_t j = 0; j < na; j++) {
    long i = arcs[j];
    size_t idx = i < 0 ? (size_t)(~i) : (size_t)i;
    if (!stitched[idx]) {
      Frag *f = pool_frag(pool);
      frag_push(f, i);
      if (nf == cap) {
        cap *= 2;
        frags = realloc(frags, cap * sizeof *frags);
        if (!frags) die("out of memory");
      }
      frags[nf++] = f;
    }
  }
  fmap_free(&byStart);
  fmap_free(&byEnd);
  free(stitched);
  *outFrags = frags;
  return nf;
}

/* MultiLineString path for topojson.mesh: each stitched fragment is one line, no Z. */
static void mesh_path(const Topo *T, const JValue *states, Out *o) {
  long *arcs;
  size_t na = mesh_arcs(T, states, &arcs);
  Pool pool = {0};
  Frag **frags;
  size_t nf = stitch(T, arcs, na, &pool, &frags);
  Pts line = {0};
  for (size_t f = 0; f < nf; f++) {
    topo_line(T, frags[f]->arcs, frags[f]->n, &line);
    geo_stream_line(o, &line, 0, 0);
  }
  free(line.xy);
  free(frags);
  pool_free(&pool);
  free(arcs);
}

/* ================= CSV (d3.csvParse with d3.autoType) ================= */
enum { V_UNDEF, V_NULL, V_NUM, V_STR };
typedef struct { int kind; double num; const char *s; size_t n; } Val;

static int is_ws(char c) { return c == ' ' || c == '\t' || c == '\n' || c == '\r' || c == '\v' || c == '\f'; }

/* Decimal literal as the JS unary + accepts it (the subset in the row's data). */
static int decimal_literal(const char *s, size_t n) {
  size_t i = 0;
  if (i < n && (s[i] == '+' || s[i] == '-')) i++;
  size_t d1 = 0, d2 = 0;
  while (i < n && s[i] >= '0' && s[i] <= '9') { i++; d1++; }
  if (i < n && s[i] == '.') {
    i++;
    while (i < n && s[i] >= '0' && s[i] <= '9') { i++; d2++; }
  }
  if (d1 + d2 == 0) return 0;
  if (i < n && (s[i] == 'e' || s[i] == 'E')) {
    i++;
    if (i < n && (s[i] == '+' || s[i] == '-')) i++;
    size_t d3 = 0;
    while (i < n && s[i] >= '0' && s[i] <= '9') { i++; d3++; }
    if (d3 == 0) return 0;
  }
  return i == n;
}

/* A whole-field hex, octal or binary literal (0x1f, 0o17, 0b101), or Infinity: JS's unary + reads these as
 * numbers, which the C side does not model, so they are reported rather than read as text. */
static int is_nondecimal_number(const char *t, size_t n) {
  if (n >= 3 && t[0] == '0' && strchr("xXoObB", t[1])) {
    int base = (t[1] == 'x' || t[1] == 'X') ? 16 : (t[1] == 'o' || t[1] == 'O') ? 8 : 2;
    for (size_t i = 2; i < n; i++) {
      int d = isdigit((unsigned char)t[i]) ? t[i] - '0' : isxdigit((unsigned char)t[i]) ? (tolower((unsigned char)t[i]) - 'a' + 10) : 99;
      if (d >= base) return 0;
    }
    return 1;
  }
  size_t k = (n && (t[0] == '+' || t[0] == '-')) ? 1 : 0;
  return n - k == 8 && memcmp(t + k, "Infinity", 8) == 0;
}

/* autoType for one field: empty is null, a numeric field is a number, anything else stays the raw text. */
static Val auto_type(const char *raw, size_t n) {
  Val v = {V_STR, 0, raw, n};
  size_t a = 0, b = n;
  while (a < b && is_ws(raw[a])) a++;
  while (b > a && is_ws(raw[b - 1])) b--;
  if (a == b) { v.kind = V_NULL; return v; }
  const char *t = raw + a;
  size_t tn = b - a;
  if ((tn == 4 && memcmp(t, "true", 4) == 0) || (tn == 5 && memcmp(t, "false", 5) == 0) || (tn == 3 && memcmp(t, "NaN", 3) == 0))
    die("jsdom-d3-startup: boolean or NaN field is not supported");
  if (decimal_literal(t, tn)) {
    char buf[128];
    if (tn >= sizeof buf) die("jsdom-d3-startup: numeric field too long");
    memcpy(buf, t, tn);
    buf[tn] = 0;
    v.kind = V_NUM;
    v.num = strtod(buf, NULL);
    return v;
  }
  if (is_nondecimal_number(t, tn))
    die("jsdom-d3-startup: non-decimal numeric field is not supported");
  if (tn >= 5 && isdigit((unsigned char)t[0]) && isdigit((unsigned char)t[1]) && isdigit((unsigned char)t[2]) &&
      isdigit((unsigned char)t[3]) && t[4] == '-')
    die("jsdom-d3-startup: date field is not supported");
  return v;
}

typedef struct {
  char *header_text[16];
  size_t ncols;
  Val *cells;      /* nrows * ncols */
  size_t nrows;
  char *ubuf;      /* unescaped field text */
  size_t ubuf_n;
} Airports;

/* RFC 4180 parse into rows of fields (d3-dsv's parseRows), then autoType per field. */
static void csv_parse_airports(const char *text, size_t n, Airports *A) {
  A->ubuf = malloc(n + 1);
  if (!A->ubuf) die("out of memory");
  size_t uo = 0;
  /* first pass: count rows so the cells can be allocated once */
  size_t nrows = 0;
  for (size_t i = 0; i < n; i++)
    if (text[i] == '\n') nrows++;
  if (n && text[n - 1] != '\n') nrows++;
  Val *cells = malloc((nrows * 8 + 8) * sizeof *cells);
  if (!cells) die("out of memory");
  size_t nf = 0, rowFields = 0, row = 0;
  size_t i = 0;
  while (i < n) {
    /* one field */
    size_t start = uo;
    if (text[i] == '"') {
      i++;
      while (i < n) {
        if (text[i] == '"') {
          if (i + 1 < n && text[i + 1] == '"') { A->ubuf[uo++] = '"'; i += 2; }
          else { i++; break; }
        } else {
          A->ubuf[uo++] = text[i++];
        }
      }
      while (i < n && text[i] != ',' && text[i] != '\n' && text[i] != '\r') A->ubuf[uo++] = text[i++];
    } else {
      while (i < n && text[i] != ',' && text[i] != '\n' && text[i] != '\r') A->ubuf[uo++] = text[i++];
    }
    if (row == 0) {
      /* header names are kept only for the lookup by name */
      if (rowFields < 16) A->header_text[rowFields] = strndup(A->ubuf + start, uo - start);
      rowFields++;
    } else {
      if (nf >= nrows * 8) die("jsdom-d3-startup: too many fields");
      cells[nf++] = auto_type(A->ubuf + start, uo - start);
      rowFields++;
    }
    if (i < n && text[i] == ',') { i++; continue; }
    /* end of row */
    if (i < n && text[i] == '\r') { i++; if (i < n && text[i] == '\n') i++; }
    else if (i < n && text[i] == '\n') i++;
    if (row == 0) {
      A->ncols = rowFields;
      if (rowFields != 8) die("jsdom-d3-startup: unexpected CSV columns");
    } else if (rowFields != A->ncols) {
      die("jsdom-d3-startup: ragged CSV row");
    }
    rowFields = 0;
    row++;
  }
  A->cells = cells;
  A->nrows = row ? row - 1 : 0;
  A->ubuf_n = uo;
}

/* d[name] for an airport row: undefined when the column does not exist. */
static Val airport_field(const Airports *A, size_t row, const char *name) {
  for (size_t c = 0; c < A->ncols; c++)
    if (strcmp(A->header_text[c], name) == 0) return A->cells[row * A->ncols + c];
  Val u = {V_UNDEF, 0, NULL, 0};
  return u;
}

/* JS ToString of a field inside a template literal. */
static void ob_val_tmpl(Out *o, Val v) {
  switch (v.kind) {
    case V_UNDEF: ob_str(o, "undefined"); break;
    case V_NULL: ob_str(o, "null"); break;
    case V_NUM: ob_num(o, v.num); break;
    default: ob_put(o, v.s, v.n); break;
  }
}
/* d3's text(value): null or undefined become "" */
static void ob_val_text(Out *o, Val v) {
  if (v.kind == V_UNDEF || v.kind == V_NULL) return;
  ob_val_tmpl(o, v);
}

/* ================= DOM and serialization ================= */
typedef struct El El;
typedef struct { const char *name; char *val; } Attr;
struct El {
  const char *tag;       /* element name; NULL for a text node */
  char *text;            /* text node content */
  size_t tlen;
  Attr *attr;
  int nattr, capattr;
  El **kid;
  int nkid, capkid;
};

static El *el_new(const char *tag) {
  El *e = calloc(1, sizeof *e);
  if (!e) die("out of memory");
  e->tag = tag;
  return e;
}
static El *el_append(El *parent, const char *tag) {
  if (parent->nkid == parent->capkid) {
    parent->capkid = parent->capkid ? parent->capkid * 2 : 4;
    parent->kid = realloc(parent->kid, parent->capkid * sizeof *parent->kid);
    if (!parent->kid) die("out of memory");
  }
  El *c = el_new(tag);
  parent->kid[parent->nkid++] = c;
  return c;
}
/* setAttribute: replaces the value of an existing name in place, else appends. */
static void el_attr(El *e, const char *name, const char *val, size_t vlen) {
  for (int i = 0; i < e->nattr; i++)
    if (strcmp(e->attr[i].name, name) == 0) {
      free(e->attr[i].val);
      e->attr[i].val = malloc(vlen + 1);
      memcpy(e->attr[i].val, val, vlen);
      e->attr[i].val[vlen] = 0;
      return;
    }
  if (e->nattr == e->capattr) {
    e->capattr = e->capattr ? e->capattr * 2 : 4;
    e->attr = realloc(e->attr, e->capattr * sizeof *e->attr);
    if (!e->attr) die("out of memory");
  }
  e->attr[e->nattr].name = name;
  e->attr[e->nattr].val = malloc(vlen + 1);
  memcpy(e->attr[e->nattr].val, val, vlen);
  e->attr[e->nattr].val[vlen] = 0;
  e->nattr++;
}
static void el_attr_str(El *e, const char *name, const char *val) { el_attr(e, name, val, strlen(val)); }
static void el_attr_out(El *e, const char *name, const Out *o) { el_attr(e, name, o->p, o->n); }
static void el_free(El *e);
/* textContent = s: drops the children and adds one text node (none for an empty string). */
static void el_text(El *e, const char *s, size_t n) {
  for (int i = 0; i < e->nkid; i++) { el_free(e->kid[i]); }
  e->nkid = 0;
  if (n == 0) return;
  El *t = el_append(e, NULL);
  t->text = malloc(n + 1);
  memcpy(t->text, s, n);
  t->text[n] = 0;
  t->tlen = n;
}
static void el_free(El *e) {
  for (int i = 0; i < e->nkid; i++) el_free(e->kid[i]);
  for (int i = 0; i < e->nattr; i++) free(e->attr[i].val);
  free(e->kid);
  free(e->attr);
  free(e->text);
  free(e);
}

/* the HTML serializer's escaping (parse5): text escapes & < > and NBSP; attributes escape & " and NBSP */
static void ser_escaped(Out *o, const char *s, size_t n, int attr) {
  for (size_t i = 0; i < n; i++) {
    unsigned char c = (unsigned char)s[i];
    if (c == '&') ob_str(o, "&amp;");
    else if (c == '<' && !attr) ob_str(o, "&lt;");
    else if (c == '>' && !attr) ob_str(o, "&gt;");
    else if (c == '"' && attr) ob_str(o, "&quot;");
    else if (c == 0xC2 && i + 1 < n && (unsigned char)s[i + 1] == 0xA0) { ob_str(o, "&nbsp;"); i++; }
    else ob_chr(o, (char)c);
  }
}
static void ser_el(Out *o, const El *e) {
  if (!e->tag) {
    ser_escaped(o, e->text, e->tlen, 0);
    return;
  }
  ob_chr(o, '<');
  ob_str(o, e->tag);
  for (int i = 0; i < e->nattr; i++) {
    ob_chr(o, ' ');
    ob_str(o, e->attr[i].name);
    ob_str(o, "=\"");
    ser_escaped(o, e->attr[i].val, strlen(e->attr[i].val), 1);
    ob_chr(o, '"');
  }
  ob_chr(o, '>');
  for (int i = 0; i < e->nkid; i++) ser_el(o, e->kid[i]);
  ob_str(o, "</");
  ob_str(o, e->tag);
  ob_chr(o, '>');
}

/* ================= the scene: runTest ================= */
typedef struct {
  size_t elements, paths, pathsWithoutD, dAttrs, M, L, Z;
  size_t tag_svg, tag_g, tag_title, tag_text, tag_tspan;
} Counts;

static void count_el(const El *e, Counts *c) {
  if (!e->tag) return;
  c->elements++;
  if (strcmp(e->tag, "svg") == 0) c->tag_svg++;
  else if (strcmp(e->tag, "g") == 0) c->tag_g++;
  else if (strcmp(e->tag, "title") == 0) c->tag_title++;
  else if (strcmp(e->tag, "text") == 0) c->tag_text++;
  else if (strcmp(e->tag, "tspan") == 0) c->tag_tspan++;
  else if (strcmp(e->tag, "path") == 0) {
    c->paths++;
    if (e->nattr == 0) c->pathsWithoutD++;
  }
  for (int i = 0; i < e->nattr; i++)
    if (strcmp(e->attr[i].name, "d") == 0) {
      c->dAttrs++;
      for (const char *p = e->attr[i].val; *p; p++) {
        if (*p == 'M') c->M++;
        else if (*p == 'L') c->L++;
        else if (*p == 'Z') c->Z++;
      }
    }
  for (int i = 0; i < e->nkid; i++) count_el(e->kid[i], c);
}

static El *run_test(const Topo *T, const JValue *states, const char *csv, size_t csvn, Out *html) {
  Airports A;
  memset(&A, 0, sizeof A);
  csv_parse_airports(csv, csvn, &A);
  size_t nrows = A.nrows;

  El *root = el_new("body");
  El *svg = el_append(root, "svg");
  {
    char b[64];
    snprintf(b, sizeof b, "%d", WIDTH);
    el_attr_str(svg, "width", b);
    snprintf(b, sizeof b, "%d", HEIGHT);
    el_attr_str(svg, "height", b);
  }
  el_attr_str(svg, "viewBox", "0,0,975,610");
  el_attr_str(svg, "style", "width: 100%; height: auto; height: intrinsic;");

  /* topojson.mesh of the state borders */
  Out d = {0};
  mesh_path(T, states, &d);
  El *meshPath = el_append(svg, "path");
  el_attr_str(meshPath, "fill", "none");
  el_attr_str(meshPath, "stroke", "currentColor");
  el_attr_str(meshPath, "stroke-linejoin", "round");
  el_attr_out(meshPath, "d", &d);
  ob_free(&d);

  /* Delaunay.from(airports, d => d.longitude, d => d.latitude).voronoi([0, 0, 975, 610]) */
  double *coords = malloc((2 * nrows + 2) * sizeof(double));
  if (!coords) die("out of memory");
  for (size_t r = 0; r < nrows; r++) {
    Val lon = airport_field(&A, r, "longitude"), lat = airport_field(&A, r, "latitude");
    if (lon.kind != V_NUM || lat.kind != V_NUM) die("jsdom-d3-startup: airport without coordinates");
    coords[2 * r] = lon.num;
    coords[2 * r + 1] = lat.num;
  }
  Delaunay D;
  delaunay_init_arrays(&D, coords, nrows);
  delaunay_init(&D);
  Voronoi V;
  V.del = &D;
  V.xmin = 0;
  V.ymin = 0;
  V.xmax = WIDTH;
  V.ymax = HEIGHT;
  V.circumcenters = malloc((D.dl.trianglesLen / 3 * 2 + 2) * sizeof(double));
  V.vectors = malloc((4 * nrows + 4) * sizeof(double));
  if (!V.circumcenters || !V.vectors) die("out of memory");
  voronoi_init(&V);

  El *cells = el_append(svg, "g");
  el_attr_str(cells, "fill", "none");
  el_attr_str(cells, "stroke", "currentColor");
  el_attr_str(cells, "stroke-opacity", "0.2");
  for (size_t r = 0; r < nrows; r++) {
    El *p = el_append(cells, "path");
    Out cd = {0};
    if (voronoi_render_cell(&V, r, &cd)) el_attr_out(p, "d", &cd);
    ob_free(&cd);
    El *title = el_append(p, "title");
    Out t = {0};
    ob_val_tmpl(&t, airport_field(&A, r, "name"));
    ob_chr(&t, '\n');
    ob_val_tmpl(&t, airport_field(&A, r, "city"));    /* no such column: undefined, as in the JS */
    ob_str(&t, ", ");
    ob_val_tmpl(&t, airport_field(&A, r, "state"));   /* no such column: undefined */
    el_text(title, t.p, t.n);
    ob_free(&t);
  }
  free(coords);

  /* the state outlines */
  Out sd = {0};
  states_path(T, states, &sd);
  El *statePath = el_append(svg, "path");
  el_attr_str(statePath, "fill", "none");
  el_attr_out(statePath, "d", &sd);
  ob_free(&sd);

  /* the labels: a text per airport with two tspans */
  El *labels = el_append(svg, "g");
  el_attr_str(labels, "font-family", "sans-serif");
  el_attr_str(labels, "font-size", "10");
  el_attr_str(labels, "text-anchor", "middle");
  for (size_t r = 0; r < nrows; r++) {
    Val lon = airport_field(&A, r, "longitude"), lat = airport_field(&A, r, "latitude");
    El *text = el_append(labels, "text");
    Out tr = {0};
    ob_str(&tr, "translate(");
    ob_val_tmpl(&tr, lon);
    ob_chr(&tr, ',');
    ob_val_tmpl(&tr, lat);
    ob_chr(&tr, ')');
    el_attr_out(text, "transform", &tr);
    ob_free(&tr);

    El *t1 = el_append(text, "tspan");
    el_attr_str(t1, "y", "-0.7em");
    el_attr_str(t1, "font-weight", "bold");
    Out s1 = {0};
    ob_val_text(&s1, airport_field(&A, r, "iata"));   /* the column is iata_code: undefined */
    el_text(t1, s1.p, s1.n);
    ob_free(&s1);

    El *t2 = el_append(text, "tspan");
    el_attr_str(t2, "x", "0");
    el_attr_str(t2, "y", "0.7em");
    el_attr_str(t2, "fill-opacity", "0.7");
    Out s2 = {0};
    ob_val_text(&s2, airport_field(&A, r, "name"));
    el_text(t2, s2.p, s2.n);
    ob_free(&s2);
  }

  /* body.html(): the serialized children of body */
  ser_el(html, svg);

  delaunay_free(&D);
  free(V.circumcenters);
  free(V.vectors);
  free(A.cells);
  free(A.ubuf);
  for (size_t c = 0; c < A.ncols && c < 16; c++) free(A.header_text[c]);
  return root;
}

/* ================= quickHash (StartupBenchmark): UTF-16 code units from the end, step 919 ================= */
static size_t utf16_units(const char *s, size_t n) {
  size_t k = 0;
  for (size_t i = 0; i < n; i++) {
    unsigned char c = (unsigned char)s[i];
    if ((c & 0xC0) != 0x80) k += (c >= 0xF0) ? 2 : 1;
  }
  return k;
}
static uint16_t *utf16_decode(const char *s, size_t n, size_t *out_n) {
  uint16_t *u = malloc((utf16_units(s, n) + 1) * sizeof *u);
  if (!u) die("out of memory");
  size_t k = 0;
  for (size_t p = 0; p < n;) {
    unsigned char c = (unsigned char)s[p];
    uint32_t cp;
    size_t len;
    if (c < 0x80) { cp = c; len = 1; }
    else if (c < 0xE0) { cp = c & 0x1F; len = 2; }
    else if (c < 0xF0) { cp = c & 0x0F; len = 3; }
    else { cp = c & 0x07; len = 4; }
    for (size_t j = 1; j < len; j++) cp = (cp << 6) | ((unsigned char)s[p + j] & 0x3F);
    p += len;
    if (cp >= 0x10000) {
      cp -= 0x10000;
      u[k++] = (uint16_t)(0xD800 + (cp >> 10));
      u[k++] = (uint16_t)(0xDC00 + (cp & 0x3FF));
    } else {
      u[k++] = (uint16_t)cp;
    }
  }
  u[k] = 0;
  *out_n = k;
  return u;
}
static int32_t quick_hash(const char *s, size_t n, size_t *units) {
  size_t m;
  uint16_t *u = utf16_decode(s, n, &m);
  uint32_t h = 5381;
  for (long i = (long)m; i > 0; i -= 919) {
    uint32_t c = (i < (long)m) ? u[i] : 0;   /* charCodeAt(length) is NaN, and NaN | 0 is 0 */
    h = (h * 33u) ^ c;
  }
  free(u);
  *units = m;
  return (int32_t)h;
}

/* ================= bundle copies (StartupBenchmark's cache-busting) ================= */
static const char CACHE_BUST[] = "\n/*ThouShaltNotCache*/\n";
/* replaceAll of the cache-busting match (its newlines included) by an id comment */
static char *iteration_source(const char *bundle, size_t n, int cacheId, size_t *out_n) {
  char id[32];
  int idn = snprintf(id, sizeof id, "/*%d*/", cacheId);
  size_t cl = strlen(CACHE_BUST);
  char *out = malloc(n + 64);
  if (!out) die("out of memory");
  size_t o = 0;
  for (size_t i = 0; i < n;) {
    if (i + cl <= n && memcmp(bundle + i, CACHE_BUST, cl) == 0) {
      memcpy(out + o, id, (size_t)idn);
      o += (size_t)idn;
      i += cl;
    } else {
      out[o++] = bundle[i++];
    }
  }
  out[o] = 0;
  *out_n = o;
  return out;
}

/* ================= main ================= */
#define ROW_ITERATIONS 15
#define REUSE 2   /* sourceCodeReuseCount */

int main(int argc, char **argv) {
  int iterations = argc > 1 ? atoi(argv[1]) : ROW_ITERATIONS;
  const char *root = argc > 2 ? argv[2] : "../jetstream";
  char path[4096];
  size_t bn, un, cn;

  snprintf(path, sizeof path, "%s/jsdom-d3-startup/dist/bundle.min.js", root);
  char *bundle = js_read_file(path, &bn);
  snprintf(path, sizeof path, "%s/jsdom-d3-startup/data/counties-albers-10m.json", root);
  char *usText = js_read_file(path, &un);
  snprintf(path, sizeof path, "%s/jsdom-d3-startup/data/airports.csv", root);
  char *csv = js_read_file(path, &cn);

  /* init (untimed): the cache-busting copies, and JSON.parse of the US data once */
  int ncopies = (iterations + REUSE - 1) / REUSE;
  char **copies = malloc((size_t)(ncopies ? ncopies : 1) * sizeof *copies);
  size_t *copyN = malloc((size_t)(ncopies ? ncopies : 1) * sizeof *copyN);
  for (int c = 0; c < ncopies; c++) copies[c] = iteration_source(bundle, bn, c * REUSE, &copyN[c]);

  JDoc *doc = doc_new();
  JCounts jc = {0, 0, 0, 0};
  const JValue *usRoot = json_parse(doc, usText, un, &jc);
  const JValue *objects = jobj_get(usRoot, "objects");
  const JValue *states = jobj_get(objects, "states");
  const JValue *tf = jobj_get(usRoot, "transform");
  Topo T;
  T.arcs = jobj_get(usRoot, "arcs");
  T.kx = jnum(jobj_get(tf, "scale")->u.items[0]);
  T.ky = jnum(jobj_get(tf, "scale")->u.items[1]);
  T.dx = jnum(jobj_get(tf, "translate")->u.items[0]);
  T.dy = jnum(jobj_get(tf, "translate")->u.items[1]);

  /* the expected results, from the node run (tools/jsdom-d3-startup-validate.cjs) */
  static const size_t EXP_PATHS = 1895, EXP_NO_D = 1886, EXP_D = 9, EXP_TITLE = 1893, EXP_TEXT = 1893,
                      EXP_TSPAN = 3786, EXP_M = 264, EXP_L = 13006, EXP_Z = 205;

  JsContext ctx;
  js_context_init(&ctx);
  double total = 0;
  int ok = 1;
  size_t units = 0;
  int32_t hash = 0;
  for (int it = 0; it < iterations; it++) {
    size_t n = copyN[it / REUSE];
    const char *src = copies[it / REUSE];
    Out html = {0};
    double t0 = js_now_ms();
    size_t tokens = 0;
    js_parse(&ctx, src, n, 0, &tokens);       /* eval: the iteration's bundle is parsed */
    El *body = run_test(&T, states, csv, cn, &html);
    hash = quick_hash(html.p, html.n, &units);
    double t1 = js_now_ms();
    total += t1 - t0;
    /* validation, after the timer */
    Counts counts;
    memset(&counts, 0, sizeof counts);
    count_el(body->kid[0], &counts);
    el_free(body);
    if (units != EXPECTED_LEN || hash != EXPECTED_HASH) {
      fprintf(stderr, "jsdom-d3-startup: expected length %d and hash %d, got %zu and %d\n", EXPECTED_LEN, EXPECTED_HASH, units, hash);
      ok = 0;
    }
    if (counts.paths != EXP_PATHS || counts.pathsWithoutD != EXP_NO_D || counts.dAttrs != EXP_D ||
        counts.tag_title != EXP_TITLE || counts.tag_text != EXP_TEXT || counts.tag_tspan != EXP_TSPAN ||
        counts.M != EXP_M || counts.L != EXP_L || counts.Z != EXP_Z) {
      fprintf(stderr, "jsdom-d3-startup: element or path-command counts differ from node (paths %zu, no d %zu, d %zu, M %zu, L %zu, Z %zu)\n",
              counts.paths, counts.pathsWithoutD, counts.dAttrs, counts.M, counts.L, counts.Z);
      ok = 0;
    }
    ob_free(&html);
  }
  printf("jsdom-d3-startup %d iterations %.3f ms (average %.3f ms)\n", iterations, total, total / iterations);
  printf("validation %s: output %d UTF-16 units, quickHash %d, element and path-command counts as node\n", ok ? "ok" : "FAILED", EXPECTED_LEN, EXPECTED_HASH);

  js_context_destroy(&ctx);
  doc_free(doc);
  for (int c = 0; c < ncopies; c++) free(copies[c]);
  free(copies);
  free(copyN);
  free(bundle);
  free(usText);
  free(csv);
  return ok ? 0 : 1;
}
