// jetstream-row: Box2D
// jetstream-files: Octane/box2d.js
// iterations: 120
// jetstream-driver: deterministic=true
/* C analogue of JetStream's Octane/box2d.js: the same Box2D 2.2 world (static ground edge,
 * a 10x5 grid of 1x1 dynamic boxes at density 5, gravity -10), the same per-iteration work
 * (build the world, then 20 steps of dt = 1/60 with 10 velocity and 3 position iterations)
 * and the same engine structure: polygon-vs-polygon clipping collision with feature ids,
 * a dynamic AABB tree broadphase with fat AABBs and a move buffer, a contact manager with
 * a contact list per body, a sequential-impulse contact solver with 2-point block solving,
 * warm starting and position correction, island building and sleep timers.
 * Not ported: continuous collision (b2World::SolveTOI), joints, controllers and circle or
 * edge shapes, none of which the benchmark scene uses. Bodies, fixtures, contacts and the
 * tree are heap allocated per world, as the JS allocates them. */
#define _POSIX_C_SOURCE 200809L
#include <float.h>
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#define PI 3.14159265358979323846
#define LINEAR_SLOP 0.005
#define ANGULAR_SLOP (2.0 / 180.0 * PI)
#define POLYGON_RADIUS (2.0 * LINEAR_SLOP)
#define MAX_POLYGON_VERTICES 8
#define MAX_MANIFOLD_POINTS 2
#define AABB_EXTENSION 0.1
#define AABB_MULTIPLIER 2.0
#define VELOCITY_THRESHOLD 1.0
#define MAX_LINEAR_CORRECTION 0.2
#define MAX_ANGULAR_CORRECTION (8.0 / 180.0 * PI)
#define CONTACT_BAUMGARTE 0.2
#define TIME_TO_SLEEP 0.5
#define LINEAR_SLEEP_TOLERANCE 0.01
#define ANGULAR_SLEEP_TOLERANCE (2.0 / 180.0 * PI)
#define MAX_TRANSLATION 2.0
#define MAX_ROTATION (0.5 * PI)
#define MAX_CONDITION_NUMBER 1000.0
#define NULL_NODE (-1)
#define TREE_CAPACITY 256
#define PAIR_CAPACITY 4096
#define MAX_BODIES 256
#define MAX_CONTACTS 1024
#define MAX_CONSTRAINTS 1024
#define MAX_STACK 256

enum { BODY_STATIC = 0, BODY_DYNAMIC = 2 };
enum { FEATURE_VERTEX = 0, FEATURE_FACE = 1 };
enum { MANIFOLD_FACE_A = 1, MANIFOLD_FACE_B = 2 };

typedef struct { double x, y; } Vec2;
typedef struct { Vec2 col1, col2; } Mat22;
typedef struct { Mat22 R; Vec2 p; } Transform;
typedef struct { Vec2 lower, upper; } AABB;
typedef struct { Vec2 localCenter, c0, c; double a0, a; } Sweep;
typedef struct { double mass; Vec2 center; double I; } MassData;

static Vec2 vec(double x, double y) { Vec2 r; r.x = x; r.y = y; return r; }
static Vec2 vadd(Vec2 a, Vec2 b) { return vec(a.x + b.x, a.y + b.y); }
static Vec2 vsub(Vec2 a, Vec2 b) { return vec(a.x - b.x, a.y - b.y); }
static Vec2 vscale(double s, Vec2 a) { return vec(s * a.x, s * a.y); }
static double vdot(Vec2 a, Vec2 b) { return a.x * b.x + a.y * b.y; }
static double vcross(Vec2 a, Vec2 b) { return a.x * b.y - a.y * b.x; }
static double vlen(Vec2 a) { return sqrt(vdot(a, a)); }
static Vec2 crossVS(Vec2 v, double s) { return vec(s * v.y, -s * v.x); }
static Vec2 crossSV(double s, Vec2 v) { return vec(-s * v.y, s * v.x); }
static double clampd(double a, double lo, double hi) { return a < lo ? lo : (a > hi ? hi : a); }
static double mind(double a, double b) { return a < b ? a : b; }
static double maxd(double a, double b) { return a > b ? a : b; }

static Mat22 rotation(double angle) {
  Mat22 m;
  double c = cos(angle), s = sin(angle);
  m.col1 = vec(c, s);
  m.col2 = vec(-s, c);
  return m;
}
static Vec2 mulMV(const Mat22 *m, Vec2 v) {
  return vec(m->col1.x * v.x + m->col2.x * v.y, m->col1.y * v.x + m->col2.y * v.y);
}
static Vec2 mulTMV(const Mat22 *m, Vec2 v) { return vec(vdot(m->col1, v), vdot(m->col2, v)); }
static Vec2 xfMul(const Transform *t, Vec2 v) { return vadd(mulMV(&t->R, v), t->p); }
static Vec2 xfMulT(const Transform *t, Vec2 v) { return mulTMV(&t->R, vsub(v, t->p)); }
static Mat22 mat22_inverse(const Mat22 *m) {
  double a = m->col1.x, b = m->col2.x, c = m->col1.y, d = m->col2.y;
  double det = a * d - b * c;
  Mat22 r;
  if (det != 0.0) det = 1.0 / det;
  r.col1 = vec(det * d, -det * c);
  r.col2 = vec(-det * b, det * a);
  return r;
}

static AABB aabb_union(AABB a, AABB b) {
  AABB r;
  r.lower = vec(mind(a.lower.x, b.lower.x), mind(a.lower.y, b.lower.y));
  r.upper = vec(maxd(a.upper.x, b.upper.x), maxd(a.upper.y, b.upper.y));
  return r;
}
static double aabb_perimeter(AABB a) { return 2.0 * ((a.upper.x - a.lower.x) + (a.upper.y - a.lower.y)); }
static int aabb_overlap(AABB a, AABB b) {
  if (b.lower.x - a.upper.x > 0.0 || b.lower.y - a.upper.y > 0.0) return 0;
  if (a.lower.x - b.upper.x > 0.0 || a.lower.y - b.upper.y > 0.0) return 0;
  return 1;
}
static int aabb_contains(AABB a, AABB b) {
  return a.lower.x <= b.lower.x && a.lower.y <= b.lower.y && b.upper.x <= a.upper.x && b.upper.y <= a.upper.y;
}

/* ---- shapes ---- */
typedef struct {
  int count;
  Vec2 v[MAX_POLYGON_VERTICES];
  Vec2 n[MAX_POLYGON_VERTICES];
  double radius;
} Polygon;

static void polygon_set_as_box(Polygon *p, double hx, double hy) {
  p->count = 4;
  p->radius = POLYGON_RADIUS;
  p->v[0] = vec(-hx, -hy);
  p->v[1] = vec(hx, -hy);
  p->v[2] = vec(hx, hy);
  p->v[3] = vec(-hx, hy);
  p->n[0] = vec(0.0, -1.0);
  p->n[1] = vec(1.0, 0.0);
  p->n[2] = vec(0.0, 1.0);
  p->n[3] = vec(-1.0, 0.0);
}

static void polygon_set_as_edge(Polygon *p, Vec2 v1, Vec2 v2) {
  Vec2 e = vsub(v2, v1);
  e = vscale(1.0 / vlen(e), e);
  p->count = 2;
  p->radius = POLYGON_RADIUS;
  p->v[0] = v1;
  p->v[1] = v2;
  p->n[0] = crossVS(e, 1.0);
  p->n[1] = vscale(-1.0, p->n[0]);
}

static AABB polygon_aabb(const Polygon *p, const Transform *xf) {
  Vec2 v = xfMul(xf, p->v[0]);
  AABB r;
  r.lower = v;
  r.upper = v;
  for (int i = 1; i < p->count; i++) {
    v = xfMul(xf, p->v[i]);
    r.lower = vec(mind(r.lower.x, v.x), mind(r.lower.y, v.y));
    r.upper = vec(maxd(r.upper.x, v.x), maxd(r.upper.y, v.y));
  }
  r.lower = vec(r.lower.x - p->radius, r.lower.y - p->radius);
  r.upper = vec(r.upper.x + p->radius, r.upper.y + p->radius);
  return r;
}

/* Area, centroid and inertia about the body origin of the polygon, triangle fan from the origin. */
static void polygon_mass(const Polygon *p, double density, MassData *md) {
  Vec2 c = vec(0.0, 0.0);
  double area = 0.0, I = 0.0;
  const double inv3 = 1.0 / 3.0;
  for (int i = 0; i < p->count; i++) {
    Vec2 p2 = p->v[i];
    Vec2 p3 = p->v[(i + 1 < p->count) ? i + 1 : 0];
    double D = vcross(p2, p3);
    double triArea = 0.5 * D;
    area += triArea;
    c = vadd(c, vscale(triArea * inv3, vadd(p2, p3)));
    double x2 = p2.x, y2 = p2.y, x3 = p3.x, y3 = p3.y;
    I += (D / 12.0) * ((x2 * x2 + x2 * x3 + x3 * x3) + (y2 * y2 + y2 * y3 + y3 * y3));
  }
  md->mass = density * area;
  md->center = vscale(1.0 / area, c);
  md->I = density * I;
}

/* ---- collision: polygon vs polygon with reference face and clipping ---- */
typedef struct { uint8_t iA, iB, tA, tB; } Feature;
typedef struct { Vec2 v; Feature id; } ClipVertex;
typedef struct { Vec2 localPoint; double normalImpulse, tangentImpulse; Feature id; } ManifoldPoint;
typedef struct {
  int type, pointCount;
  Vec2 localNormal, localPoint;
  ManifoldPoint points[MAX_MANIFOLD_POINTS];
} Manifold;

static uint32_t feature_key(Feature f) {
  return (uint32_t)f.iA | ((uint32_t)f.iB << 8) | ((uint32_t)f.tA << 16) | ((uint32_t)f.tB << 24);
}

static double find_max_separation(int *edgeOut, const Polygon *p1, const Transform *xf1,
                                  const Polygon *p2, const Transform *xf2) {
  int bestIndex = 0;
  double maxSeparation = -DBL_MAX;
  for (int i = 0; i < p1->count; i++) {
    Vec2 n = mulMV(&xf1->R, p1->n[i]);
    Vec2 v1 = xfMul(xf1, p1->v[i]);
    double si = DBL_MAX;
    for (int j = 0; j < p2->count; j++) {
      Vec2 v2 = xfMul(xf2, p2->v[j]);
      double sij = vdot(n, vsub(v2, v1));
      if (sij < si) si = sij;
    }
    if (si > maxSeparation) {
      maxSeparation = si;
      bestIndex = i;
    }
  }
  *edgeOut = bestIndex;
  return maxSeparation;
}

static void find_incident_edge(ClipVertex c[2], const Polygon *p1, const Transform *xf1, int edge1,
                               const Polygon *p2, const Transform *xf2) {
  Vec2 normal1 = mulTMV(&xf2->R, mulMV(&xf1->R, p1->n[edge1]));
  int i1 = 0;
  double minDot = DBL_MAX;
  for (int i = 0; i < p2->count; i++) {
    double d = vdot(normal1, p2->n[i]);
    if (d < minDot) {
      minDot = d;
      i1 = i;
    }
  }
  int i2 = (i1 + 1 < p2->count) ? i1 + 1 : 0;
  c[0].v = xfMul(xf2, p2->v[i1]);
  c[0].id.iA = (uint8_t)edge1;
  c[0].id.iB = (uint8_t)i1;
  c[0].id.tA = FEATURE_FACE;
  c[0].id.tB = FEATURE_VERTEX;
  c[1].v = xfMul(xf2, p2->v[i2]);
  c[1].id.iA = (uint8_t)edge1;
  c[1].id.iB = (uint8_t)i2;
  c[1].id.tA = FEATURE_FACE;
  c[1].id.tB = FEATURE_VERTEX;
}

static int clip_segment_to_line(ClipVertex vout[2], const ClipVertex vin[2], Vec2 normal, double offset,
                                int vertexIndexA) {
  int numOut = 0;
  double d0 = vdot(normal, vin[0].v) - offset;
  double d1 = vdot(normal, vin[1].v) - offset;
  if (d0 <= 0.0) vout[numOut++] = vin[0];
  if (d1 <= 0.0) vout[numOut++] = vin[1];
  if (d0 * d1 < 0.0) {
    double interp = d0 / (d0 - d1);
    vout[numOut].v = vadd(vin[0].v, vscale(interp, vsub(vin[1].v, vin[0].v)));
    vout[numOut].id.iA = (uint8_t)vertexIndexA;
    vout[numOut].id.iB = vin[0].id.iB;
    vout[numOut].id.tA = FEATURE_VERTEX;
    vout[numOut].id.tB = FEATURE_FACE;
    numOut++;
  }
  return numOut;
}

static void collide_polygons(Manifold *m, const Polygon *polyA, const Transform *xfA,
                             const Polygon *polyB, const Transform *xfB) {
  m->pointCount = 0;
  double totalRadius = polyA->radius + polyB->radius;
  int edgeA = 0;
  double separationA = find_max_separation(&edgeA, polyA, xfA, polyB, xfB);
  if (separationA > totalRadius) return;
  int edgeB = 0;
  double separationB = find_max_separation(&edgeB, polyB, xfB, polyA, xfA);
  if (separationB > totalRadius) return;

  const Polygon *poly1, *poly2;
  const Transform *xf1, *xf2;
  int edge1, flip;
  const double kTol = 0.1 * LINEAR_SLOP;
  if (separationB > separationA + kTol) {
    flip = 1;
    poly1 = polyB; poly2 = polyA; xf1 = xfB; xf2 = xfA; edge1 = edgeB;
    m->type = MANIFOLD_FACE_B;
  } else {
    flip = 0;
    poly1 = polyA; poly2 = polyB; xf1 = xfA; xf2 = xfB; edge1 = edgeA;
    m->type = MANIFOLD_FACE_A;
  }

  ClipVertex incident[2];
  find_incident_edge(incident, poly1, xf1, edge1, poly2, xf2);

  int count1 = poly1->count;
  int iv1 = edge1;
  int iv2 = (edge1 + 1 < count1) ? edge1 + 1 : 0;
  Vec2 v11 = poly1->v[iv1];
  Vec2 v12 = poly1->v[iv2];
  Vec2 localTangent = vscale(1.0 / vlen(vsub(v12, v11)), vsub(v12, v11));
  Vec2 localNormal = crossVS(localTangent, 1.0);
  Vec2 planePoint = vscale(0.5, vadd(v11, v12));

  Vec2 tangent = mulMV(&xf1->R, localTangent);
  Vec2 normal = crossVS(tangent, 1.0);

  v11 = xfMul(xf1, v11);
  v12 = xfMul(xf1, v12);

  double frontOffset = vdot(normal, v11);
  double sideOffset1 = -vdot(tangent, v11) + totalRadius;
  double sideOffset2 = vdot(tangent, v12) + totalRadius;

  ClipVertex clip1[2], clip2[2];
  if (clip_segment_to_line(clip1, incident, vscale(-1.0, tangent), sideOffset1, iv1) < 2) return;
  if (clip_segment_to_line(clip2, clip1, tangent, sideOffset2, iv2) < 2) return;

  m->localNormal = localNormal;
  m->localPoint = planePoint;
  int pointCount = 0;
  for (int i = 0; i < MAX_MANIFOLD_POINTS; i++) {
    double separation = vdot(normal, clip2[i].v) - frontOffset;
    if (separation <= totalRadius) {
      ManifoldPoint *cp = &m->points[pointCount];
      cp->localPoint = xfMulT(xf2, clip2[i].v);
      cp->id = clip2[i].id;
      if (flip) {
        uint8_t t;
        t = cp->id.iA; cp->id.iA = cp->id.iB; cp->id.iB = t;
        t = cp->id.tA; cp->id.tA = cp->id.tB; cp->id.tB = t;
      }
      pointCount++;
    }
  }
  m->pointCount = pointCount;
}

/* Contact points in world space plus the normal from shape A to shape B. */
static void world_manifold(const Manifold *m, const Transform *xfA, double radiusA,
                           const Transform *xfB, double radiusB, Vec2 *normal, Vec2 *points) {
  if (m->type == MANIFOLD_FACE_A) {
    Vec2 nrm = mulMV(&xfA->R, m->localNormal);
    Vec2 plane = xfMul(xfA, m->localPoint);
    for (int i = 0; i < m->pointCount; i++) {
      Vec2 clip = xfMul(xfB, m->points[i].localPoint);
      Vec2 cA = vadd(clip, vscale(radiusA - vdot(vsub(clip, plane), nrm), nrm));
      Vec2 cB = vsub(clip, vscale(radiusB, nrm));
      points[i] = vscale(0.5, vadd(cA, cB));
    }
    *normal = nrm;
  } else {
    Vec2 nrm = mulMV(&xfB->R, m->localNormal);
    Vec2 plane = xfMul(xfB, m->localPoint);
    for (int i = 0; i < m->pointCount; i++) {
      Vec2 clip = xfMul(xfA, m->points[i].localPoint);
      Vec2 cB = vadd(clip, vscale(radiusB - vdot(vsub(clip, plane), nrm), nrm));
      Vec2 cA = vsub(clip, vscale(radiusA, nrm));
      points[i] = vscale(0.5, vadd(cA, cB));
    }
    *normal = vscale(-1.0, nrm);
  }
}

/* Separation and contact points used by the position solver, from the current sweeps. */
static void position_manifold(const Manifold *m, const Transform *xfA, double radiusA,
                              const Transform *xfB, double radiusB, Vec2 *normal, Vec2 *points, double *seps) {
  if (m->type == MANIFOLD_FACE_A) {
    Vec2 nrm = mulMV(&xfA->R, m->localNormal);
    Vec2 plane = xfMul(xfA, m->localPoint);
    for (int i = 0; i < m->pointCount; i++) {
      Vec2 clip = xfMul(xfB, m->points[i].localPoint);
      seps[i] = vdot(vsub(clip, plane), nrm) - radiusA - radiusB;
      points[i] = clip;
    }
    *normal = nrm;
  } else {
    Vec2 nrm = mulMV(&xfB->R, m->localNormal);
    Vec2 plane = xfMul(xfB, m->localPoint);
    for (int i = 0; i < m->pointCount; i++) {
      Vec2 clip = xfMul(xfA, m->points[i].localPoint);
      seps[i] = vdot(vsub(clip, plane), nrm) - radiusA - radiusB;
      points[i] = clip;
    }
    *normal = vscale(-1.0, nrm);
  }
}

/* ---- dynamic AABB tree ---- */
typedef struct Fixture Fixture;
typedef struct Contact Contact;
typedef struct Body Body;

typedef struct {
  AABB aabb;
  Fixture *userData;
  int parent, child1, child2, height;
} TreeNode;

typedef struct {
  TreeNode *nodes;
  int root, freeList;
} Tree;

typedef struct { int a, b; } Pair;

typedef struct {
  Tree tree;
  int moveBuffer[TREE_CAPACITY];
  int moveCount;
  Pair pairs[PAIR_CAPACITY];
  int pairCount;
} Broadphase;

static void tree_init(Tree *t) {
  t->nodes = malloc(TREE_CAPACITY * sizeof *t->nodes);
  for (int i = 0; i < TREE_CAPACITY; i++) {
    t->nodes[i].child1 = (i + 1 < TREE_CAPACITY) ? i + 1 : NULL_NODE;
    t->nodes[i].child2 = NULL_NODE;
    t->nodes[i].parent = NULL_NODE;
    t->nodes[i].height = -1;
    t->nodes[i].userData = NULL;
  }
  t->root = NULL_NODE;
  t->freeList = 0;
}

static int tree_alloc(Tree *t) {
  if (t->freeList == NULL_NODE) {
    fprintf(stderr, "Box2D: dynamic tree full\n");
    exit(1);
  }
  int id = t->freeList;
  TreeNode *n = &t->nodes[id];
  t->freeList = n->child1;
  n->parent = NULL_NODE;
  n->child1 = NULL_NODE;
  n->child2 = NULL_NODE;
  n->height = 0;
  n->userData = NULL;
  return id;
}

static void tree_free_node(Tree *t, int id) {
  t->nodes[id].child1 = t->freeList;
  t->nodes[id].height = -1;
  t->freeList = id;
}

static void tree_insert_leaf(Tree *t, int leaf) {
  TreeNode *nodes = t->nodes;
  if (t->root == NULL_NODE) {
    t->root = leaf;
    nodes[leaf].parent = NULL_NODE;
    return;
  }
  AABB leafAABB = nodes[leaf].aabb;
  int index = t->root;
  while (nodes[index].child1 != NULL_NODE) {
    int child1 = nodes[index].child1;
    int child2 = nodes[index].child2;
    double area = aabb_perimeter(nodes[index].aabb);
    double combinedArea = aabb_perimeter(aabb_union(nodes[index].aabb, leafAABB));
    double cost = 2.0 * combinedArea;
    double inheritanceCost = 2.0 * (combinedArea - area);
    double cost1, cost2;
    if (nodes[child1].child1 == NULL_NODE) {
      cost1 = aabb_perimeter(aabb_union(leafAABB, nodes[child1].aabb)) + inheritanceCost;
    } else {
      double oldArea = aabb_perimeter(nodes[child1].aabb);
      double newArea = aabb_perimeter(aabb_union(leafAABB, nodes[child1].aabb));
      cost1 = (newArea - oldArea) + inheritanceCost;
    }
    if (nodes[child2].child1 == NULL_NODE) {
      cost2 = aabb_perimeter(aabb_union(leafAABB, nodes[child2].aabb)) + inheritanceCost;
    } else {
      double oldArea = aabb_perimeter(nodes[child2].aabb);
      double newArea = aabb_perimeter(aabb_union(leafAABB, nodes[child2].aabb));
      cost2 = (newArea - oldArea) + inheritanceCost;
    }
    if (cost < cost1 && cost < cost2) break;
    index = (cost1 < cost2) ? child1 : child2;
  }

  int sibling = index;
  int oldParent = nodes[sibling].parent;
  int newParent = tree_alloc(t);
  nodes = t->nodes;
  nodes[newParent].parent = oldParent;
  nodes[newParent].userData = NULL;
  nodes[newParent].aabb = aabb_union(leafAABB, nodes[sibling].aabb);
  nodes[newParent].height = nodes[sibling].height + 1;
  if (oldParent != NULL_NODE) {
    if (nodes[oldParent].child1 == sibling) nodes[oldParent].child1 = newParent;
    else nodes[oldParent].child2 = newParent;
  } else {
    t->root = newParent;
  }
  nodes[newParent].child1 = sibling;
  nodes[newParent].child2 = leaf;
  nodes[sibling].parent = newParent;
  nodes[leaf].parent = newParent;

  index = nodes[leaf].parent;
  while (index != NULL_NODE) {
    int child1 = nodes[index].child1;
    int child2 = nodes[index].child2;
    nodes[index].height = 1 + ((nodes[child1].height > nodes[child2].height) ? nodes[child1].height : nodes[child2].height);
    nodes[index].aabb = aabb_union(nodes[child1].aabb, nodes[child2].aabb);
    index = nodes[index].parent;
  }
}

static void tree_remove_leaf(Tree *t, int leaf) {
  TreeNode *nodes = t->nodes;
  if (leaf == t->root) {
    t->root = NULL_NODE;
    return;
  }
  int parent = nodes[leaf].parent;
  int grandParent = nodes[parent].parent;
  int sibling = (nodes[parent].child1 == leaf) ? nodes[parent].child2 : nodes[parent].child1;
  if (grandParent != NULL_NODE) {
    if (nodes[grandParent].child1 == parent) nodes[grandParent].child1 = sibling;
    else nodes[grandParent].child2 = sibling;
    nodes[sibling].parent = grandParent;
    tree_free_node(t, parent);
    int index = grandParent;
    while (index != NULL_NODE) {
      int child1 = nodes[index].child1;
      int child2 = nodes[index].child2;
      nodes[index].aabb = aabb_union(nodes[child1].aabb, nodes[child2].aabb);
      nodes[index].height = 1 + ((nodes[child1].height > nodes[child2].height) ? nodes[child1].height : nodes[child2].height);
      index = nodes[index].parent;
    }
  } else {
    t->root = sibling;
    nodes[sibling].parent = NULL_NODE;
    tree_free_node(t, parent);
  }
}

static int tree_create_proxy(Tree *t, AABB aabb, Fixture *userData) {
  int id = tree_alloc(t);
  AABB r = aabb;
  r.lower = vec(r.lower.x - AABB_EXTENSION, r.lower.y - AABB_EXTENSION);
  r.upper = vec(r.upper.x + AABB_EXTENSION, r.upper.y + AABB_EXTENSION);
  t->nodes[id].aabb = r;
  t->nodes[id].userData = userData;
  tree_insert_leaf(t, id);
  return id;
}

static int tree_move_proxy(Tree *t, int proxy, AABB aabb, Vec2 displacement) {
  if (aabb_contains(t->nodes[proxy].aabb, aabb)) return 0;
  tree_remove_leaf(t, proxy);
  AABB b = aabb;
  b.lower = vec(b.lower.x - AABB_EXTENSION, b.lower.y - AABB_EXTENSION);
  b.upper = vec(b.upper.x + AABB_EXTENSION, b.upper.y + AABB_EXTENSION);
  Vec2 d = vscale(AABB_MULTIPLIER, displacement);
  if (d.x < 0.0) b.lower.x += d.x; else b.upper.x += d.x;
  if (d.y < 0.0) b.lower.y += d.y; else b.upper.y += d.y;
  t->nodes[proxy].aabb = b;
  tree_insert_leaf(t, proxy);
  return 1;
}

static void bp_buffer_move(Broadphase *bp, int proxy) {
  bp->moveBuffer[bp->moveCount++] = proxy;
}

static int bp_create_proxy(Broadphase *bp, AABB aabb, Fixture *userData) {
  int id = tree_create_proxy(&bp->tree, aabb, userData);
  bp_buffer_move(bp, id);
  return id;
}

static void bp_move_proxy(Broadphase *bp, int proxy, AABB aabb, Vec2 displacement) {
  if (tree_move_proxy(&bp->tree, proxy, aabb, displacement)) bp_buffer_move(bp, proxy);
}

/* For every moved proxy, query the tree with its fat AABB and buffer the overlapping pairs. */
static void bp_update_pairs(Broadphase *bp) {
  Tree *t = &bp->tree;
  bp->pairCount = 0;
  for (int k = 0; k < bp->moveCount; k++) {
    int m = bp->moveBuffer[k];
    AABB fat = t->nodes[m].aabb;
    int stack[MAX_STACK];
    int top = 0;
    stack[top++] = t->root;
    while (top > 0) {
      int id = stack[--top];
      if (id == NULL_NODE) continue;
      TreeNode *n = &t->nodes[id];
      if (!aabb_overlap(n->aabb, fat)) continue;
      if (n->child1 == NULL_NODE) {
        if (id == m) continue;
        if (bp->pairCount >= PAIR_CAPACITY) {
          fprintf(stderr, "Box2D: pair buffer full\n");
          exit(1);
        }
        Pair *p = &bp->pairs[bp->pairCount++];
        p->a = id < m ? id : m;
        p->b = id < m ? m : id;
      } else {
        stack[top++] = n->child1;
        stack[top++] = n->child2;
      }
    }
  }
  bp->moveCount = 0;
}

/* ---- bodies, fixtures, contacts ---- */
typedef struct ContactEdge {
  Body *other;
  Contact *contact;
  struct ContactEdge *prev, *next;
} ContactEdge;

struct Fixture {
  Body *body;
  Polygon shape;
  double density, friction, restitution;
  int proxyId;
  uint16_t categoryBits, maskBits;
  int groupIndex;
  Fixture *next;
};

struct Body {
  int type, awake, active, allowSleep, islandFlag;
  Transform xf;
  Sweep sweep;
  Vec2 linVel, force;
  double angVel, torque;
  double mass, invMass, I, invI;
  double linearDamping, angularDamping, sleepTime;
  Fixture *fixtures;
  int fixtureCount;
  ContactEdge *contacts;
  Body *next;
};

struct Contact {
  Fixture *fA, *fB;
  Body *bA, *bB;
  ContactEdge edgeA, edgeB;
  Manifold manifold;
  int touching, island;
  double friction, restitution;
  Contact *prev, *next;
};


static void body_sync_transform(Body *b) {
  b->xf.R = rotation(b->sweep.a);
  b->xf.p = vsub(b->sweep.c, mulMV(&b->xf.R, b->sweep.localCenter));
}

/* Transform from the sweep's current center and angle, as the position solver uses it. */
static Transform sweep_transform(const Sweep *s) {
  Transform xf;
  xf.R = rotation(s->a);
  xf.p = vsub(s->c, mulMV(&xf.R, s->localCenter));
  return xf;
}

static void body_set_awake(Body *b, int flag) {
  if (flag) {
    b->awake = 1;
    b->sleepTime = 0.0;
  } else {
    b->awake = 0;
    b->sleepTime = 0.0;
    b->linVel = vec(0.0, 0.0);
    b->angVel = 0.0;
    b->force = vec(0.0, 0.0);
    b->torque = 0.0;
  }
}

static void body_reset_mass(Body *b) {
  b->mass = 0.0;
  b->invMass = 0.0;
  b->I = 0.0;
  b->invI = 0.0;
  b->sweep.localCenter = vec(0.0, 0.0);
  if (b->type != BODY_DYNAMIC) {
    b->sweep.c0 = b->xf.p;
    b->sweep.c = b->xf.p;
    b->sweep.a0 = b->sweep.a;
    return;
  }
  Vec2 lc = vec(0.0, 0.0);
  for (Fixture *f = b->fixtures; f; f = f->next) {
    MassData md;
    polygon_mass(&f->shape, f->density, &md);
    b->mass += md.mass;
    lc = vadd(lc, vscale(md.mass, md.center));
    b->I += md.I;
  }
  if (b->mass > 0.0) {
    b->invMass = 1.0 / b->mass;
    lc = vscale(b->invMass, lc);
  }
  if (b->I > 0.0) {
    b->I -= b->mass * vdot(lc, lc);
    b->invI = 1.0 / b->I;
  }
  Vec2 oldCenter = b->sweep.c;
  b->sweep.localCenter = lc;
  b->sweep.c0 = xfMul(&b->xf, lc);
  b->sweep.c = b->sweep.c0;
  b->linVel = vadd(b->linVel, crossSV(b->angVel, vsub(b->sweep.c, oldCenter)));
}

typedef struct {
  double dt, invDt, dtRatio;
  int velocityIterations, positionIterations, warmStarting;
} TimeStep;

typedef struct {
  Vec2 rA, rB;
  double normalImpulse, tangentImpulse;
  double normalMass, tangentMass, velocityBias;
} VelPoint;

typedef struct {
  Contact *contact;
  Body *bA, *bB;
  Vec2 normal;
  double friction;
  int pointCount;
  VelPoint points[MAX_MANIFOLD_POINTS];
  Mat22 K, normalMass;
} VelConstraint;

typedef struct World {
  Vec2 gravity;
  Body *bodyList;
  int bodyCount;
  Contact *contactList;
  int contactCount;
  Broadphase *bp;
  int newFixture;
  double invDt0;
  Body *islandBodies[MAX_BODIES];
  int islandBodyCount;
  Contact *islandContacts[MAX_CONTACTS];
  int islandContactCount;
  Body *stack[MAX_BODIES];
  VelConstraint *constraints;
} World;

static Body *body_create(World *w, int type, Vec2 position) {
  Body *b = calloc(1, sizeof *b);
  b->type = type;
  b->awake = 1;
  b->active = 1;
  b->allowSleep = 1;
  b->xf.R = rotation(0.0);
  b->xf.p = position;
  b->sweep.localCenter = vec(0.0, 0.0);
  b->sweep.c0 = position;
  b->sweep.c = position;
  b->sweep.a0 = 0.0;
  b->sweep.a = 0.0;
  b->next = w->bodyList;
  w->bodyList = b;
  w->bodyCount++;
  return b;
}

static void bp_init_for_world(World *w) {
  w->bp = calloc(1, sizeof *w->bp);
  tree_init(&w->bp->tree);
  w->bp->moveCount = 0;
  w->bp->pairCount = 0;
}

static Fixture *fixture_create(World *w, Body *b, const Polygon *shape, double density, double friction) {
  Fixture *f = calloc(1, sizeof *f);
  f->body = b;
  f->shape = *shape;
  f->density = density;
  f->friction = friction;
  f->restitution = 0.0;
  f->categoryBits = 1;
  f->maskBits = 65535;
  f->groupIndex = 0;
  f->next = b->fixtures;
  b->fixtures = f;
  b->fixtureCount++;
  if (b->active) {
    AABB aabb = polygon_aabb(&f->shape, &b->xf);
    f->proxyId = bp_create_proxy(w->bp, aabb, f);
  } else {
    f->proxyId = NULL_NODE;
  }
  w->newFixture = 1;
  body_reset_mass(b);
  return f;
}

/* Fixture::Synchronize: refresh the proxy from the swept AABB of the body's motion this step. */
static void fixture_synchronize(Broadphase *bp, Fixture *f, const Transform *xf1, const Transform *xf2) {
  if (f->proxyId == NULL_NODE) return;
  AABB aabb1 = polygon_aabb(&f->shape, xf1);
  AABB aabb2 = polygon_aabb(&f->shape, xf2);
  AABB combined = aabb_union(aabb1, aabb2);
  Vec2 displacement = vsub(xf2->p, xf1->p);
  bp_move_proxy(bp, f->proxyId, combined, displacement);
}

static void body_synchronize_fixtures(World *w, Body *b) {
  Transform xf1;
  xf1.R = rotation(b->sweep.a0);
  xf1.p = vsub(b->sweep.c0, mulMV(&xf1.R, b->sweep.localCenter));
  for (Fixture *f = b->fixtures; f; f = f->next) fixture_synchronize(w->bp, f, &xf1, &b->xf);
}

static void contact_destroy(World *w, Contact *c) {
  if (c->prev) c->prev->next = c->next;
  if (c->next) c->next->prev = c->prev;
  if (c == w->contactList) w->contactList = c->next;
  ContactEdge *ea = &c->edgeA, *eb = &c->edgeB;
  if (ea->prev) ea->prev->next = ea->next;
  if (ea->next) ea->next->prev = ea->prev;
  if (ea == c->bA->contacts) c->bA->contacts = ea->next;
  if (eb->prev) eb->prev->next = eb->next;
  if (eb->next) eb->next->prev = eb->prev;
  if (eb == c->bB->contacts) c->bB->contacts = eb->next;
  w->contactCount--;
  free(c);
}

static void contact_update(Contact *c) {
  Manifold old = c->manifold;
  Transform xfA = c->bA->xf;
  Transform xfB = c->bB->xf;
  collide_polygons(&c->manifold, &c->fA->shape, &xfA, &c->fB->shape, &xfB);
  int touching = c->manifold.pointCount > 0;
  for (int i = 0; i < c->manifold.pointCount; i++) {
    ManifoldPoint *mp2 = &c->manifold.points[i];
    mp2->normalImpulse = 0.0;
    mp2->tangentImpulse = 0.0;
    uint32_t key = feature_key(mp2->id);
    for (int j = 0; j < old.pointCount; j++) {
      if (feature_key(old.points[j].id) == key) {
        mp2->normalImpulse = old.points[j].normalImpulse;
        mp2->tangentImpulse = old.points[j].tangentImpulse;
        break;
      }
    }
  }
  if (touching != c->touching) {
    body_set_awake(c->bA, 1);
    body_set_awake(c->bB, 1);
  }
  c->touching = touching;
}

/* ContactManager::AddPair: create a contact for a new broadphase pair if it should collide. */
static void add_pair(World *w, Fixture *fA, Fixture *fB) {
  Body *bA = fA->body, *bB = fB->body;
  if (bA == bB) return;
  for (ContactEdge *e = bB->contacts; e; e = e->next) {
    Contact *c = e->contact;
    if ((c->fA == fA && c->fB == fB) || (c->fA == fB && c->fB == fA)) return;
  }
  if (bA->type != BODY_DYNAMIC && bB->type != BODY_DYNAMIC) return;
  if (!((fA->maskBits & fB->categoryBits) != 0 && (fA->categoryBits & fB->maskBits) != 0)) return;
  if (fA->groupIndex == fB->groupIndex && fA->groupIndex != 0 && fA->groupIndex < 0) return;

  Contact *c = calloc(1, sizeof *c);
  c->fA = fA;
  c->fB = fB;
  c->bA = bA;
  c->bB = bB;
  c->friction = sqrt(fA->friction * fB->friction);
  c->restitution = maxd(fA->restitution, fB->restitution);
  c->edgeA.other = bB;
  c->edgeA.contact = c;
  c->edgeB.other = bA;
  c->edgeB.contact = c;

  c->prev = NULL;
  c->next = w->contactList;
  if (w->contactList) w->contactList->prev = c;
  w->contactList = c;
  w->contactCount++;

  c->edgeA.prev = NULL;
  c->edgeA.next = bA->contacts;
  if (bA->contacts) bA->contacts->prev = &c->edgeA;
  bA->contacts = &c->edgeA;

  c->edgeB.prev = NULL;
  c->edgeB.next = bB->contacts;
  if (bB->contacts) bB->contacts->prev = &c->edgeB;
  bB->contacts = &c->edgeB;
}

static void find_new_contacts(World *w) {
  Broadphase *bp = w->bp;
  bp_update_pairs(bp);
  for (int i = 0; i < bp->pairCount; i++) {
    Pair p = bp->pairs[i];
    add_pair(w, bp->tree.nodes[p.a].userData, bp->tree.nodes[p.b].userData);
  }
}

static void contact_collide(World *w) {
  Contact *c = w->contactList;
  while (c) {
    Contact *next = c->next;
    if (!c->bA->awake && !c->bB->awake) {
      c = next;
      continue;
    }
    AABB fatA = w->bp->tree.nodes[c->fA->proxyId].aabb;
    AABB fatB = w->bp->tree.nodes[c->fB->proxyId].aabb;
    if (!aabb_overlap(fatA, fatB)) {
      contact_destroy(w, c);
      c = next;
      continue;
    }
    contact_update(c);
    c = next;
  }
}

/* ---- contact solver ---- */
static void apply_impulse(Body *bA, Body *bB, Vec2 rA, Vec2 rB, Vec2 P) {
  bA->linVel = vsub(bA->linVel, vscale(bA->invMass, P));
  bA->angVel -= bA->invI * vcross(rA, P);
  bB->linVel = vadd(bB->linVel, vscale(bB->invMass, P));
  bB->angVel += bB->invI * vcross(rB, P);
}

static Vec2 rel_velocity(const Body *bA, const Body *bB, Vec2 rA, Vec2 rB) {
  return vsub(vsub(vadd(bB->linVel, crossSV(bB->angVel, rB)), bA->linVel), crossSV(bA->angVel, rA));
}

static void solver_init(VelConstraint *cons, Contact **contacts, int n) {
  for (int i = 0; i < n; i++) {
    Contact *c = contacts[i];
    VelConstraint *vc = &cons[i];
    Body *bA = c->bA, *bB = c->bB;
    const Manifold *m = &c->manifold;
    Vec2 wpts[MAX_MANIFOLD_POINTS];
    Vec2 normal;
    world_manifold(m, &bA->xf, c->fA->shape.radius, &bB->xf, c->fB->shape.radius, &normal, wpts);

    vc->contact = c;
    vc->bA = bA;
    vc->bB = bB;
    vc->normal = normal;
    vc->friction = c->friction;
    vc->pointCount = m->pointCount;
    double mA = bA->invMass, iA = bA->invI, mB = bB->invMass, iB = bB->invI;
    Vec2 tangent = crossVS(normal, 1.0);

    for (int j = 0; j < vc->pointCount; j++) {
      VelPoint *vp = &vc->points[j];
      vp->normalImpulse = m->points[j].normalImpulse;
      vp->tangentImpulse = m->points[j].tangentImpulse;
      vp->rA = vsub(wpts[j], bA->sweep.c);
      vp->rB = vsub(wpts[j], bB->sweep.c);
      double rnA = vcross(vp->rA, normal), rnB = vcross(vp->rB, normal);
      double kNormal = mA + mB + iA * rnA * rnA + iB * rnB * rnB;
      vp->normalMass = kNormal > 0.0 ? 1.0 / kNormal : 0.0;
      double rtA = vcross(vp->rA, tangent), rtB = vcross(vp->rB, tangent);
      double kTangent = mA + mB + iA * rtA * rtA + iB * rtB * rtB;
      vp->tangentMass = kTangent > 0.0 ? 1.0 / kTangent : 0.0;
      double vRel = vdot(normal, rel_velocity(bA, bB, vp->rA, vp->rB));
      vp->velocityBias = (vRel < -VELOCITY_THRESHOLD) ? -c->restitution * vRel : 0.0;
    }
    if (vc->pointCount == 2) {
      const VelPoint *p1 = &vc->points[0], *p2 = &vc->points[1];
      double rn1A = vcross(p1->rA, normal), rn1B = vcross(p1->rB, normal);
      double rn2A = vcross(p2->rA, normal), rn2B = vcross(p2->rB, normal);
      double k11 = mA + mB + iA * rn1A * rn1A + iB * rn1B * rn1B;
      double k22 = mA + mB + iA * rn2A * rn2A + iB * rn2B * rn2B;
      double k12 = mA + mB + iA * rn1A * rn2A + iB * rn1B * rn2B;
      if (k11 * k11 < MAX_CONDITION_NUMBER * (k11 * k22 - k12 * k12)) {
        vc->K.col1 = vec(k11, k12);
        vc->K.col2 = vec(k12, k22);
        vc->normalMass = mat22_inverse(&vc->K);
      } else {
        vc->pointCount = 1;
      }
    }
  }
}

static void solver_warm_start(VelConstraint *cons, int n, const TimeStep *step) {
  for (int i = 0; i < n; i++) {
    VelConstraint *vc = &cons[i];
    Body *bA = vc->bA, *bB = vc->bB;
    Vec2 tangent = crossVS(vc->normal, 1.0);
    for (int j = 0; j < vc->pointCount; j++) {
      VelPoint *vp = &vc->points[j];
      if (step->warmStarting) {
        vp->normalImpulse *= step->dtRatio;
        vp->tangentImpulse *= step->dtRatio;
        Vec2 P = vadd(vscale(vp->normalImpulse, vc->normal), vscale(vp->tangentImpulse, tangent));
        apply_impulse(bA, bB, vp->rA, vp->rB, P);
      } else {
        vp->normalImpulse = 0.0;
        vp->tangentImpulse = 0.0;
      }
    }
  }
}

/* Solves the 2x2 mini LCP of a two-point contact; returns 0 when no admissible case exists. */
static int block_solve2(const VelConstraint *vc, double b1, double b2, double *x1, double *x2) {
  const VelPoint *p1 = &vc->points[0], *p2 = &vc->points[1];
  const Mat22 *K = &vc->K, *M = &vc->normalMass;
  *x1 = -(M->col1.x * b1 + M->col2.x * b2);
  *x2 = -(M->col1.y * b1 + M->col2.y * b2);
  if (*x1 >= 0.0 && *x2 >= 0.0) return 1;
  *x1 = -p1->normalMass * b1;
  *x2 = 0.0;
  if (*x1 >= 0.0 && (K->col1.y * *x1 + b2) >= 0.0) return 1;
  *x1 = 0.0;
  *x2 = -p2->normalMass * b2;
  if (*x2 >= 0.0 && (K->col2.x * *x2 + b1) >= 0.0) return 1;
  *x1 = 0.0;
  *x2 = 0.0;
  return b1 >= 0.0 && b2 >= 0.0;
}

static void solver_solve_velocity(VelConstraint *cons, int n) {
  for (int i = 0; i < n; i++) {
    VelConstraint *vc = &cons[i];
    Body *bA = vc->bA, *bB = vc->bB;
    Vec2 normal = vc->normal;
    Vec2 tangent = crossVS(normal, 1.0);

    /* Friction first: non-penetration is more important than friction. */
    for (int j = 0; j < vc->pointCount; j++) {
      VelPoint *vp = &vc->points[j];
      double vt = vdot(rel_velocity(bA, bB, vp->rA, vp->rB), tangent);
      double lambda = -vp->tangentMass * vt;
      double maxFriction = vc->friction * vp->normalImpulse;
      double newImpulse = clampd(vp->tangentImpulse + lambda, -maxFriction, maxFriction);
      double delta = newImpulse - vp->tangentImpulse;
      vp->tangentImpulse = newImpulse;
      apply_impulse(bA, bB, vp->rA, vp->rB, vscale(delta, tangent));
    }

    if (vc->pointCount == 1) {
      VelPoint *vp = &vc->points[0];
      double vn = vdot(rel_velocity(bA, bB, vp->rA, vp->rB), normal);
      double lambda = -vp->normalMass * (vn - vp->velocityBias);
      double newImpulse = maxd(vp->normalImpulse + lambda, 0.0);
      double delta = newImpulse - vp->normalImpulse;
      vp->normalImpulse = newImpulse;
      apply_impulse(bA, bB, vp->rA, vp->rB, vscale(delta, normal));
    } else {
      VelPoint *p1 = &vc->points[0], *p2 = &vc->points[1];
      double vn1 = vdot(rel_velocity(bA, bB, p1->rA, p1->rB), normal);
      double vn2 = vdot(rel_velocity(bA, bB, p2->rA, p2->rB), normal);
      double a1 = p1->normalImpulse, a2 = p2->normalImpulse;
      double b1 = vn1 - p1->velocityBias - (vc->K.col1.x * a1 + vc->K.col2.x * a2);
      double b2 = vn2 - p2->velocityBias - (vc->K.col1.y * a1 + vc->K.col2.y * a2);
      double x1, x2;
      if (block_solve2(vc, b1, b2, &x1, &x2)) {
        Vec2 P1 = vscale(x1 - a1, normal);
        Vec2 P2 = vscale(x2 - a2, normal);
        apply_impulse(bA, bB, p1->rA, p1->rB, P1);
        apply_impulse(bA, bB, p2->rA, p2->rB, P2);
        p1->normalImpulse = x1;
        p2->normalImpulse = x2;
      }
    }
  }
}

static void solver_store_impulses(VelConstraint *cons, int n) {
  for (int i = 0; i < n; i++) {
    VelConstraint *vc = &cons[i];
    for (int j = 0; j < vc->pointCount; j++) {
      vc->contact->manifold.points[j].normalImpulse = vc->points[j].normalImpulse;
      vc->contact->manifold.points[j].tangentImpulse = vc->points[j].tangentImpulse;
    }
  }
}

/* Returns 1 when every contact's minimum separation is within tolerance. */
static int solver_solve_position(VelConstraint *cons, int n) {
  double minSeparation = 0.0;
  for (int i = 0; i < n; i++) {
    VelConstraint *vc = &cons[i];
    Body *bA = vc->bA, *bB = vc->bB;
    const Manifold *m = &vc->contact->manifold;
    Transform xfA = sweep_transform(&bA->sweep);
    Transform xfB = sweep_transform(&bB->sweep);
    Vec2 normal;
    Vec2 pts[MAX_MANIFOLD_POINTS];
    double seps[MAX_MANIFOLD_POINTS];
    position_manifold(m, &xfA, vc->contact->fA->shape.radius, &xfB, vc->contact->fB->shape.radius,
                      &normal, pts, seps);
    double mA = bA->invMass, iA = bA->invI, mB = bB->invMass, iB = bB->invI;
    for (int j = 0; j < vc->pointCount; j++) {
      Vec2 point = pts[j];
      double sep = seps[j];
      Vec2 rA = vsub(point, bA->sweep.c);
      Vec2 rB = vsub(point, bB->sweep.c);
      minSeparation = mind(minSeparation, sep);
      double C = clampd(CONTACT_BAUMGARTE * (sep + LINEAR_SLOP), -MAX_LINEAR_CORRECTION, 0.0);
      double rnA = vcross(rA, normal), rnB = vcross(rB, normal);
      double K = mA + mB + iA * rnA * rnA + iB * rnB * rnB;
      double impulse = K > 0.0 ? -C / K : 0.0;
      Vec2 P = vscale(impulse, normal);
      bA->sweep.c = vsub(bA->sweep.c, vscale(mA, P));
      bA->sweep.a -= iA * vcross(rA, P);
      bB->sweep.c = vadd(bB->sweep.c, vscale(mB, P));
      bB->sweep.a += iB * vcross(rB, P);
    }
  }
  return minSeparation >= -3.0 * LINEAR_SLOP;
}

/* b2Island::Solve: integrate velocities, solve contacts, integrate positions, solve
 * positions, then update sleep timers. */
static void island_solve(World *w, const TimeStep *step, Vec2 gravity, int allowSleep) {
  int nb = w->islandBodyCount, nc = w->islandContactCount;
  double dt = step->dt;
  for (int i = 0; i < nb; i++) {
    Body *b = w->islandBodies[i];
    if (b->type == BODY_DYNAMIC) {
      b->linVel.x += dt * (gravity.x + b->invMass * b->force.x);
      b->linVel.y += dt * (gravity.y + b->invMass * b->force.y);
      b->angVel += dt * b->invI * b->torque;
      b->linVel = vscale(clampd(1.0 - dt * b->linearDamping, 0.0, 1.0), b->linVel);
      b->angVel *= clampd(1.0 - dt * b->angularDamping, 0.0, 1.0);
    }
  }

  solver_init(w->constraints, w->islandContacts, nc);
  solver_warm_start(w->constraints, nc, step);
  for (int it = 0; it < step->velocityIterations; it++) solver_solve_velocity(w->constraints, nc);
  solver_store_impulses(w->constraints, nc);

  for (int i = 0; i < nb; i++) {
    Body *b = w->islandBodies[i];
    if (b->type == BODY_STATIC) continue;
    Vec2 translation = vscale(dt, b->linVel);
    if (vdot(translation, translation) > MAX_TRANSLATION * MAX_TRANSLATION) {
      double len = vlen(b->linVel);
      b->linVel = vscale(MAX_TRANSLATION * step->invDt / len, b->linVel);
    }
    double rotation_step = dt * b->angVel;
    if (rotation_step * rotation_step > MAX_ROTATION * MAX_ROTATION) {
      b->angVel = b->angVel < 0.0 ? -MAX_ROTATION * step->invDt : MAX_ROTATION * step->invDt;
    }
    b->sweep.c0 = b->sweep.c;
    b->sweep.a0 = b->sweep.a;
    b->sweep.c = vadd(b->sweep.c, vscale(dt, b->linVel));
    b->sweep.a += dt * b->angVel;
    body_sync_transform(b);
  }

  for (int it = 0; it < step->positionIterations; it++) {
    if (solver_solve_position(w->constraints, nc)) break;
  }
  for (int i = 0; i < nb; i++) {
    Body *b = w->islandBodies[i];
    if (b->type != BODY_STATIC) body_sync_transform(b);
  }

  if (allowSleep) {
    double minSleepTime = DBL_MAX;
    double linTol2 = LINEAR_SLEEP_TOLERANCE * LINEAR_SLEEP_TOLERANCE;
    double angTol2 = ANGULAR_SLEEP_TOLERANCE * ANGULAR_SLEEP_TOLERANCE;
    for (int i = 0; i < nb; i++) {
      Body *b = w->islandBodies[i];
      if (b->type == BODY_STATIC) continue;
      if (!b->allowSleep || b->angVel * b->angVel > angTol2 || vdot(b->linVel, b->linVel) > linTol2) {
        b->sleepTime = 0.0;
        minSleepTime = 0.0;
      } else {
        b->sleepTime += dt;
        minSleepTime = mind(minSleepTime, b->sleepTime);
      }
    }
    if (minSleepTime >= TIME_TO_SLEEP) {
      for (int i = 0; i < nb; i++) body_set_awake(w->islandBodies[i], 0);
    }
  }
}

/* b2World::Solve: build each awake island by a depth-first walk over touching contacts,
 * solve it, then synchronize fixtures and look for new pairs. */
static void world_solve(World *w, const TimeStep *step) {
  for (Body *b = w->bodyList; b; b = b->next) b->islandFlag = 0;
  for (Contact *c = w->contactList; c; c = c->next) c->island = 0;

  for (Body *seed = w->bodyList; seed; seed = seed->next) {
    if (seed->islandFlag) continue;
    if (!seed->awake || !seed->active) continue;
    if (seed->type == BODY_STATIC) continue;

    w->islandBodyCount = 0;
    w->islandContactCount = 0;
    int top = 0;
    w->stack[top++] = seed;
    seed->islandFlag = 1;
    while (top > 0) {
      Body *b = w->stack[--top];
      w->islandBodies[w->islandBodyCount++] = b;
      b->awake = 1;
      if (b->type == BODY_STATIC) continue;
      for (ContactEdge *e = b->contacts; e; e = e->next) {
        Contact *c = e->contact;
        if (c->island) continue;
        if (!c->touching) continue;
        c->island = 1;
        w->islandContacts[w->islandContactCount++] = c;
        Body *other = e->other;
        if (other->islandFlag) continue;
        w->stack[top++] = other;
        other->islandFlag = 1;
      }
    }
    island_solve(w, step, w->gravity, 1);
  }

  for (Body *b = w->bodyList; b; b = b->next) {
    if (!b->awake || !b->active) continue;
    if (b->type == BODY_STATIC) continue;
    body_synchronize_fixtures(w, b);
  }
  find_new_contacts(w);
}

static void world_step(World *w, double dt, int velocityIterations, int positionIterations) {
  TimeStep step;
  step.dt = dt;
  step.velocityIterations = velocityIterations;
  step.positionIterations = positionIterations;
  step.invDt = dt > 0.0 ? 1.0 / dt : 0.0;
  step.dtRatio = w->invDt0 * dt;
  step.warmStarting = 1;

  if (w->newFixture) {
    find_new_contacts(w);
    w->newFixture = 0;
  }
  contact_collide(w);
  if (step.dt > 0.0) world_solve(w, &step);
  if (step.dt > 0.0) w->invDt0 = step.invDt;
}

static World *world_create(Vec2 gravity) {
  World *w = calloc(1, sizeof *w);
  w->gravity = gravity;
  bp_init_for_world(w);
  w->constraints = malloc(MAX_CONSTRAINTS * sizeof *w->constraints);
  return w;
}

static void world_destroy(World *w) {
  for (Contact *c = w->contactList; c;) {
    Contact *next = c->next;
    free(c);
    c = next;
  }
  for (Body *b = w->bodyList; b;) {
    Body *next = b->next;
    for (Fixture *f = b->fixtures; f;) {
      Fixture *fnext = f->next;
      free(f);
      f = fnext;
    }
    free(b);
    b = next;
  }
  free(w->bp->tree.nodes);
  free(w->bp);
  free(w->constraints);
  free(w);
}

/* MakeNewWorld from box2d.js: static ground edge and a 10 x 5 grid of boxes. */
static World *make_new_world(void) {
  World *world = world_create(vec(0.0, -10.0));

  Body *ground = body_create(world, BODY_STATIC, vec(0.0, 0.0));
  Polygon edge;
  polygon_set_as_edge(&edge, vec(-40.0, 0.0), vec(40.0, 0.0));
  fixture_create(world, ground, &edge, 0.0, 0.2);

  Polygon box;
  polygon_set_as_box(&box, 0.5, 0.5);
  Vec2 x = vec(-7.0, 0.75);
  Vec2 deltaX = vec(0.5625, 1.0);
  Vec2 deltaY = vec(1.125, 0.0);
  for (int i = 0; i < 10; ++i) {
    Vec2 y = x;
    for (int j = 0; j < 5; ++j) {
      Body *body = body_create(world, BODY_DYNAMIC, y);
      fixture_create(world, body, &box, 5.0, 0.2);
      y = vadd(y, deltaY);
    }
    x = vadd(x, deltaX);
  }
  return world;
}

/* runBox2D: build the world and step it 20 times. Returns a checksum over body poses. */
static double run_box2d(void) {
  World *world = make_new_world();
  for (int i = 0; i < 20; i++) world_step(world, 1.0 / 60.0, 10, 3);
  double sum = 0.0;
  for (Body *b = world->bodyList; b; b = b->next) {
    if (b->type == BODY_STATIC) continue;
    sum += b->sweep.c.x + 2.0 * b->sweep.c.y + b->sweep.a;
  }
  world_destroy(world);
  return sum;
}

int main(int argc, char **argv) {
  int iterations = argc > 1 ? atoi(argv[1]) : 120;
  struct timespec a, b;
  double checksum = 0.0;
  clock_gettime(CLOCK_MONOTONIC, &a);
  for (int i = 0; i < iterations; i++) checksum = run_box2d();
  clock_gettime(CLOCK_MONOTONIC, &b);
  printf("Box2D %d iterations %.3f ms\n", iterations, (b.tv_sec - a.tv_sec) * 1e3 + (b.tv_nsec - a.tv_nsec) / 1e6);
  printf("checksum %.9f\n", checksum);
  if (!isfinite(checksum)) {
    fprintf(stderr, "Box2D: validation failed, non-finite body state\n");
    return 1;
  }
  return 0;
}
