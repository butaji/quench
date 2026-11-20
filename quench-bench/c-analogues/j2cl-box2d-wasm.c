// jetstream-row: j2cl-box2d-wasm
// jetstream-files: wasm/j2cl-box2d/benchmark.js wasm/j2cl-box2d/build/Box2dBenchmark_j2wasm_entry.js
// iterations: 40
// jetstream-driver: class=Async preload=wasmBinary=wasm/j2cl-box2d/build/Box2dBenchmark_j2wasm_binary.wasm
/* C analogue of the J2CL Box2D row (benchmarking/java/.../octane/Box2dBenchmark.java on the
 * jbox2d library from google/j2cl samples/box2d). Each iteration runs the JetStream driver's
 * runFixedCount(5): five times { build a fresh world, 20 steps of world.step(1/60, 10, 3) }.
 *
 * Ported from the Java sources, with float arithmetic in the same operation order:
 *   - org.jbox2d.common.MathUtils: sin/cos through the 57k-entry sincos LUT (Settings
 *     SINCOS_LUT_ENABLED), round/floor (FAST_ROUND/FAST_FLOOR), sqrt, Vec2/Rot/Transform/Mat22.
 *   - PolygonShape (setAsBox, computeAABB, computeMass) and EdgeShape (set, computeAABB).
 *   - Collision.collidePolygons (reference face, findMaxSeparation, clipSegmentToLine) and
 *     Collision.collideEdgeAndPolygon (EPCollider) for the ground edge against the boxes.
 *   - DynamicTree (insertLeaf, removeLeaf, balance, fat AABBs with extension, moveProxy with
 *     jbox2d's test), BroadPhase (move buffer, pair buffer sorted by proxy ids, dedupe).
 *   - ContactManager (addPair with registers, collide, destroy), Contact.update with
 *     ContactID-matched warm starting, Body/Fixture create, resetMassData, sync, advance.
 *   - World.step: findNewContacts, collide, solve (island DFS, sleep timers, sequential
 *     impulses with 2-point block solve, warm starting, Baumgarte position correction),
 *     synchronizeFixtures, and solveTOI (continuous collision: TimeOfImpact + GJK Distance,
 *     TOI island, TOI position solve, 20 position iterations), which jbox2d runs by default.
 * Not ported (not reached by this scene): joints, circle and chain shapes, EdgeShape ghost
 * vertices (vertex0/vertex3 branches of EPCollider; EdgeShape.set never sets them), contact
 * listeners and filters beyond the default ContactFilter, ray casts and AABB queries,
 * joint-related island parts, subStepping (World default off), object pooling (contacts are
 * malloc'd per creation), profiling timers, and the JS-side driver. The sin/cos LUT and the
 * float ops follow jbox2d's order exactly, so results are expected to match the Java run
 * (the checksum below was produced by compiling the j2cl samples/box2d sources with javac). */

#define _POSIX_C_SOURCE 200809L
#include <float.h>
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

/* ---------------- org.jbox2d.common.Settings ---------------- */
#define JB_PI ((float)3.14159265358979323846)
#define TWOPI_F ((float)(3.14159265358979323846 * 2))
#define HALF_PI_F (JB_PI / 2)
#define JB_EPSILON 1.1920928955078125E-7f
#define LINEAR_SLOP 0.005f
#define ANGULAR_SLOP (2.0f / 180.0f * JB_PI)
#define POLYGON_RADIUS (2.0f * LINEAR_SLOP)
#define AABB_EXTENSION 0.1f
#define AABB_MULTIPLIER 2.0f
#define MAX_POLYGON_VERTICES 8
#define MAX_MANIFOLD_POINTS 2
#define MAX_SUB_STEPS 8
#define MAX_TOI_CONTACTS 32
#define VELOCITY_THRESHOLD 1.0f
#define MAX_LINEAR_CORRECTION 0.2f
#define BAUMGARTE 0.2f
#define TOI_BAUMGARTE 0.75f
#define MAX_TRANSLATION 2.0f
#define MAX_TRANSLATION_SQ (MAX_TRANSLATION * MAX_TRANSLATION)
#define MAX_ROTATION (0.5f * JB_PI)
#define MAX_ROTATION_SQ (MAX_ROTATION * MAX_ROTATION)
#define TIME_TO_SLEEP 0.5f
#define LINEAR_SLEEP_TOL 0.01f
#define ANGULAR_SLEEP_TOL (2.0f / 180.0f * JB_PI)
#define SINCOS_LUT_PRECISION 0.00011f
#define GJK_MAX_ITERS 20
#define TOI_MAX_ITERATIONS 1000
#define TOI_TARGET_ROOT_ITERS 50

/* Benchmark parameters (Box2dBenchmark.java) */
#define NUMBER_OF_STEPS 20
#define INTERNAL_ITERATIONS 5
#define DEFAULT_ITERATIONS 40
#define CHECKSUM_EXPECTED 136.105121
#define CHECKSUM_TOLERANCE 1e-3
#define MIN_ALLOWED_Y 0.45

/* ---------------- float math (org.jbox2d.common.MathUtils) ---------------- */
static int lut_len;
static float *lut_sin;

static void lut_init(void)
{
    lut_len = (int)ceil((3.14159265358979323846 * 2) / (double)SINCOS_LUT_PRECISION);
    lut_sin = (float *)malloc(sizeof(float) * (size_t)lut_len);
    for (int i = 0; i < lut_len; i++)
        lut_sin[i] = (float)sin((double)((float)i * SINCOS_LUT_PRECISION));
}

static int jb_floor(float x)
{
    int y = (int)x;
    if (x < 0 && x != y)
        return y - 1;
    return y;
}

static int jb_round(float x) { return jb_floor(x + .5f); }
static float jb_min(float a, float b) { return a < b ? a : b; }
static float jb_max(float a, float b) { return a > b ? a : b; }
static float jb_abs(float x) { return x > 0 ? x : -x; }
static float jb_clamp(float a, float lo, float hi) { return jb_max(lo, jb_min(a, hi)); }

static float jb_sin(float x)
{
    x = fmodf(x, TWOPI_F);
    if (x < 0)
        x += TWOPI_F;
    return lut_sin[jb_round(x / SINCOS_LUT_PRECISION) % lut_len];
}

static float jb_cos(float x) { return jb_sin(HALF_PI_F - x); }

/* ---------------- Vec2, Rot, Transform, Mat22, AABB, Sweep ---------------- */
typedef struct { float x, y; } Vec2;
typedef struct { float s, c; } Rot;
typedef struct { Vec2 p; Rot q; } Transform;
typedef struct { Vec2 ex, ey; } Mat22;
typedef struct { Vec2 lo, up; } AABB;
typedef struct { Vec2 localCenter, c0, c; float a0, a, alpha0; } Sweep;
typedef struct { float mass; Vec2 center; float I; } MassData;

static Vec2 V2(float x, float y) { Vec2 r; r.x = x; r.y = y; return r; }
static Vec2 v2add(Vec2 a, Vec2 b) { return V2(a.x + b.x, a.y + b.y); }
static Vec2 v2sub(Vec2 a, Vec2 b) { return V2(a.x - b.x, a.y - b.y); }
static Vec2 v2scale(Vec2 a, float s) { return V2(a.x * s, a.y * s); }
static float v2dot(Vec2 a, Vec2 b) { return a.x * b.x + a.y * b.y; }
static float v2cross(Vec2 a, Vec2 b) { return a.x * b.y - a.y * b.x; }
static Vec2 v2crossVS(Vec2 a, float s) { return V2(s * a.y, -s * a.x); }
static Vec2 v2crossSV(float s, Vec2 a) { return V2(-s * a.y, s * a.x); }
static float v2lenSq(Vec2 a) { return a.x * a.x + a.y * a.y; }

/* Vec2.normalize: returns the length, leaves tiny vectors untouched. */
static float v2normalize(Vec2 *v)
{
    float length = sqrtf(v->x * v->x + v->y * v->y);
    if (length < JB_EPSILON)
        return 0.0f;
    float invLength = 1.0f / length;
    v->x *= invLength;
    v->y *= invLength;
    return length;
}

static Rot rot_set(float angle)
{
    Rot r;
    r.s = jb_sin(angle);
    r.c = jb_cos(angle);
    return r;
}

static Vec2 rot_mulV(Rot q, Vec2 v) { return V2(q.c * v.x - q.s * v.y, q.s * v.x + q.c * v.y); }
static Vec2 rot_mulTV(Rot q, Vec2 v) { return V2(q.c * v.x + q.s * v.y, -q.s * v.x + q.c * v.y); }

static Rot rot_mulT(Rot q, Rot r)
{
    Rot o;
    o.s = q.c * r.s - q.s * r.c;
    o.c = q.c * r.c + q.s * r.s;
    return o;
}

static Vec2 xf_mulV(const Transform *T, Vec2 v)
{
    Vec2 r;
    r.x = (T->q.c * v.x - T->q.s * v.y) + T->p.x;
    r.y = (T->q.s * v.x + T->q.c * v.y) + T->p.y;
    return r;
}

static Vec2 xf_mulTV(const Transform *T, Vec2 v)
{
    float px = v.x - T->p.x;
    float py = v.y - T->p.y;
    return V2(T->q.c * px + T->q.s * py, -T->q.s * px + T->q.c * py);
}

/* Transform.mulTransToOutUnsafe(A, B): A^T * B */
static Transform xf_mulT(const Transform *A, const Transform *B)
{
    Transform o;
    o.q = rot_mulT(A->q, B->q);
    Vec2 pool = v2sub(B->p, A->p);
    o.p = rot_mulTV(A->q, pool);
    return o;
}

static Vec2 mat22_mulV(const Mat22 *R, Vec2 v)
{
    return V2(R->ex.x * v.x + R->ey.x * v.y, R->ex.y * v.x + R->ey.y * v.y);
}

static Mat22 mat22_invert(const Mat22 *m)
{
    float a = m->ex.x, b = m->ey.x, c = m->ex.y, d = m->ey.y;
    float det = a * d - b * c;
    det = 1.0f / det;
    Mat22 o;
    o.ex.x = det * d;
    o.ey.x = -det * b;
    o.ex.y = -det * c;
    o.ey.y = det * a;
    return o;
}

static AABB aabb_combine(AABB a, AABB b)
{
    AABB o;
    o.lo.x = a.lo.x < b.lo.x ? a.lo.x : b.lo.x;
    o.lo.y = a.lo.y < b.lo.y ? a.lo.y : b.lo.y;
    o.up.x = a.up.x > b.up.x ? a.up.x : b.up.x;
    o.up.y = a.up.y > b.up.y ? a.up.y : b.up.y;
    return o;
}

static float aabb_perimeter(AABB a) { return 2.0f * (a.up.x - a.lo.x + a.up.y - a.lo.y); }

static int aabb_testOverlap(AABB a, AABB b)
{
    if (b.lo.x - a.up.x > 0.0f || b.lo.y - a.up.y > 0.0f)
        return 0;
    if (a.lo.x - b.up.x > 0.0f || a.lo.y - b.up.y > 0.0f)
        return 0;
    return 1;
}

static void sweep_normalize(Sweep *s)
{
    float d = TWOPI_F * (float)jb_floor(s->a0 / TWOPI_F);
    s->a0 -= d;
    s->a -= d;
}

static Transform sweep_getTransform(const Sweep *s, float beta)
{
    Transform xf;
    xf.p.x = (1.0f - beta) * s->c0.x + beta * s->c.x;
    xf.p.y = (1.0f - beta) * s->c0.y + beta * s->c.y;
    xf.q = rot_set((1.0f - beta) * s->a0 + beta * s->a);
    xf.p.x -= xf.q.c * s->localCenter.x - xf.q.s * s->localCenter.y;
    xf.p.y -= xf.q.s * s->localCenter.x + xf.q.c * s->localCenter.y;
    return xf;
}

static void sweep_advance(Sweep *s, float alpha)
{
    s->c0.x = (1.0f - alpha) * s->c0.x + alpha * s->c.x;
    s->c0.y = (1.0f - alpha) * s->c0.y + alpha * s->c.y;
    s->a0 = (1.0f - alpha) * s->a0 + alpha * s->a;
}

/* ---------------- shapes (PolygonShape, EdgeShape) ---------------- */
enum { SHAPE_POLYGON = 0, SHAPE_EDGE = 1 };

typedef struct {
    int type;
    float radius;
    /* polygon */
    int count;
    Vec2 v[MAX_POLYGON_VERTICES];
    Vec2 n[MAX_POLYGON_VERTICES];
    Vec2 centroid;
    /* edge */
    Vec2 v1, v2, v0, v3;
    int hasV0, hasV3;
} Shape;

static void shape_setAsBox(Shape *s, float hx, float hy)
{
    s->count = 4;
    s->v[0] = V2(-hx, -hy);
    s->v[1] = V2(hx, -hy);
    s->v[2] = V2(hx, hy);
    s->v[3] = V2(-hx, hy);
    s->n[0] = V2(0.0f, -1.0f);
    s->n[1] = V2(1.0f, 0.0f);
    s->n[2] = V2(0.0f, 1.0f);
    s->n[3] = V2(-1.0f, 0.0f);
    s->centroid = V2(0.0f, 0.0f);
}

static void shape_polygon_init(Shape *s)
{
    memset(s, 0, sizeof *s);
    s->type = SHAPE_POLYGON;
    s->radius = POLYGON_RADIUS;
}

static void shape_edge_init(Shape *s, Vec2 v1, Vec2 v2)
{
    memset(s, 0, sizeof *s);
    s->type = SHAPE_EDGE;
    s->radius = POLYGON_RADIUS;
    s->v1 = v1;
    s->v2 = v2;
    s->hasV0 = 0;
    s->hasV3 = 0;
}

static void shape_aabb(const Shape *s, AABB *aabb, const Transform *xf)
{
    if (s->type == SHAPE_EDGE) {
        float v1x = (xf->q.c * s->v1.x - xf->q.s * s->v1.y) + xf->p.x;
        float v1y = (xf->q.s * s->v1.x + xf->q.c * s->v1.y) + xf->p.y;
        float v2x = (xf->q.c * s->v2.x - xf->q.s * s->v2.y) + xf->p.x;
        float v2y = (xf->q.s * s->v2.x + xf->q.c * s->v2.y) + xf->p.y;
        aabb->lo.x = v1x < v2x ? v1x : v2x;
        aabb->lo.y = v1y < v2y ? v1y : v2y;
        aabb->up.x = v1x > v2x ? v1x : v2x;
        aabb->up.y = v1y > v2y ? v1y : v2y;
        aabb->lo.x -= s->radius;
        aabb->lo.y -= s->radius;
        aabb->up.x += s->radius;
        aabb->up.y += s->radius;
        return;
    }
    Vec2 v1 = s->v[0];
    aabb->lo.x = (xf->q.c * v1.x - xf->q.s * v1.y) + xf->p.x;
    aabb->lo.y = (xf->q.s * v1.x + xf->q.c * v1.y) + xf->p.y;
    aabb->up.x = aabb->lo.x;
    aabb->up.y = aabb->lo.y;
    for (int i = 1; i < s->count; ++i) {
        Vec2 v2 = s->v[i];
        float vx = (xf->q.c * v2.x - xf->q.s * v2.y) + xf->p.x;
        float vy = (xf->q.s * v2.x + xf->q.c * v2.y) + xf->p.y;
        aabb->lo.x = aabb->lo.x < vx ? aabb->lo.x : vx;
        aabb->lo.y = aabb->lo.y < vy ? aabb->lo.y : vy;
        aabb->up.x = aabb->up.x > vx ? aabb->up.x : vx;
        aabb->up.y = aabb->up.y > vy ? aabb->up.y : vy;
    }
    aabb->lo.x -= s->radius;
    aabb->lo.y -= s->radius;
    aabb->up.x += s->radius;
    aabb->up.y += s->radius;
}

static void shape_mass(const Shape *s, MassData *md, float density)
{
    if (s->type == SHAPE_EDGE) {
        md->mass = 0.0f;
        md->center = v2scale(v2add(s->v1, s->v2), 0.5f);
        md->I = 0.0f;
        return;
    }
    Vec2 center = V2(0.0f, 0.0f);
    float area = 0.0f;
    float I = 0.0f;
    Vec2 sv = V2(0.0f, 0.0f);
    for (int i = 0; i < s->count; ++i) {
        sv.x += s->v[i].x;
        sv.y += s->v[i].y;
    }
    sv.x *= 1.0f / s->count;
    sv.y *= 1.0f / s->count;
    const float k_inv3 = 1.0f / 3.0f;
    for (int i = 0; i < s->count; ++i) {
        Vec2 e1 = v2sub(s->v[i], sv);
        Vec2 nsv = V2(-sv.x, -sv.y);
        Vec2 nxt = (i + 1 < s->count) ? s->v[i + 1] : s->v[0];
        Vec2 e2 = V2(nsv.x + nxt.x, nsv.y + nxt.y);
        float D = v2cross(e1, e2);
        float triangleArea = 0.5f * D;
        area += triangleArea;
        center.x += triangleArea * k_inv3 * (e1.x + e2.x);
        center.y += triangleArea * k_inv3 * (e1.y + e2.y);
        float ex1 = e1.x, ey1 = e1.y;
        float ex2 = e2.x, ey2 = e2.y;
        float intx2 = ex1 * ex1 + ex2 * ex1 + ex2 * ex2;
        float inty2 = ey1 * ey1 + ey2 * ey1 + ey2 * ey2;
        I += (0.25f * k_inv3 * D) * (intx2 + inty2);
    }
    md->mass = density * area;
    center.x *= 1.0f / area;
    center.y *= 1.0f / area;
    md->center = v2add(center, sv);
    md->I = I * density;
    md->I += md->mass * v2dot(md->center, md->center);
}

/* ---------------- collision: manifolds and polygon/edge contact functions ---------------- */
enum { CID_VERTEX = 0, CID_FACE = 1 };
enum { MT_CIRCLES = 0, MT_FACE_A = 1, MT_FACE_B = 2 };
enum { EPAXIS_UNKNOWN = 0, EPAXIS_EDGE_A = 1, EPAXIS_EDGE_B = 2 };

typedef struct { uint8_t indexA, indexB, typeA, typeB; } ContactID;

static uint32_t cid_key(const ContactID *c)
{
    return ((uint32_t)c->indexA << 24) | ((uint32_t)c->indexB << 16) | ((uint32_t)c->typeA << 8) |
           (uint32_t)c->typeB;
}

typedef struct { Vec2 v; ContactID id; } ClipVertex;
typedef struct { Vec2 localPoint; float normalImpulse, tangentImpulse; ContactID id; } ManifoldPoint;
typedef struct {
    int type;
    int pointCount;
    Vec2 localNormal, localPoint;
    ManifoldPoint points[MAX_MANIFOLD_POINTS];
} Manifold;

typedef struct { float separation; int edgeIndex; } EdgeResults;

static float edgeSeparation(const Shape *poly1, const Transform *xf1, int edge1, const Shape *poly2,
                            const Transform *xf2)
{
    int count2 = poly2->count;
    const Vec2 *vertices2 = poly2->v;
    Rot xf1q = xf1->q;
    Rot xf2q = xf2->q;
    Vec2 v = poly1->n[edge1];
    float normal1Worldx = xf1q.c * v.x - xf1q.s * v.y;
    float normal1Worldy = xf1q.s * v.x + xf1q.c * v.y;
    Rot q1 = xf2q;
    float normal1x = q1.c * normal1Worldx + q1.s * normal1Worldy;
    float normal1y = -q1.s * normal1Worldx + q1.c * normal1Worldy;
    int index = 0;
    float minDot = FLT_MAX;
    for (int i = 0; i < count2; ++i) {
        Vec2 a = vertices2[i];
        float dot = a.x * normal1x + a.y * normal1y;
        if (dot < minDot) {
            minDot = dot;
            index = i;
        }
    }
    Vec2 v3 = poly1->v[edge1];
    float v1x = (xf1q.c * v3.x - xf1q.s * v3.y) + xf1->p.x;
    float v1y = (xf1q.s * v3.x + xf1q.c * v3.y) + xf1->p.y;
    Vec2 v4 = vertices2[index];
    float v2x = (xf2q.c * v4.x - xf2q.s * v4.y) + xf2->p.x - v1x;
    float v2y = (xf2q.s * v4.x + xf2q.c * v4.y) + xf2->p.y - v1y;
    return v2x * normal1Worldx + v2y * normal1Worldy;
}

static void findMaxSeparation(EdgeResults *results, const Shape *poly1, const Transform *xf1,
                              const Shape *poly2, const Transform *xf2)
{
    int count1 = poly1->count;
    const Vec2 *normals1 = poly1->n;
    Vec2 poly1centroid = poly1->centroid;
    Vec2 poly2centroid = poly2->centroid;
    Rot xf2q = xf2->q;
    Rot xf1q = xf1->q;
    float dx = (xf2q.c * poly2centroid.x - xf2q.s * poly2centroid.y) + xf2->p.x;
    float dy = (xf2q.s * poly2centroid.x + xf2q.c * poly2centroid.y) + xf2->p.y;
    dx -= (xf1q.c * poly1centroid.x - xf1q.s * poly1centroid.y) + xf1->p.x;
    dy -= (xf1q.s * poly1centroid.x + xf1q.c * poly1centroid.y) + xf1->p.y;
    float dLocal1x = xf1q.c * dx + xf1q.s * dy;
    float dLocal1y = -xf1q.s * dx + xf1q.c * dy;
    int edge = 0;
    float dot;
    float maxDot = -FLT_MAX;
    for (int i = 0; i < count1; i++) {
        Vec2 normal = normals1[i];
        dot = normal.x * dLocal1x + normal.y * dLocal1y;
        if (dot > maxDot) {
            maxDot = dot;
            edge = i;
        }
    }
    float s = edgeSeparation(poly1, xf1, edge, poly2, xf2);
    int prevEdge = edge - 1 >= 0 ? edge - 1 : count1 - 1;
    float sPrev = edgeSeparation(poly1, xf1, prevEdge, poly2, xf2);
    int nextEdge = edge + 1 < count1 ? edge + 1 : 0;
    float sNext = edgeSeparation(poly1, xf1, nextEdge, poly2, xf2);
    int bestEdge;
    float bestSeparation;
    int increment;
    if (sPrev > s && sPrev > sNext) {
        increment = -1;
        bestEdge = prevEdge;
        bestSeparation = sPrev;
    } else if (sNext > s) {
        increment = 1;
        bestEdge = nextEdge;
        bestSeparation = sNext;
    } else {
        results->edgeIndex = edge;
        results->separation = s;
        return;
    }
    for (;;) {
        if (increment == -1)
            edge = bestEdge - 1 >= 0 ? bestEdge - 1 : count1 - 1;
        else
            edge = bestEdge + 1 < count1 ? bestEdge + 1 : 0;
        s = edgeSeparation(poly1, xf1, edge, poly2, xf2);
        if (s > bestSeparation) {
            bestEdge = edge;
            bestSeparation = s;
        } else {
            break;
        }
    }
    results->edgeIndex = bestEdge;
    results->separation = bestSeparation;
}

static void findIncidentEdge(ClipVertex c[2], const Shape *poly1, const Transform *xf1, int edge1,
                             const Shape *poly2, const Transform *xf2)
{
    const Vec2 *normals1 = poly1->n;
    int count2 = poly2->count;
    const Vec2 *vertices2 = poly2->v;
    const Vec2 *normals2 = poly2->n;
    Rot xf1q = xf1->q;
    Rot xf2q = xf2->q;
    Vec2 v = normals1[edge1];
    float tempx = xf1q.c * v.x - xf1q.s * v.y;
    float tempy = xf1q.s * v.x + xf1q.c * v.y;
    float normal1x = xf2q.c * tempx + xf2q.s * tempy;
    float normal1y = -xf2q.s * tempx + xf2q.c * tempy;
    int index = 0;
    float minDot = FLT_MAX;
    for (int i = 0; i < count2; ++i) {
        Vec2 b = normals2[i];
        float dot = normal1x * b.x + normal1y * b.y;
        if (dot < minDot) {
            minDot = dot;
            index = i;
        }
    }
    int i1 = index;
    int i2 = i1 + 1 < count2 ? i1 + 1 : 0;
    Vec2 v1 = vertices2[i1];
    c[0].v.x = (xf2q.c * v1.x - xf2q.s * v1.y) + xf2->p.x;
    c[0].v.y = (xf2q.s * v1.x + xf2q.c * v1.y) + xf2->p.y;
    c[0].id.indexA = (uint8_t)edge1;
    c[0].id.indexB = (uint8_t)i1;
    c[0].id.typeA = CID_FACE;
    c[0].id.typeB = CID_VERTEX;
    Vec2 v2 = vertices2[i2];
    c[1].v.x = (xf2q.c * v2.x - xf2q.s * v2.y) + xf2->p.x;
    c[1].v.y = (xf2q.s * v2.x + xf2q.c * v2.y) + xf2->p.y;
    c[1].id.indexA = (uint8_t)edge1;
    c[1].id.indexB = (uint8_t)i2;
    c[1].id.typeA = CID_FACE;
    c[1].id.typeB = CID_VERTEX;
}

static int clipSegmentToLine(ClipVertex vOut[2], const ClipVertex vIn[2], Vec2 normal, float offset,
                             int vertexIndexA)
{
    int numOut = 0;
    const ClipVertex *vIn0 = &vIn[0];
    const ClipVertex *vIn1 = &vIn[1];
    float distance0 = v2dot(normal, vIn0->v) - offset;
    float distance1 = v2dot(normal, vIn1->v) - offset;
    if (distance0 <= 0.0f)
        vOut[numOut++] = *vIn0;
    if (distance1 <= 0.0f)
        vOut[numOut++] = *vIn1;
    if (distance0 * distance1 < 0.0f) {
        float interp = distance0 / (distance0 - distance1);
        ClipVertex *o = &vOut[numOut];
        o->v.x = vIn0->v.x + interp * (vIn1->v.x - vIn0->v.x);
        o->v.y = vIn0->v.y + interp * (vIn1->v.y - vIn0->v.y);
        o->id.indexA = (uint8_t)vertexIndexA;
        o->id.indexB = vIn0->id.indexB;
        o->id.typeA = CID_VERTEX;
        o->id.typeB = CID_FACE;
        ++numOut;
    }
    return numOut;
}

/* Collision.collidePolygons */
static void collidePolygons(Manifold *manifold, const Shape *polyA, const Transform *xfA,
                            const Shape *polyB, const Transform *xfB)
{
    manifold->pointCount = 0;
    float totalRadius = polyA->radius + polyB->radius;
    EdgeResults results1, results2;
    findMaxSeparation(&results1, polyA, xfA, polyB, xfB);
    if (results1.separation > totalRadius)
        return;
    findMaxSeparation(&results2, polyB, xfB, polyA, xfA);
    if (results2.separation > totalRadius)
        return;

    const Shape *poly1;
    const Shape *poly2;
    const Transform *xf1;
    const Transform *xf2;
    int edge1;
    int flip;
    const float k_relativeTol = 0.98f;
    const float k_absoluteTol = 0.001f;
    if (results2.separation > k_relativeTol * results1.separation + k_absoluteTol) {
        poly1 = polyB;
        poly2 = polyA;
        xf1 = xfB;
        xf2 = xfA;
        edge1 = results2.edgeIndex;
        manifold->type = MT_FACE_B;
        flip = 1;
    } else {
        poly1 = polyA;
        poly2 = polyB;
        xf1 = xfA;
        xf2 = xfB;
        edge1 = results1.edgeIndex;
        manifold->type = MT_FACE_A;
        flip = 0;
    }
    Rot xf1q = xf1->q;
    ClipVertex incidentEdge[2];
    findIncidentEdge(incidentEdge, poly1, xf1, edge1, poly2, xf2);
    int count1 = poly1->count;
    const Vec2 *vertices1 = poly1->v;
    int iv1 = edge1;
    int iv2 = edge1 + 1 < count1 ? edge1 + 1 : 0;
    Vec2 v11 = vertices1[iv1];
    Vec2 v12 = vertices1[iv2];
    Vec2 localTangent;
    localTangent.x = v12.x - v11.x;
    localTangent.y = v12.y - v11.y;
    v2normalize(&localTangent);
    Vec2 localNormal;
    localNormal.x = 1.0f * localTangent.y;
    localNormal.y = -1.0f * localTangent.x;
    Vec2 planePoint;
    planePoint.x = (v11.x + v12.x) * .5f;
    planePoint.y = (v11.y + v12.y) * .5f;
    Vec2 tangent;
    tangent.x = xf1q.c * localTangent.x - xf1q.s * localTangent.y;
    tangent.y = xf1q.s * localTangent.x + xf1q.c * localTangent.y;
    float normalx = 1.0f * tangent.y;
    float normaly = -1.0f * tangent.x;
    v11 = xf_mulV(xf1, v11);
    v12 = xf_mulV(xf1, v12);
    float frontOffset = normalx * v11.x + normaly * v11.y;
    float sideOffset1 = -(tangent.x * v11.x + tangent.y * v11.y) + totalRadius;
    float sideOffset2 = tangent.x * v12.x + tangent.y * v12.y + totalRadius;
    ClipVertex clipPoints1[2], clipPoints2[2];
    int np;
    tangent.x = -tangent.x;
    tangent.y = -tangent.y;
    np = clipSegmentToLine(clipPoints1, incidentEdge, tangent, sideOffset1, iv1);
    tangent.x = -tangent.x;
    tangent.y = -tangent.y;
    if (np < 2)
        return;
    np = clipSegmentToLine(clipPoints2, clipPoints1, tangent, sideOffset2, iv2);
    if (np < 2)
        return;
    manifold->localNormal = localNormal;
    manifold->localPoint = planePoint;
    int pointCount = 0;
    for (int i = 0; i < MAX_MANIFOLD_POINTS; ++i) {
        float separation = normalx * clipPoints2[i].v.x + normaly * clipPoints2[i].v.y - frontOffset;
        if (separation <= totalRadius) {
            ManifoldPoint *cp = &manifold->points[pointCount];
            float px = clipPoints2[i].v.x - xf2->p.x;
            float py = clipPoints2[i].v.y - xf2->p.y;
            cp->localPoint.x = (xf2->q.c * px + xf2->q.s * py);
            cp->localPoint.y = (-xf2->q.s * px + xf2->q.c * py);
            cp->id = clipPoints2[i].id;
            if (flip) {
                ContactID t = cp->id;
                cp->id.indexA = t.indexB;
                cp->id.indexB = t.indexA;
                cp->id.typeA = t.typeB;
                cp->id.typeB = t.typeA;
            }
            ++pointCount;
        }
    }
    manifold->pointCount = pointCount;
}

/* Collision.collideEdgeAndPolygon: EPCollider with the edge's ghost vertices absent. */
typedef struct { float separation; int type; int index; } EPAxis;

static void collideEdgeAndPolygon(Manifold *manifold, const Shape *edgeA, const Transform *xfA,
                                  const Shape *polygonB, const Transform *xfB)
{
    Transform m_xf = xf_mulT(xfA, xfB);
    Vec2 m_centroidB = xf_mulV(&m_xf, polygonB->centroid);
    Vec2 m_v1 = edgeA->v1;
    Vec2 m_v2 = edgeA->v2;

    Vec2 edge1 = v2sub(m_v2, m_v1);
    v2normalize(&edge1);
    Vec2 m_normal1 = V2(edge1.y, -edge1.x);
    float offset1 = v2dot(m_normal1, v2sub(m_centroidB, m_v1));

    /* hasVertex0 == hasVertex3 == false: the "else" branch of EPCollider.collide. */
    int m_front = offset1 >= 0.0f;
    Vec2 m_normal;
    Vec2 m_lowerLimit, m_upperLimit;
    if (m_front) {
        m_normal = m_normal1;
        m_lowerLimit = V2(-m_normal1.x, -m_normal1.y);
        m_upperLimit = V2(-m_normal1.x, -m_normal1.y);
    } else {
        m_normal = V2(-m_normal1.x, -m_normal1.y);
        m_lowerLimit = m_normal1;
        m_upperLimit = m_normal1;
    }

    Vec2 polyV[MAX_POLYGON_VERTICES], polyN[MAX_POLYGON_VERTICES];
    int polyCount = polygonB->count;
    for (int i = 0; i < polygonB->count; ++i) {
        polyV[i] = xf_mulV(&m_xf, polygonB->v[i]);
        polyN[i] = rot_mulV(m_xf.q, polygonB->n[i]);
    }
    float m_radius = 2.0f * POLYGON_RADIUS;
    manifold->pointCount = 0;

    /* computeEdgeSeparation */
    EPAxis edgeAxis;
    edgeAxis.type = EPAXIS_EDGE_A;
    edgeAxis.index = m_front ? 0 : 1;
    edgeAxis.separation = FLT_MAX;
    {
        float nx = m_normal.x;
        float ny = m_normal.y;
        for (int i = 0; i < polyCount; ++i) {
            Vec2 v = polyV[i];
            float tempx = v.x - m_v1.x;
            float tempy = v.y - m_v1.y;
            float s = nx * tempx + ny * tempy;
            if (s < edgeAxis.separation)
                edgeAxis.separation = s;
        }
    }
    if (edgeAxis.type == EPAXIS_UNKNOWN)
        return;
    if (edgeAxis.separation > m_radius)
        return;

    /* computePolygonSeparation */
    EPAxis polygonAxis;
    polygonAxis.type = EPAXIS_UNKNOWN;
    polygonAxis.index = -1;
    polygonAxis.separation = -FLT_MAX;
    {
        Vec2 perp = V2(-m_normal.y, m_normal.x);
        for (int i = 0; i < polyCount; ++i) {
            Vec2 normalB = polyN[i];
            Vec2 vB = polyV[i];
            Vec2 nn = V2(-normalB.x, -normalB.y);
            float tempx = vB.x - m_v1.x;
            float tempy = vB.y - m_v1.y;
            float s1 = nn.x * tempx + nn.y * tempy;
            tempx = vB.x - m_v2.x;
            tempy = vB.y - m_v2.y;
            float s2 = nn.x * tempx + nn.y * tempy;
            float s = jb_min(s1, s2);
            if (s > m_radius) {
                polygonAxis.type = EPAXIS_EDGE_B;
                polygonAxis.index = i;
                polygonAxis.separation = s;
                break;
            }
            if (nn.x * perp.x + nn.y * perp.y >= 0.0f) {
                if (v2dot(v2sub(nn, m_upperLimit), m_normal) < -ANGULAR_SLOP)
                    continue;
            } else {
                if (v2dot(v2sub(nn, m_lowerLimit), m_normal) < -ANGULAR_SLOP)
                    continue;
            }
            if (s > polygonAxis.separation) {
                polygonAxis.type = EPAXIS_EDGE_B;
                polygonAxis.index = i;
                polygonAxis.separation = s;
            }
        }
    }
    if (polygonAxis.type != EPAXIS_UNKNOWN && polygonAxis.separation > m_radius)
        return;

    const float k_relativeTol = 0.98f;
    const float k_absoluteTol = 0.001f;
    EPAxis primaryAxis;
    if (polygonAxis.type == EPAXIS_UNKNOWN)
        primaryAxis = edgeAxis;
    else if (polygonAxis.separation > k_relativeTol * edgeAxis.separation + k_absoluteTol)
        primaryAxis = polygonAxis;
    else
        primaryAxis = edgeAxis;

    ClipVertex ie[2];
    struct {
        int i1, i2;
        Vec2 v1, v2, normal, sideNormal1, sideNormal2;
        float sideOffset1, sideOffset2;
    } rf;

    if (primaryAxis.type == EPAXIS_EDGE_A) {
        manifold->type = MT_FACE_A;
        int bestIndex = 0;
        float bestValue = v2dot(m_normal, polyN[0]);
        for (int i = 1; i < polyCount; ++i) {
            float value = v2dot(m_normal, polyN[i]);
            if (value < bestValue) {
                bestValue = value;
                bestIndex = i;
            }
        }
        int i1 = bestIndex;
        int i2 = i1 + 1 < polyCount ? i1 + 1 : 0;
        ie[0].v = polyV[i1];
        ie[0].id.indexA = 0;
        ie[0].id.indexB = (uint8_t)i1;
        ie[0].id.typeA = CID_FACE;
        ie[0].id.typeB = CID_VERTEX;
        ie[1].v = polyV[i2];
        ie[1].id.indexA = 0;
        ie[1].id.indexB = (uint8_t)i2;
        ie[1].id.typeA = CID_FACE;
        ie[1].id.typeB = CID_VERTEX;
        if (m_front) {
            rf.i1 = 0;
            rf.i2 = 1;
            rf.v1 = m_v1;
            rf.v2 = m_v2;
            rf.normal = m_normal1;
        } else {
            rf.i1 = 1;
            rf.i2 = 0;
            rf.v1 = m_v2;
            rf.v2 = m_v1;
            rf.normal = V2(-m_normal1.x, -m_normal1.y);
        }
    } else {
        manifold->type = MT_FACE_B;
        ie[0].v = m_v1;
        ie[0].id.indexA = 0;
        ie[0].id.indexB = (uint8_t)primaryAxis.index;
        ie[0].id.typeA = CID_VERTEX;
        ie[0].id.typeB = CID_FACE;
        ie[1].v = m_v2;
        ie[1].id.indexA = 0;
        ie[1].id.indexB = (uint8_t)primaryAxis.index;
        ie[1].id.typeA = CID_VERTEX;
        ie[1].id.typeB = CID_FACE;
        rf.i1 = primaryAxis.index;
        rf.i2 = rf.i1 + 1 < polyCount ? rf.i1 + 1 : 0;
        rf.v1 = polyV[rf.i1];
        rf.v2 = polyV[rf.i2];
        rf.normal = polyN[rf.i1];
    }
    rf.sideNormal1 = V2(rf.normal.y, -rf.normal.x);
    rf.sideNormal2 = V2(-rf.sideNormal1.x, -rf.sideNormal1.y);
    rf.sideOffset1 = v2dot(rf.sideNormal1, rf.v1);
    rf.sideOffset2 = v2dot(rf.sideNormal2, rf.v2);

    ClipVertex clipPoints1[2], clipPoints2[2];
    int np;
    np = clipSegmentToLine(clipPoints1, ie, rf.sideNormal1, rf.sideOffset1, rf.i1);
    if (np < MAX_MANIFOLD_POINTS)
        return;
    np = clipSegmentToLine(clipPoints2, clipPoints1, rf.sideNormal2, rf.sideOffset2, rf.i2);
    if (np < MAX_MANIFOLD_POINTS)
        return;

    if (primaryAxis.type == EPAXIS_EDGE_A) {
        manifold->localNormal = rf.normal;
        manifold->localPoint = rf.v1;
    } else {
        manifold->localNormal = polygonB->n[rf.i1];
        manifold->localPoint = polygonB->v[rf.i1];
    }
    int pointCount = 0;
    for (int i = 0; i < MAX_MANIFOLD_POINTS; ++i) {
        float separation = v2dot(rf.normal, v2sub(clipPoints2[i].v, rf.v1));
        if (separation <= m_radius) {
            ManifoldPoint *cp = &manifold->points[pointCount];
            if (primaryAxis.type == EPAXIS_EDGE_A) {
                cp->localPoint = xf_mulTV(&m_xf, clipPoints2[i].v);
                cp->id = clipPoints2[i].id;
            } else {
                cp->localPoint = clipPoints2[i].v;
                cp->id.typeA = clipPoints2[i].id.typeB;
                cp->id.typeB = clipPoints2[i].id.typeA;
                cp->id.indexA = clipPoints2[i].id.indexB;
                cp->id.indexB = clipPoints2[i].id.indexA;
            }
            ++pointCount;
        }
    }
    manifold->pointCount = pointCount;
}

/* WorldManifold.initialize for FACE_A / FACE_B manifolds */
typedef struct { Vec2 normal; Vec2 points[MAX_MANIFOLD_POINTS]; } WorldManifold;

static void worldManifold_init(WorldManifold *wm, const Manifold *m, const Transform *xfA,
                               float radiusA, const Transform *xfB, float radiusB)
{
    if (m->pointCount == 0)
        return;
    if (m->type == MT_FACE_A) {
        wm->normal = rot_mulV(xfA->q, m->localNormal);
        Vec2 planePoint = xf_mulV(xfA, m->localPoint);
        for (int i = 0; i < m->pointCount; i++) {
            Vec2 clipPoint = xf_mulV(xfB, m->points[i].localPoint);
            float scalar = radiusA - ((clipPoint.x - planePoint.x) * wm->normal.x +
                                      (clipPoint.y - planePoint.y) * wm->normal.y);
            float cAx = wm->normal.x * scalar + clipPoint.x;
            float cAy = wm->normal.y * scalar + clipPoint.y;
            float cBx = -wm->normal.x * radiusB + clipPoint.x;
            float cBy = -wm->normal.y * radiusB + clipPoint.y;
            wm->points[i].x = (cAx + cBx) * .5f;
            wm->points[i].y = (cAy + cBy) * .5f;
        }
    } else if (m->type == MT_FACE_B) {
        wm->normal = rot_mulV(xfB->q, m->localNormal);
        Vec2 planePoint = xf_mulV(xfB, m->localPoint);
        for (int i = 0; i < m->pointCount; i++) {
            Vec2 clipPoint = xf_mulV(xfA, m->points[i].localPoint);
            float scalar = radiusB - ((clipPoint.x - planePoint.x) * wm->normal.x +
                                      (clipPoint.y - planePoint.y) * wm->normal.y);
            float cBx = wm->normal.x * scalar + clipPoint.x;
            float cBy = wm->normal.y * scalar + clipPoint.y;
            float cAx = -wm->normal.x * radiusA + clipPoint.x;
            float cAy = -wm->normal.y * radiusA + clipPoint.y;
            wm->points[i].x = (cAx + cBx) * .5f;
            wm->points[i].y = (cAy + cBy) * .5f;
        }
        wm->normal.x = -wm->normal.x;
        wm->normal.y = -wm->normal.y;
    }
}

/* ---------------- DynamicTree (collision/broadphase/DynamicTree.java) ---------------- */
typedef struct {
    AABB aabb;
    void *userData;
    int parent, child1, child2;
    int id;
    int height;
} TreeNode;

typedef struct {
    TreeNode *nodes;
    int nodeCount, nodeCapacity, freeList, root;
    int insertionCount;
} DynamicTree;

static void tree_init(DynamicTree *t)
{
    t->nodeCapacity = 16;
    t->nodes = (TreeNode *)calloc((size_t)t->nodeCapacity, sizeof(TreeNode));
    for (int i = t->nodeCapacity - 1; i >= 0; i--) {
        t->nodes[i].id = i;
        t->nodes[i].parent = (i == t->nodeCapacity - 1) ? -1 : i + 1;
        t->nodes[i].child1 = -1;
        t->nodes[i].child2 = -1;
        t->nodes[i].height = -1;
    }
    t->root = -1;
    t->nodeCount = 0;
    t->freeList = 0;
    t->insertionCount = 0;
}

static int tree_allocateNode(DynamicTree *t)
{
    if (t->freeList == -1) {
        int oldCap = t->nodeCapacity;
        t->nodeCapacity *= 2;
        t->nodes = (TreeNode *)realloc(t->nodes, sizeof(TreeNode) * (size_t)t->nodeCapacity);
        memset(&t->nodes[oldCap], 0, sizeof(TreeNode) * (size_t)(t->nodeCapacity - oldCap));
        for (int i = t->nodeCapacity - 1; i >= t->nodeCount; i--) {
            t->nodes[i].id = i;
            t->nodes[i].parent = (i == t->nodeCapacity - 1) ? -1 : i + 1;
            t->nodes[i].child1 = -1;
            t->nodes[i].child2 = -1;
            t->nodes[i].height = -1;
        }
        t->freeList = t->nodeCount;
    }
    int nodeId = t->freeList;
    TreeNode *node = &t->nodes[nodeId];
    t->freeList = node->parent != -1 ? node->parent : -1;
    node->parent = -1;
    node->child1 = -1;
    node->child2 = -1;
    node->height = 0;
    node->userData = NULL;
    ++t->nodeCount;
    return nodeId;
}

static void tree_freeNode(DynamicTree *t, int nodeId)
{
    t->nodes[nodeId].parent = t->freeList != -1 ? t->freeList : -1;
    t->nodes[nodeId].height = -1;
    t->freeList = nodeId;
    t->nodeCount--;
}

static int tree_balance(DynamicTree *t, int iA)
{
    TreeNode *A = &t->nodes[iA];
    if (A->child1 == -1 || A->height < 2)
        return iA;
    int iB = A->child1;
    int iC = A->child2;
    TreeNode *B = &t->nodes[iB];
    TreeNode *C = &t->nodes[iC];
    int balance = C->height - B->height;
    if (balance > 1) {
        int iF = C->child1;
        int iG = C->child2;
        TreeNode *F = &t->nodes[iF];
        TreeNode *G = &t->nodes[iG];
        C->child1 = iA;
        C->parent = A->parent;
        A->parent = iC;
        if (C->parent != -1) {
            if (t->nodes[C->parent].child1 == iA)
                t->nodes[C->parent].child1 = iC;
            else
                t->nodes[C->parent].child2 = iC;
        } else {
            t->root = iC;
        }
        if (F->height > G->height) {
            C->child2 = iF;
            A->child2 = iG;
            G->parent = iA;
            A->aabb = aabb_combine(B->aabb, G->aabb);
            C->aabb = aabb_combine(A->aabb, F->aabb);
            A->height = 1 + (B->height > G->height ? B->height : G->height);
            C->height = 1 + (A->height > F->height ? A->height : F->height);
        } else {
            C->child2 = iG;
            A->child2 = iF;
            F->parent = iA;
            A->aabb = aabb_combine(B->aabb, F->aabb);
            C->aabb = aabb_combine(A->aabb, G->aabb);
            A->height = 1 + (B->height > F->height ? B->height : F->height);
            C->height = 1 + (A->height > G->height ? A->height : G->height);
        }
        return iC;
    }
    if (balance < -1) {
        int iD = B->child1;
        int iE = B->child2;
        TreeNode *D = &t->nodes[iD];
        TreeNode *E = &t->nodes[iE];
        B->child1 = iA;
        B->parent = A->parent;
        A->parent = iB;
        if (B->parent != -1) {
            if (t->nodes[B->parent].child1 == iA)
                t->nodes[B->parent].child1 = iB;
            else
                t->nodes[B->parent].child2 = iB;
        } else {
            t->root = iB;
        }
        if (D->height > E->height) {
            B->child2 = iD;
            A->child1 = iE;
            E->parent = iA;
            A->aabb = aabb_combine(C->aabb, E->aabb);
            B->aabb = aabb_combine(A->aabb, D->aabb);
            A->height = 1 + (C->height > E->height ? C->height : E->height);
            B->height = 1 + (A->height > D->height ? A->height : D->height);
        } else {
            B->child2 = iE;
            A->child1 = iD;
            D->parent = iA;
            A->aabb = aabb_combine(C->aabb, D->aabb);
            B->aabb = aabb_combine(A->aabb, E->aabb);
            A->height = 1 + (C->height > D->height ? C->height : D->height);
            B->height = 1 + (A->height > E->height ? A->height : E->height);
        }
        return iB;
    }
    return iA;
}

static void tree_insertLeaf(DynamicTree *t, int leaf_index)
{
    t->insertionCount++;
    if (t->root == -1) {
        t->root = leaf_index;
        t->nodes[leaf_index].parent = -1;
        return;
    }
    AABB leafAABB = t->nodes[leaf_index].aabb;
    int index = t->root;
    while (t->nodes[index].child1 != -1) {
        int node = index;
        int child1 = t->nodes[node].child1;
        int child2 = t->nodes[node].child2;
        float area = aabb_perimeter(t->nodes[node].aabb);
        AABB combinedAABB = aabb_combine(t->nodes[node].aabb, leafAABB);
        float combinedArea = aabb_perimeter(combinedAABB);
        float cost = 2.0f * combinedArea;
        float inheritanceCost = 2.0f * (combinedArea - area);
        float cost1;
        if (t->nodes[child1].child1 == -1) {
            combinedAABB = aabb_combine(leafAABB, t->nodes[child1].aabb);
            cost1 = aabb_perimeter(combinedAABB) + inheritanceCost;
        } else {
            combinedAABB = aabb_combine(leafAABB, t->nodes[child1].aabb);
            float oldArea = aabb_perimeter(t->nodes[child1].aabb);
            float newArea = aabb_perimeter(combinedAABB);
            cost1 = (newArea - oldArea) + inheritanceCost;
        }
        float cost2;
        if (t->nodes[child2].child1 == -1) {
            combinedAABB = aabb_combine(leafAABB, t->nodes[child2].aabb);
            cost2 = aabb_perimeter(combinedAABB) + inheritanceCost;
        } else {
            combinedAABB = aabb_combine(leafAABB, t->nodes[child2].aabb);
            float oldArea = aabb_perimeter(t->nodes[child2].aabb);
            float newArea = aabb_perimeter(combinedAABB);
            cost2 = newArea - oldArea + inheritanceCost;
        }
        if (cost < cost1 && cost < cost2)
            break;
        if (cost1 < cost2)
            index = child1;
        else
            index = child2;
    }
    int sibling = index;
    int oldParent = t->nodes[sibling].parent;
    int newParent = tree_allocateNode(t);
    t->nodes[newParent].parent = oldParent;
    t->nodes[newParent].userData = NULL;
    t->nodes[newParent].aabb = aabb_combine(leafAABB, t->nodes[sibling].aabb);
    t->nodes[newParent].height = t->nodes[sibling].height + 1;
    if (oldParent != -1) {
        if (t->nodes[oldParent].child1 == sibling)
            t->nodes[oldParent].child1 = newParent;
        else
            t->nodes[oldParent].child2 = newParent;
        t->nodes[newParent].child1 = sibling;
        t->nodes[newParent].child2 = leaf_index;
        t->nodes[sibling].parent = newParent;
        t->nodes[leaf_index].parent = newParent;
    } else {
        t->nodes[newParent].child1 = sibling;
        t->nodes[newParent].child2 = leaf_index;
        t->nodes[sibling].parent = newParent;
        t->nodes[leaf_index].parent = newParent;
        t->root = newParent;
    }
    index = t->nodes[leaf_index].parent;
    while (index != -1) {
        index = tree_balance(t, index);
        int child1 = t->nodes[index].child1;
        int child2 = t->nodes[index].child2;
        t->nodes[index].height = 1 + (t->nodes[child1].height > t->nodes[child2].height
                                          ? t->nodes[child1].height
                                          : t->nodes[child2].height);
        t->nodes[index].aabb = aabb_combine(t->nodes[child1].aabb, t->nodes[child2].aabb);
        index = t->nodes[index].parent;
    }
}

static void tree_removeLeaf(DynamicTree *t, int leaf)
{
    if (leaf == t->root) {
        t->root = -1;
        return;
    }
    int parent = t->nodes[leaf].parent;
    int grandParent = t->nodes[parent].parent;
    int sibling;
    if (t->nodes[parent].child1 == leaf)
        sibling = t->nodes[parent].child2;
    else
        sibling = t->nodes[parent].child1;
    if (grandParent != -1) {
        if (t->nodes[grandParent].child1 == parent)
            t->nodes[grandParent].child1 = sibling;
        else
            t->nodes[grandParent].child2 = sibling;
        t->nodes[sibling].parent = grandParent;
        tree_freeNode(t, parent);
        int index = grandParent;
        while (index != -1) {
            index = tree_balance(t, index);
            int child1 = t->nodes[index].child1;
            int child2 = t->nodes[index].child2;
            t->nodes[index].aabb = aabb_combine(t->nodes[child1].aabb, t->nodes[child2].aabb);
            t->nodes[index].height = 1 + (t->nodes[child1].height > t->nodes[child2].height
                                              ? t->nodes[child1].height
                                              : t->nodes[child2].height);
            index = t->nodes[index].parent;
        }
    } else {
        t->root = sibling;
        t->nodes[sibling].parent = -1;
        tree_freeNode(t, parent);
    }
}

static int tree_createProxy(DynamicTree *t, AABB aabb, void *userData)
{
    int proxyId = tree_allocateNode(t);
    TreeNode *node = &t->nodes[proxyId];
    node->aabb.lo.x = aabb.lo.x - AABB_EXTENSION;
    node->aabb.lo.y = aabb.lo.y - AABB_EXTENSION;
    node->aabb.up.x = aabb.up.x + AABB_EXTENSION;
    node->aabb.up.y = aabb.up.y + AABB_EXTENSION;
    node->userData = userData;
    tree_insertLeaf(t, proxyId);
    return proxyId;
}

static int tree_moveProxy(DynamicTree *t, int proxyId, AABB aabb, Vec2 displacement)
{
    TreeNode *node = &t->nodes[proxyId];
    AABB nodeAABB = node->aabb;
    if (nodeAABB.lo.x > aabb.lo.x && nodeAABB.lo.y > aabb.lo.y && aabb.up.x > nodeAABB.up.x &&
        aabb.up.y > nodeAABB.up.y)
        return 0;
    tree_removeLeaf(t, proxyId);
    node = &t->nodes[proxyId];
    Vec2 *lowerBound = &node->aabb.lo;
    Vec2 *upperBound = &node->aabb.up;
    lowerBound->x = aabb.lo.x - AABB_EXTENSION;
    lowerBound->y = aabb.lo.y - AABB_EXTENSION;
    upperBound->x = aabb.up.x + AABB_EXTENSION;
    upperBound->y = aabb.up.y + AABB_EXTENSION;
    float dx = displacement.x * AABB_MULTIPLIER;
    float dy = displacement.y * AABB_MULTIPLIER;
    if (dx < 0.0f)
        lowerBound->x += dx;
    else
        upperBound->x += dx;
    if (dy < 0.0f)
        lowerBound->y += dy;
    else
        upperBound->y += dy;
    tree_insertLeaf(t, proxyId);
    return 1;
}

/* DynamicTree.query with a stack; TreeCallback is BroadPhase.treeCallback. */
typedef struct BroadPhase BroadPhase;
static int bp_treeCallback(BroadPhase *bp, int proxyId);

static void tree_query(DynamicTree *t, BroadPhase *bp, AABB aabb)
{
    int stack[1024];
    int count = 0;
    stack[count++] = t->root;
    while (count > 0) {
        int node = stack[--count];
        if (node == -1)
            continue;
        if (aabb_testOverlap(t->nodes[node].aabb, aabb)) {
            if (t->nodes[node].child1 == -1) {
                bp_treeCallback(bp, t->nodes[node].id);
            } else {
                stack[count++] = t->nodes[node].child1;
                stack[count++] = t->nodes[node].child2;
            }
        }
    }
}

/* ---------------- BroadPhase ---------------- */
typedef struct { int proxyIdA, proxyIdB; } Pair;

struct BroadPhase {
    DynamicTree tree;
    int proxyCount;
    int *moveBuffer;
    int moveCapacity, moveCount;
    Pair *pairBuffer;
    int pairCapacity, pairCount;
    int queryProxyId;
};

static void bp_bufferMove(BroadPhase *bp, int proxyId)
{
    if (bp->moveCount == bp->moveCapacity) {
        bp->moveCapacity *= 2;
        bp->moveBuffer = (int *)realloc(bp->moveBuffer, sizeof(int) * (size_t)bp->moveCapacity);
    }
    bp->moveBuffer[bp->moveCount] = proxyId;
    ++bp->moveCount;
}

static void bp_init(BroadPhase *bp)
{
    tree_init(&bp->tree);
    bp->proxyCount = 0;
    bp->pairCapacity = 16;
    bp->pairCount = 0;
    bp->pairBuffer = (Pair *)malloc(sizeof(Pair) * (size_t)bp->pairCapacity);
    bp->moveCapacity = 16;
    bp->moveCount = 0;
    bp->moveBuffer = (int *)malloc(sizeof(int) * (size_t)bp->moveCapacity);
    bp->queryProxyId = -1;
}

static void bp_destroy(BroadPhase *bp)
{
    free(bp->tree.nodes);
    free(bp->moveBuffer);
    free(bp->pairBuffer);
}

static int bp_createProxy(BroadPhase *bp, AABB aabb, void *userData)
{
    int proxyId = tree_createProxy(&bp->tree, aabb, userData);
    ++bp->proxyCount;
    bp_bufferMove(bp, proxyId);
    return proxyId;
}

static void bp_moveProxy(BroadPhase *bp, int proxyId, AABB aabb, Vec2 displacement)
{
    if (tree_moveProxy(&bp->tree, proxyId, aabb, displacement))
        bp_bufferMove(bp, proxyId);
}

static int bp_testOverlap(BroadPhase *bp, int proxyIdA, int proxyIdB)
{
    return aabb_testOverlap(bp->tree.nodes[proxyIdA].aabb, bp->tree.nodes[proxyIdB].aabb);
}

static int bp_treeCallback(BroadPhase *bp, int proxyId)
{
    if (proxyId == bp->queryProxyId)
        return 1;
    if (bp->pairCount == bp->pairCapacity) {
        bp->pairCapacity *= 2;
        bp->pairBuffer = (Pair *)realloc(bp->pairBuffer, sizeof(Pair) * (size_t)bp->pairCapacity);
    }
    if (proxyId < bp->queryProxyId) {
        bp->pairBuffer[bp->pairCount].proxyIdA = proxyId;
        bp->pairBuffer[bp->pairCount].proxyIdB = bp->queryProxyId;
    } else {
        bp->pairBuffer[bp->pairCount].proxyIdA = bp->queryProxyId;
        bp->pairBuffer[bp->pairCount].proxyIdB = proxyId;
    }
    ++bp->pairCount;
    return 1;
}

static int pair_cmp(const void *x, const void *y)
{
    const Pair *a = (const Pair *)x;
    const Pair *b = (const Pair *)y;
    if (a->proxyIdA < b->proxyIdA)
        return -1;
    if (a->proxyIdA == b->proxyIdA)
        return a->proxyIdB < b->proxyIdB ? -1 : (a->proxyIdB == b->proxyIdB ? 0 : 1);
    return 1;
}

/* ---------------- bodies, fixtures, contacts ---------------- */
enum { BODY_STATIC = 0, BODY_DYNAMIC = 2 };
enum {
    BF_ISLAND = 0x0001,
    BF_AWAKE = 0x0002,
    BF_AUTOSLEEP = 0x0004,
    BF_BULLET = 0x0008,
    BF_FIXEDROT = 0x0010,
    BF_ACTIVE = 0x0020
};
enum {
    CF_ISLAND = 0x0001,
    CF_TOUCHING = 0x0002,
    CF_ENABLED = 0x0004,
    CF_FILTER = 0x0008,
    CF_TOI = 0x0020
};
enum { WF_NEW_FIXTURE = 0x0001, WF_CLEAR_FORCES = 0x0004 };
enum { CONTACT_POLYGON = 0, CONTACT_EDGE_POLYGON = 1 };

typedef struct { uint16_t categoryBits, maskBits; int16_t groupIndex; } Filter;

typedef struct { float dt, inv_dt, dtRatio; int velocityIterations, positionIterations; int warmStarting; } TimeStep;
typedef struct { Vec2 c; float a; } Position;
typedef struct { Vec2 v; float w; } Velocity;

typedef struct Body Body;
typedef struct Fixture Fixture;
typedef struct Contact Contact;
typedef struct World World;

typedef struct FixtureProxy {
    AABB aabb;
    int proxyId;
    Fixture *fixture;
    int childIndex;
} FixtureProxy;

struct Fixture {
    Body *body;
    Fixture *next;
    Shape shape;
    float density, friction, restitution;
    Filter filter;
    int isSensor;
    int proxyCount;
    FixtureProxy proxy;
};

typedef struct ContactEdge {
    Contact *contact;
    struct ContactEdge *prev, *next;
    Body *other;
} ContactEdge;

struct Contact {
    int flags;
    Contact *prev, *next;
    ContactEdge nodeA, nodeB;
    Fixture *fixtureA, *fixtureB;
    int indexA, indexB;
    Manifold manifold;
    float toiCount, toi;
    float friction, restitution, tangentSpeed;
    int kind;
};

struct Body {
    int type;
    int flags;
    int islandIndex;
    Transform xf;
    Sweep sweep;
    Vec2 linearVelocity;
    float angularVelocity;
    Vec2 force;
    float torque;
    Body *prev, *next;
    Fixture *fixtureList;
    int fixtureCount;
    ContactEdge *contactList;
    float mass, invMass, I, invI;
    float linearDamping, angularDamping, gravityScale;
    float sleepTime;
    World *world;
};

static int body_isAwake(const Body *b) { return (b->flags & BF_AWAKE) == BF_AWAKE; }
static int body_isActive(const Body *b) { return (b->flags & BF_ACTIVE) == BF_ACTIVE; }
static int body_isBullet(const Body *b) { return (b->flags & BF_BULLET) == BF_BULLET; }

static void body_setAwake(Body *b, int flag)
{
    if (flag) {
        if ((b->flags & BF_AWAKE) == 0) {
            b->flags |= BF_AWAKE;
            b->sleepTime = 0.0f;
        }
    } else {
        b->flags &= ~BF_AWAKE;
        b->sleepTime = 0.0f;
        b->linearVelocity = V2(0.0f, 0.0f);
        b->angularVelocity = 0.0f;
        b->force = V2(0.0f, 0.0f);
        b->torque = 0.0f;
    }
}

static void body_resetMassData(Body *b)
{
    b->mass = 0.0f;
    b->invMass = 0.0f;
    b->I = 0.0f;
    b->invI = 0.0f;
    b->sweep.localCenter = V2(0.0f, 0.0f);
    if (b->type == BODY_STATIC) {
        b->sweep.c0 = b->xf.p;
        b->sweep.c = b->xf.p;
        b->sweep.a0 = b->sweep.a;
        return;
    }
    Vec2 localCenter = V2(0.0f, 0.0f);
    for (Fixture *f = b->fixtureList; f != NULL; f = f->next) {
        if (f->density == 0.0f)
            continue;
        MassData massData;
        shape_mass(&f->shape, &massData, f->density);
        b->mass += massData.mass;
        Vec2 temp = v2scale(massData.center, massData.mass);
        localCenter = v2add(localCenter, temp);
        b->I += massData.I;
    }
    if (b->mass > 0.0f) {
        b->invMass = 1.0f / b->mass;
        localCenter = v2scale(localCenter, b->invMass);
    } else {
        b->mass = 1.0f;
        b->invMass = 1.0f;
    }
    if (b->I > 0.0f && (b->flags & BF_FIXEDROT) == 0) {
        b->I -= b->mass * v2dot(localCenter, localCenter);
        b->invI = 1.0f / b->I;
    } else {
        b->I = 0.0f;
        b->invI = 0.0f;
    }
    Vec2 oldCenter = b->sweep.c;
    b->sweep.localCenter = localCenter;
    b->sweep.c0 = xf_mulV(&b->xf, b->sweep.localCenter);
    b->sweep.c = b->sweep.c0;
    Vec2 temp = v2sub(b->sweep.c, oldCenter);
    Vec2 temp2 = v2crossSV(b->angularVelocity, temp);
    b->linearVelocity = v2add(b->linearVelocity, temp2);
}

static void fixture_create(Fixture *f, Body *body, const Shape *shape, float density, float friction,
                           Filter filter, int isSensor)
{
    f->body = body;
    f->next = NULL;
    f->friction = friction;
    f->restitution = 0.0f;
    f->filter = filter;
    f->isSensor = isSensor;
    f->shape = *shape;
    f->density = density;
    f->proxyCount = 0;
    f->proxy.proxyId = -1;
    f->proxy.fixture = NULL;
    f->proxy.childIndex = 0;
}

static void fixture_createProxies(Fixture *f, BroadPhase *bp, const Transform *xf)
{
    f->proxyCount = 1;
    shape_aabb(&f->shape, &f->proxy.aabb, xf);
    f->proxy.proxyId = bp_createProxy(bp, f->proxy.aabb, &f->proxy);
    f->proxy.fixture = f;
    f->proxy.childIndex = 0;
}

static void fixture_synchronize(Fixture *f, BroadPhase *bp, const Transform *transform1,
                                const Transform *transform2)
{
    if (f->proxyCount == 0)
        return;
    FixtureProxy *proxy = &f->proxy;
    AABB aabb1, aab;
    shape_aabb(&f->shape, &aabb1, transform1);
    shape_aabb(&f->shape, &aab, transform2);
    proxy->aabb.lo.x = aabb1.lo.x < aab.lo.x ? aabb1.lo.x : aab.lo.x;
    proxy->aabb.lo.y = aabb1.lo.y < aab.lo.y ? aabb1.lo.y : aab.lo.y;
    proxy->aabb.up.x = aabb1.up.x > aab.up.x ? aabb1.up.x : aab.up.x;
    proxy->aabb.up.y = aabb1.up.y > aab.up.y ? aabb1.up.y : aab.up.y;
    Vec2 displacement;
    displacement.x = transform2->p.x - transform1->p.x;
    displacement.y = transform2->p.y - transform1->p.y;
    bp_moveProxy(bp, proxy->proxyId, proxy->aabb, displacement);
}

static void body_synchronizeFixtures(Body *b, BroadPhase *bp)
{
    Transform xf1;
    xf1.q = rot_set(b->sweep.a0);
    xf1.p.x = b->sweep.c0.x - xf1.q.c * b->sweep.localCenter.x + xf1.q.s * b->sweep.localCenter.y;
    xf1.p.y = b->sweep.c0.y - xf1.q.s * b->sweep.localCenter.x - xf1.q.c * b->sweep.localCenter.y;
    for (Fixture *f = b->fixtureList; f != NULL; f = f->next)
        fixture_synchronize(f, bp, &xf1, &b->xf);
}

static void body_synchronizeTransform(Body *b)
{
    b->xf.q = rot_set(b->sweep.a);
    Rot q = b->xf.q;
    Vec2 v = b->sweep.localCenter;
    b->xf.p.x = b->sweep.c.x - q.c * v.x + q.s * v.y;
    b->xf.p.y = b->sweep.c.y - q.s * v.x - q.c * v.y;
}

/* Body.advance (protected): used by solveTOI */
static void body_advance(Body *b, float t)
{
    sweep_advance(&b->sweep, t);
    b->sweep.c = b->sweep.c0;
    b->sweep.a = b->sweep.a0;
    b->xf.q = rot_set(b->sweep.a);
    Vec2 p = rot_mulV(b->xf.q, b->sweep.localCenter);
    p.x *= -1;
    p.y *= -1;
    p.x += b->sweep.c.x;
    p.y += b->sweep.c.y;
    b->xf.p = p;
}

static void contact_init(Contact *c, Fixture *fA, int indexA, Fixture *fB, int indexB)
{
    c->flags = 0;
    c->fixtureA = fA;
    c->fixtureB = fB;
    c->indexA = indexA;
    c->indexB = indexB;
    c->manifold.pointCount = 0;
    c->prev = NULL;
    c->next = NULL;
    memset(&c->nodeA, 0, sizeof c->nodeA);
    memset(&c->nodeB, 0, sizeof c->nodeB);
    c->toiCount = 0;
    c->friction = sqrtf(fA->friction * fB->friction);
    c->restitution = fA->restitution > fB->restitution ? fA->restitution : fB->restitution;
    c->tangentSpeed = 0;
}

static void contact_evaluate(Contact *c, Manifold *manifold, const Transform *xfA, const Transform *xfB)
{
    if (c->kind == CONTACT_EDGE_POLYGON)
        collideEdgeAndPolygon(manifold, &c->fixtureA->shape, xfA, &c->fixtureB->shape, xfB);
    else
        collidePolygons(manifold, &c->fixtureA->shape, xfA, &c->fixtureB->shape, xfB);
}

/* Contact.update with a null contact listener */
static void contact_update(Contact *c)
{
    Manifold oldManifold = c->manifold;
    c->flags |= CF_ENABLED;
    int touching = 0;
    int wasTouching = (c->flags & CF_TOUCHING) == CF_TOUCHING;
    Body *bodyA = c->fixtureA->body;
    Body *bodyB = c->fixtureB->body;
    Transform xfA = bodyA->xf;
    Transform xfB = bodyB->xf;
    contact_evaluate(c, &c->manifold, &xfA, &xfB);
    touching = c->manifold.pointCount > 0;
    for (int i = 0; i < c->manifold.pointCount; ++i) {
        ManifoldPoint *mp2 = &c->manifold.points[i];
        mp2->normalImpulse = 0.0f;
        mp2->tangentImpulse = 0.0f;
        uint32_t id2 = cid_key(&mp2->id);
        for (int j = 0; j < oldManifold.pointCount; ++j) {
            ManifoldPoint *mp1 = &oldManifold.points[j];
            if (cid_key(&mp1->id) == id2) {
                mp2->normalImpulse = mp1->normalImpulse;
                mp2->tangentImpulse = mp1->tangentImpulse;
                break;
            }
        }
    }
    if (touching != wasTouching) {
        body_setAwake(bodyA, 1);
        body_setAwake(bodyB, 1);
    }
    if (touching)
        c->flags |= CF_TOUCHING;
    else
        c->flags &= ~CF_TOUCHING;
}

/* ---------------- ContactManager ---------------- */
typedef struct {
    BroadPhase bp;
    Contact *contactList;
    int contactCount;
} ContactManager;

static int filter_shouldCollide(const Filter *a, const Filter *b)
{
    if (a->groupIndex == b->groupIndex && a->groupIndex != 0)
        return a->groupIndex > 0;
    return (a->maskBits & b->categoryBits) != 0 && (a->categoryBits & b->maskBits) != 0;
}

static int body_shouldCollide(const Body *b, const Body *other)
{
    if (b->type != BODY_DYNAMIC && other->type != BODY_DYNAMIC)
        return 0;
    return 1;
}

/* World.popContact: registers are (edge,poly) primary, (poly,edge) swapped, (poly,poly). */
static Contact *cm_popContact(Fixture *fA, int indexA, Fixture *fB, int indexB)
{
    int tA = fA->shape.type, tB = fB->shape.type;
    Contact *c = (Contact *)calloc(1, sizeof(Contact));
    if (tA == SHAPE_POLYGON && tB == SHAPE_POLYGON) {
        c->kind = CONTACT_POLYGON;
        contact_init(c, fA, indexA, fB, indexB);
    } else if (tA == SHAPE_EDGE && tB == SHAPE_POLYGON) {
        c->kind = CONTACT_EDGE_POLYGON;
        contact_init(c, fA, indexA, fB, indexB);
    } else if (tA == SHAPE_POLYGON && tB == SHAPE_EDGE) {
        c->kind = CONTACT_EDGE_POLYGON;
        contact_init(c, fB, indexB, fA, indexA);
    } else {
        free(c);
        return NULL;
    }
    return c;
}

static void cm_pushContact(Contact *c)
{
    Fixture *fA = c->fixtureA;
    Fixture *fB = c->fixtureB;
    if (c->manifold.pointCount > 0 && !fA->isSensor && !fB->isSensor) {
        body_setAwake(fA->body, 1);
        body_setAwake(fB->body, 1);
    }
    free(c);
}

static void cm_addPair(ContactManager *cm, FixtureProxy *proxyA, FixtureProxy *proxyB)
{
    Fixture *fixtureA = proxyA->fixture;
    Fixture *fixtureB = proxyB->fixture;
    int indexA = proxyA->childIndex;
    int indexB = proxyB->childIndex;
    Body *bodyA = fixtureA->body;
    Body *bodyB = fixtureB->body;
    if (bodyA == bodyB)
        return;
    for (ContactEdge *edge = bodyB->contactList; edge != NULL; edge = edge->next) {
        if (edge->other == bodyA) {
            Fixture *fA = edge->contact->fixtureA;
            Fixture *fB = edge->contact->fixtureB;
            int iA = edge->contact->indexA;
            int iB = edge->contact->indexB;
            if (fA == fixtureA && iA == indexA && fB == fixtureB && iB == indexB)
                return;
            if (fA == fixtureB && iA == indexB && fB == fixtureA && iB == indexA)
                return;
        }
    }
    if (body_shouldCollide(bodyB, bodyA) == 0)
        return;
    if (filter_shouldCollide(&fixtureA->filter, &fixtureB->filter) == 0)
        return;
    Contact *c = cm_popContact(fixtureA, indexA, fixtureB, indexB);
    if (c == NULL)
        return;
    fixtureA = c->fixtureA;
    fixtureB = c->fixtureB;
    bodyA = fixtureA->body;
    bodyB = fixtureB->body;
    c->prev = NULL;
    c->next = cm->contactList;
    if (cm->contactList != NULL)
        cm->contactList->prev = c;
    cm->contactList = c;
    c->nodeA.contact = c;
    c->nodeA.other = bodyB;
    c->nodeA.prev = NULL;
    c->nodeA.next = bodyA->contactList;
    if (bodyA->contactList != NULL)
        bodyA->contactList->prev = &c->nodeA;
    bodyA->contactList = &c->nodeA;
    c->nodeB.contact = c;
    c->nodeB.other = bodyA;
    c->nodeB.prev = NULL;
    c->nodeB.next = bodyB->contactList;
    if (bodyB->contactList != NULL)
        bodyB->contactList->prev = &c->nodeB;
    bodyB->contactList = &c->nodeB;
    if (!fixtureA->isSensor && !fixtureB->isSensor) {
        body_setAwake(bodyA, 1);
        body_setAwake(bodyB, 1);
    }
    ++cm->contactCount;
}

static void cm_destroy(ContactManager *cm, Contact *c)
{
    Fixture *fixtureA = c->fixtureA;
    Fixture *fixtureB = c->fixtureB;
    Body *bodyA = fixtureA->body;
    Body *bodyB = fixtureB->body;
    if (c->prev != NULL)
        c->prev->next = c->next;
    if (c->next != NULL)
        c->next->prev = c->prev;
    if (c == cm->contactList)
        cm->contactList = c->next;
    if (c->nodeA.prev != NULL)
        c->nodeA.prev->next = c->nodeA.next;
    if (c->nodeA.next != NULL)
        c->nodeA.next->prev = c->nodeA.prev;
    if (&c->nodeA == bodyA->contactList)
        bodyA->contactList = c->nodeA.next;
    if (c->nodeB.prev != NULL)
        c->nodeB.prev->next = c->nodeB.next;
    if (c->nodeB.next != NULL)
        c->nodeB.next->prev = c->nodeB.prev;
    if (&c->nodeB == bodyB->contactList)
        bodyB->contactList = c->nodeB.next;
    cm_pushContact(c);
    --cm->contactCount;
}

static void cm_collide(ContactManager *cm)
{
    Contact *c = cm->contactList;
    while (c != NULL) {
        Fixture *fixtureA = c->fixtureA;
        Fixture *fixtureB = c->fixtureB;
        Body *bodyA = fixtureA->body;
        Body *bodyB = fixtureB->body;
        if ((c->flags & CF_FILTER) == CF_FILTER) {
            if (body_shouldCollide(bodyB, bodyA) == 0) {
                Contact *cNuke = c;
                c = cNuke->next;
                cm_destroy(cm, cNuke);
                continue;
            }
            if (filter_shouldCollide(&fixtureA->filter, &fixtureB->filter) == 0) {
                Contact *cNuke = c;
                c = cNuke->next;
                cm_destroy(cm, cNuke);
                continue;
            }
            c->flags &= ~CF_FILTER;
        }
        int activeA = body_isAwake(bodyA) && bodyA->type != BODY_STATIC;
        int activeB = body_isAwake(bodyB) && bodyB->type != BODY_STATIC;
        if (activeA == 0 && activeB == 0) {
            c = c->next;
            continue;
        }
        int proxyIdA = fixtureA->proxy.proxyId;
        int proxyIdB = fixtureB->proxy.proxyId;
        int overlap = bp_testOverlap(&cm->bp, proxyIdA, proxyIdB);
        if (overlap == 0) {
            Contact *cNuke = c;
            c = cNuke->next;
            cm_destroy(cm, cNuke);
            continue;
        }
        contact_update(c);
        c = c->next;
    }
}

static void bp_updatePairs(BroadPhase *bp, ContactManager *cm)
{
    bp->pairCount = 0;
    for (int i = 0; i < bp->moveCount; ++i) {
        bp->queryProxyId = bp->moveBuffer[i];
        if (bp->queryProxyId == -1)
            continue;
        AABB fatAABB = bp->tree.nodes[bp->queryProxyId].aabb;
        tree_query(&bp->tree, bp, fatAABB);
    }
    bp->moveCount = 0;
    qsort(bp->pairBuffer, (size_t)bp->pairCount, sizeof(Pair), pair_cmp);
    int i = 0;
    while (i < bp->pairCount) {
        Pair primaryPair = bp->pairBuffer[i];
        FixtureProxy *userDataA = (FixtureProxy *)bp->tree.nodes[primaryPair.proxyIdA].userData;
        FixtureProxy *userDataB = (FixtureProxy *)bp->tree.nodes[primaryPair.proxyIdB].userData;
        cm_addPair(cm, userDataA, userDataB);
        ++i;
        while (i < bp->pairCount) {
            Pair pair = bp->pairBuffer[i];
            if (pair.proxyIdA != primaryPair.proxyIdA || pair.proxyIdB != primaryPair.proxyIdB)
                break;
            ++i;
        }
    }
}

/* ---------------- distance (GJK) and time of impact ---------------- */
typedef struct { Vec2 v[MAX_POLYGON_VERTICES]; int count; float radius; } DistanceProxy;

static void proxy_set(DistanceProxy *p, const Shape *shape)
{
    if (shape->type == SHAPE_EDGE) {
        p->v[0] = shape->v1;
        p->v[1] = shape->v2;
        p->count = 2;
        p->radius = shape->radius;
    } else {
        p->count = shape->count;
        p->radius = shape->radius;
        for (int i = 0; i < p->count; i++)
            p->v[i] = shape->v[i];
    }
}

static int proxy_getSupport(const DistanceProxy *p, Vec2 d)
{
    int bestIndex = 0;
    float bestValue = v2dot(p->v[0], d);
    for (int i = 1; i < p->count; i++) {
        float value = v2dot(p->v[i], d);
        if (value > bestValue) {
            bestIndex = i;
            bestValue = value;
        }
    }
    return bestIndex;
}

typedef struct {
    float metric;
    int count;
    int indexA[3];
    int indexB[3];
} SimplexCache;

typedef struct {
    Vec2 wA, wB, w;
    float a;
    int indexA, indexB;
} SVertex;

typedef struct {
    SVertex v[3];
    int count;
} Simplex;

static float simplex_getMetric(const Simplex *s)
{
    switch (s->count) {
    case 1:
        return 0.0f;
    case 2:
        return sqrtf((s->v[0].w.x - s->v[1].w.x) * (s->v[0].w.x - s->v[1].w.x) +
                     (s->v[0].w.y - s->v[1].w.y) * (s->v[0].w.y - s->v[1].w.y));
    case 3: {
        Vec2 case3 = v2sub(s->v[1].w, s->v[0].w);
        Vec2 case33 = v2sub(s->v[2].w, s->v[0].w);
        return v2cross(case3, case33);
    }
    default:
        return 0.0f;
    }
}

static void simplex_readCache(Simplex *s, const SimplexCache *cache, const DistanceProxy *pA,
                              const Transform *tA, const DistanceProxy *pB, const Transform *tB)
{
    s->count = cache->count;
    for (int i = 0; i < s->count; ++i) {
        SVertex *v = &s->v[i];
        v->indexA = cache->indexA[i];
        v->indexB = cache->indexB[i];
        v->wA = xf_mulV(tA, pA->v[v->indexA]);
        v->wB = xf_mulV(tB, pB->v[v->indexB]);
        v->w = v2sub(v->wB, v->wA);
        v->a = 0.0f;
    }
    if (s->count > 1) {
        float metric1 = cache->metric;
        float metric2 = simplex_getMetric(s);
        if (metric2 < 0.5f * metric1 || 2.0f * metric1 < metric2 || metric2 < JB_EPSILON)
            s->count = 0;
    }
    if (s->count == 0) {
        SVertex *v = &s->v[0];
        v->indexA = 0;
        v->indexB = 0;
        v->wA = xf_mulV(tA, pA->v[0]);
        v->wB = xf_mulV(tB, pB->v[0]);
        v->w = v2sub(v->wB, v->wA);
        s->count = 1;
    }
}

static void simplex_writeCache(const Simplex *s, SimplexCache *cache)
{
    cache->metric = simplex_getMetric(s);
    cache->count = s->count;
    for (int i = 0; i < s->count; ++i) {
        cache->indexA[i] = s->v[i].indexA;
        cache->indexB[i] = s->v[i].indexB;
    }
}

static Vec2 simplex_searchDirection(const Simplex *s)
{
    switch (s->count) {
    case 1:
        return V2(-s->v[0].w.x, -s->v[0].w.y);
    case 2: {
        Vec2 e12 = v2sub(s->v[1].w, s->v[0].w);
        Vec2 out = V2(-s->v[0].w.x, -s->v[0].w.y);
        float sgn = v2cross(e12, out);
        if (sgn > 0.0f)
            return v2crossSV(1.0f, e12);
        return v2crossVS(e12, 1.0f);
    }
    default:
        return V2(0.0f, 0.0f);
    }
}

static Vec2 simplex_closestPoint(const Simplex *s)
{
    switch (s->count) {
    case 1:
        return s->v[0].w;
    case 2: {
        Vec2 case22 = v2scale(s->v[1].w, s->v[1].a);
        return v2add(v2scale(s->v[0].w, s->v[0].a), case22);
    }
    default:
        return V2(0.0f, 0.0f);
    }
}

static void simplex_witnessPoints(const Simplex *s, Vec2 *pA, Vec2 *pB)
{
    switch (s->count) {
    case 1:
        *pA = s->v[0].wA;
        *pB = s->v[0].wB;
        break;
    case 2: {
        Vec2 case2 = v2scale(s->v[0].wA, s->v[0].a);
        *pA = v2add(v2scale(s->v[1].wA, s->v[1].a), case2);
        case2 = v2scale(s->v[0].wB, s->v[0].a);
        *pB = v2add(v2scale(s->v[1].wB, s->v[1].a), case2);
        break;
    }
    case 3: {
        Vec2 pa = v2scale(s->v[0].wA, s->v[0].a);
        Vec2 case3 = v2scale(s->v[1].wA, s->v[1].a);
        Vec2 case33 = v2scale(s->v[2].wA, s->v[2].a);
        pa = v2add(v2add(pa, case3), case33);
        *pA = pa;
        *pB = pa;
        break;
    }
    default:
        break;
    }
}

static void simplex_solve2(Simplex *s)
{
    Vec2 w1 = s->v[0].w;
    Vec2 w2 = s->v[1].w;
    Vec2 e12 = v2sub(w2, w1);
    float d12_2 = -v2dot(w1, e12);
    if (d12_2 <= 0.0f) {
        s->v[0].a = 1.0f;
        s->count = 1;
        return;
    }
    float d12_1 = v2dot(w2, e12);
    if (d12_1 <= 0.0f) {
        s->v[1].a = 1.0f;
        s->count = 1;
        s->v[0] = s->v[1];
        return;
    }
    float inv_d12 = 1.0f / (d12_1 + d12_2);
    s->v[0].a = d12_1 * inv_d12;
    s->v[1].a = d12_2 * inv_d12;
    s->count = 2;
}

static void simplex_solve3(Simplex *s)
{
    Vec2 w1 = s->v[0].w;
    Vec2 w2 = s->v[1].w;
    Vec2 w3 = s->v[2].w;
    Vec2 e12 = v2sub(w2, w1);
    float w1e12 = v2dot(w1, e12);
    float w2e12 = v2dot(w2, e12);
    float d12_1 = w2e12;
    float d12_2 = -w1e12;
    Vec2 e13 = v2sub(w3, w1);
    float w1e13 = v2dot(w1, e13);
    float w3e13 = v2dot(w3, e13);
    float d13_1 = w3e13;
    float d13_2 = -w1e13;
    Vec2 e23 = v2sub(w3, w2);
    float w2e23 = v2dot(w2, e23);
    float w3e23 = v2dot(w3, e23);
    float d23_1 = w3e23;
    float d23_2 = -w2e23;
    float n123 = v2cross(e12, e13);
    float d123_1 = n123 * v2cross(w2, w3);
    float d123_2 = n123 * v2cross(w3, w1);
    float d123_3 = n123 * v2cross(w1, w2);
    if (d12_2 <= 0.0f && d13_2 <= 0.0f) {
        s->v[0].a = 1.0f;
        s->count = 1;
        return;
    }
    if (d12_1 > 0.0f && d12_2 > 0.0f && d123_3 <= 0.0f) {
        float inv_d12 = 1.0f / (d12_1 + d12_2);
        s->v[0].a = d12_1 * inv_d12;
        s->v[1].a = d12_2 * inv_d12;
        s->count = 2;
        return;
    }
    if (d13_1 > 0.0f && d13_2 > 0.0f && d123_2 <= 0.0f) {
        float inv_d13 = 1.0f / (d13_1 + d13_2);
        s->v[0].a = d13_1 * inv_d13;
        s->v[2].a = d13_2 * inv_d13;
        s->count = 2;
        s->v[1] = s->v[2];
        return;
    }
    if (d12_1 <= 0.0f && d23_2 <= 0.0f) {
        s->v[1].a = 1.0f;
        s->count = 1;
        s->v[0] = s->v[1];
        return;
    }
    if (d13_1 <= 0.0f && d23_1 <= 0.0f) {
        s->v[2].a = 1.0f;
        s->count = 1;
        s->v[0] = s->v[2];
        return;
    }
    if (d23_1 > 0.0f && d23_2 > 0.0f && d123_1 <= 0.0f) {
        float inv_d23 = 1.0f / (d23_1 + d23_2);
        s->v[1].a = d23_1 * inv_d23;
        s->v[2].a = d23_2 * inv_d23;
        s->count = 2;
        s->v[0] = s->v[2];
        return;
    }
    float inv_d123 = 1.0f / (d123_1 + d123_2 + d123_3);
    s->v[0].a = d123_1 * inv_d123;
    s->v[1].a = d123_2 * inv_d123;
    s->v[2].a = d123_3 * inv_d123;
    s->count = 3;
}

typedef struct {
    const DistanceProxy *proxyA, *proxyB;
    Transform transformA, transformB;
    int useRadii;
} DistanceInput;

typedef struct {
    Vec2 pointA, pointB;
    float distance;
    int iterations;
} DistanceOutput;

/* Distance.distance (GJK) */
static void distance_compute(DistanceOutput *output, SimplexCache *cache, const DistanceInput *input)
{
    const DistanceProxy *proxyA = input->proxyA;
    const DistanceProxy *proxyB = input->proxyB;
    Transform transformA = input->transformA;
    Transform transformB = input->transformB;
    Simplex simplex;
    simplex.count = 0;
    memset(simplex.v, 0, sizeof simplex.v);
    simplex_readCache(&simplex, cache, proxyA, &transformA, proxyB, &transformB);
    SVertex *vertices = simplex.v;
    int saveCount = 0;
    int saveA[3] = {0, 0, 0};
    int saveB[3] = {0, 0, 0};
    Vec2 closestPoint = simplex_closestPoint(&simplex);
    float distanceSqr1 = v2lenSq(closestPoint);
    float distanceSqr2 = distanceSqr1;
    int iter = 0;
    Vec2 d = V2(0.0f, 0.0f);
    while (iter < GJK_MAX_ITERS) {
        saveCount = simplex.count;
        for (int i = 0; i < saveCount; i++) {
            saveA[i] = vertices[i].indexA;
            saveB[i] = vertices[i].indexB;
        }
        switch (simplex.count) {
        case 1:
            break;
        case 2:
            simplex_solve2(&simplex);
            break;
        case 3:
            simplex_solve3(&simplex);
            break;
        default:
            break;
        }
        if (simplex.count == 3)
            break;
        closestPoint = simplex_closestPoint(&simplex);
        distanceSqr2 = v2lenSq(closestPoint);
        distanceSqr1 = distanceSqr2;
        d = simplex_searchDirection(&simplex);
        if (v2lenSq(d) < JB_EPSILON * JB_EPSILON)
            break;
        SVertex *vertex = &vertices[simplex.count];
        /* Java negates d in place for proxy A and negates it back for proxy B. */
        d = V2(-d.x, -d.y);
        Vec2 temp = rot_mulTV(transformA.q, d);
        vertex->indexA = proxy_getSupport(proxyA, temp);
        vertex->wA = xf_mulV(&transformA, proxyA->v[vertex->indexA]);
        d = V2(-d.x, -d.y);
        temp = rot_mulTV(transformB.q, d);
        vertex->indexB = proxy_getSupport(proxyB, temp);
        vertex->wB = xf_mulV(&transformB, proxyB->v[vertex->indexB]);
        vertex->w = v2sub(vertex->wB, vertex->wA);
        ++iter;
        int duplicate = 0;
        for (int i = 0; i < saveCount; ++i) {
            if (vertex->indexA == saveA[i] && vertex->indexB == saveB[i]) {
                duplicate = 1;
                break;
            }
        }
        if (duplicate)
            break;
        ++simplex.count;
    }
    output->pointA = V2(0.0f, 0.0f);
    output->pointB = V2(0.0f, 0.0f);
    simplex_witnessPoints(&simplex, &output->pointA, &output->pointB);
    output->distance = sqrtf((output->pointB.x - output->pointA.x) * (output->pointB.x - output->pointA.x) +
                             (output->pointB.y - output->pointA.y) * (output->pointB.y - output->pointA.y));
    output->iterations = iter;
    simplex_writeCache(&simplex, cache);
    if (input->useRadii) {
        float rA = proxyA->radius;
        float rB = proxyB->radius;
        if (output->distance > rA + rB && output->distance > JB_EPSILON) {
            output->distance -= rA + rB;
            Vec2 normal = v2sub(output->pointB, output->pointA);
            v2normalize(&normal);
            Vec2 temp = v2scale(normal, rA);
            output->pointA = v2add(output->pointA, temp);
            temp = v2scale(normal, rB);
            output->pointB = v2sub(output->pointB, temp);
        } else {
            Vec2 mid = v2scale(v2add(output->pointA, output->pointB), .5f);
            output->pointA = mid;
            output->pointB = mid;
            output->distance = 0.0f;
        }
    }
}

/* TimeOfImpact.java: SeparationFunction */
enum { SEP_POINTS = 0, SEP_FACE_A = 1, SEP_FACE_B = 2 };
typedef struct {
    const DistanceProxy *proxyA, *proxyB;
    const Sweep *sweepA, *sweepB;
    int type;
    Vec2 localPoint;
    Vec2 axis;
} SepFunction;

static float sep_initialize(SepFunction *f, const SimplexCache *cache, const DistanceProxy *proxyA,
                            const Sweep *sweepA, const DistanceProxy *proxyB, const Sweep *sweepB,
                            float t1)
{
    f->proxyA = proxyA;
    f->proxyB = proxyB;
    int count = cache->count;
    f->sweepA = sweepA;
    f->sweepB = sweepB;
    Transform xfa = sweep_getTransform(sweepA, t1);
    Transform xfb = sweep_getTransform(sweepB, t1);
    if (count == 1) {
        f->type = SEP_POINTS;
        Vec2 localPointA = proxyA->v[cache->indexA[0]];
        Vec2 localPointB = proxyB->v[cache->indexB[0]];
        Vec2 pointA = xf_mulV(&xfa, localPointA);
        Vec2 pointB = xf_mulV(&xfb, localPointB);
        f->axis = v2sub(pointB, pointA);
        return v2normalize(&f->axis);
    } else if (cache->indexA[0] == cache->indexA[1]) {
        f->type = SEP_FACE_B;
        Vec2 localPointB1 = proxyB->v[cache->indexB[0]];
        Vec2 localPointB2 = proxyB->v[cache->indexB[1]];
        Vec2 temp = v2sub(localPointB2, localPointB1);
        f->axis = v2crossVS(temp, 1.0f);
        v2normalize(&f->axis);
        Vec2 normal = rot_mulV(xfb.q, f->axis);
        f->localPoint = v2scale(v2add(localPointB1, localPointB2), .5f);
        Vec2 pointB = xf_mulV(&xfb, f->localPoint);
        Vec2 localPointA = proxyA->v[cache->indexA[0]];
        Vec2 pointA = xf_mulV(&xfa, localPointA);
        temp = v2sub(pointA, pointB);
        float s = v2dot(temp, normal);
        if (s < 0.0f) {
            f->axis = V2(-f->axis.x, -f->axis.y);
            s = -s;
        }
        return s;
    } else {
        f->type = SEP_FACE_A;
        Vec2 localPointA1 = proxyA->v[cache->indexA[0]];
        Vec2 localPointA2 = proxyA->v[cache->indexA[1]];
        Vec2 temp = v2sub(localPointA2, localPointA1);
        f->axis = v2crossVS(temp, 1.0f);
        v2normalize(&f->axis);
        Vec2 normal = rot_mulV(xfa.q, f->axis);
        f->localPoint = v2scale(v2add(localPointA1, localPointA2), .5f);
        Vec2 pointA = xf_mulV(&xfa, f->localPoint);
        Vec2 localPointB = proxyB->v[cache->indexB[0]];
        Vec2 pointB = xf_mulV(&xfb, localPointB);
        temp = v2sub(pointB, pointA);
        float s = v2dot(temp, normal);
        if (s < 0.0f) {
            f->axis = V2(-f->axis.x, -f->axis.y);
            s = -s;
        }
        return s;
    }
}

static float sep_findMinSeparation(SepFunction *f, int *indexes, float t)
{
    Transform xfa = sweep_getTransform(f->sweepA, t);
    Transform xfb = sweep_getTransform(f->sweepB, t);
    switch (f->type) {
    case SEP_POINTS: {
        Vec2 axisA = rot_mulTV(xfa.q, f->axis);
        Vec2 negAxis = V2(-f->axis.x, -f->axis.y);
        Vec2 axisB = rot_mulTV(xfb.q, negAxis);
        indexes[0] = proxy_getSupport(f->proxyA, axisA);
        indexes[1] = proxy_getSupport(f->proxyB, axisB);
        Vec2 pointA = xf_mulV(&xfa, f->proxyA->v[indexes[0]]);
        Vec2 pointB = xf_mulV(&xfb, f->proxyB->v[indexes[1]]);
        return v2dot(v2sub(pointB, pointA), f->axis);
    }
    case SEP_FACE_A: {
        Vec2 normal = rot_mulV(xfa.q, f->axis);
        Vec2 pointA = xf_mulV(&xfa, f->localPoint);
        Vec2 negN = V2(-normal.x, -normal.y);
        Vec2 axisB = rot_mulTV(xfb.q, negN);
        indexes[0] = -1;
        indexes[1] = proxy_getSupport(f->proxyB, axisB);
        Vec2 pointB = xf_mulV(&xfb, f->proxyB->v[indexes[1]]);
        return v2dot(v2sub(pointB, pointA), normal);
    }
    case SEP_FACE_B: {
        Vec2 normal = rot_mulV(xfb.q, f->axis);
        Vec2 pointB = xf_mulV(&xfb, f->localPoint);
        Vec2 negN = V2(-normal.x, -normal.y);
        Vec2 axisA = rot_mulTV(xfa.q, negN);
        indexes[1] = -1;
        indexes[0] = proxy_getSupport(f->proxyA, axisA);
        Vec2 pointA = xf_mulV(&xfa, f->proxyA->v[indexes[0]]);
        return v2dot(v2sub(pointA, pointB), normal);
    }
    default:
        indexes[0] = -1;
        indexes[1] = -1;
        return 0.0f;
    }
}

static float sep_evaluate(SepFunction *f, int indexA, int indexB, float t)
{
    Transform xfa = sweep_getTransform(f->sweepA, t);
    Transform xfb = sweep_getTransform(f->sweepB, t);
    switch (f->type) {
    case SEP_POINTS: {
        Vec2 axisA = rot_mulTV(xfa.q, f->axis);
        Vec2 negAxis = V2(-f->axis.x, -f->axis.y);
        Vec2 axisB = rot_mulTV(xfb.q, negAxis);
        (void)axisA;
        (void)axisB;
        Vec2 pointA = xf_mulV(&xfa, f->proxyA->v[indexA]);
        Vec2 pointB = xf_mulV(&xfb, f->proxyB->v[indexB]);
        return v2dot(v2sub(pointB, pointA), f->axis);
    }
    case SEP_FACE_A: {
        Vec2 normal = rot_mulV(xfa.q, f->axis);
        Vec2 pointA = xf_mulV(&xfa, f->localPoint);
        Vec2 negN = V2(-normal.x, -normal.y);
        Vec2 axisB = rot_mulTV(xfb.q, negN);
        (void)axisB;
        Vec2 pointB = xf_mulV(&xfb, f->proxyB->v[indexB]);
        return v2dot(v2sub(pointB, pointA), normal);
    }
    case SEP_FACE_B: {
        Vec2 normal = rot_mulV(xfb.q, f->axis);
        Vec2 pointB = xf_mulV(&xfb, f->localPoint);
        Vec2 negN = V2(-normal.x, -normal.y);
        Vec2 axisA = rot_mulTV(xfa.q, negN);
        (void)axisA;
        Vec2 pointA = xf_mulV(&xfa, f->proxyA->v[indexA]);
        return v2dot(v2sub(pointA, pointB), normal);
    }
    default:
        return 0.0f;
    }
}

typedef enum { TOI_UNKNOWN, TOI_FAILED, TOI_OVERLAPPED, TOI_TOUCHING, TOI_SEPARATED } TOIState;
typedef struct {
    DistanceProxy proxyA, proxyB;
    Sweep sweepA, sweepB;
    float tMax;
} TOIInput;
typedef struct {
    TOIState state;
    float t;
} TOIOutput;

/* TimeOfImpact.timeOfImpact */
static void toi_timeOfImpact(TOIOutput *output, const TOIInput *input)
{
    output->state = TOI_UNKNOWN;
    output->t = input->tMax;
    const DistanceProxy *proxyA = &input->proxyA;
    const DistanceProxy *proxyB = &input->proxyB;
    Sweep sweepA = input->sweepA;
    Sweep sweepB = input->sweepB;
    sweep_normalize(&sweepA);
    sweep_normalize(&sweepB);
    float tMax = input->tMax;
    float totalRadius = proxyA->radius + proxyB->radius;
    float target = jb_max(LINEAR_SLOP, totalRadius - 3.0f * LINEAR_SLOP);
    float tolerance = 0.25f * LINEAR_SLOP;
    float t1 = 0.0f;
    int iter = 0;
    SimplexCache cache;
    memset(&cache, 0, sizeof cache);
    cache.count = 0;
    DistanceInput distanceInput;
    distanceInput.proxyA = proxyA;
    distanceInput.proxyB = proxyB;
    distanceInput.useRadii = 0;
    for (;;) {
        Transform xfA = sweep_getTransform(&sweepA, t1);
        Transform xfB = sweep_getTransform(&sweepB, t1);
        distanceInput.transformA = xfA;
        distanceInput.transformB = xfB;
        DistanceOutput distanceOutput;
        distance_compute(&distanceOutput, &cache, &distanceInput);
        if (distanceOutput.distance <= 0.0f) {
            output->state = TOI_OVERLAPPED;
            output->t = 0.0f;
            break;
        }
        if (distanceOutput.distance < target + tolerance) {
            output->state = TOI_TOUCHING;
            output->t = t1;
            break;
        }
        SepFunction fcn;
        memset(&fcn, 0, sizeof fcn);
        sep_initialize(&fcn, &cache, proxyA, &sweepA, proxyB, &sweepB, t1);
        int done = 0;
        float t2 = tMax;
        int pushBackIter = 0;
        int indexes[2] = {0, 0};
        for (;;) {
            float s2 = sep_findMinSeparation(&fcn, indexes, t2);
            if (s2 > target + tolerance) {
                output->state = TOI_SEPARATED;
                output->t = tMax;
                done = 1;
                break;
            }
            if (s2 > target - tolerance) {
                t1 = t2;
                break;
            }
            float s1 = sep_evaluate(&fcn, indexes[0], indexes[1], t1);
            if (s1 < target - tolerance) {
                output->state = TOI_FAILED;
                output->t = t1;
                done = 1;
                break;
            }
            if (s1 <= target + tolerance) {
                output->state = TOI_TOUCHING;
                output->t = t1;
                done = 1;
                break;
            }
            int rootIterCount = 0;
            float a1 = t1, a2 = t2;
            for (;;) {
                float t;
                if ((rootIterCount & 1) == 1)
                    t = a1 + (target - s1) * (a2 - a1) / (s2 - s1);
                else
                    t = 0.5f * (a1 + a2);
                float s = sep_evaluate(&fcn, indexes[0], indexes[1], t);
                if (jb_abs(s - target) < tolerance) {
                    t2 = t;
                    break;
                }
                if (s > target) {
                    a1 = t;
                    s1 = s;
                } else {
                    a2 = t;
                    s2 = s;
                }
                ++rootIterCount;
                if (rootIterCount == TOI_TARGET_ROOT_ITERS)
                    break;
            }
            ++pushBackIter;
            if (pushBackIter == MAX_POLYGON_VERTICES)
                break;
        }
        ++iter;
        if (done)
            break;
        if (iter == TOI_MAX_ITERATIONS) {
            output->state = TOI_FAILED;
            output->t = t1;
            break;
        }
    }
}

/* ---------------- sequential-impulse contact solver ---------------- */
typedef struct {
    Vec2 rA, rB;
    float normalImpulse, tangentImpulse, normalMass, tangentMass, velocityBias;
} VCPoint;

typedef struct {
    VCPoint points[MAX_MANIFOLD_POINTS];
    Vec2 normal;
    Mat22 normalMass, K;
    int indexA, indexB;
    float invMassA, invMassB, invIA, invIB;
    float friction, restitution, tangentSpeed;
    int pointCount;
    int contactIndex;
} VelConstraint;

typedef struct {
    Vec2 localPoints[MAX_MANIFOLD_POINTS];
    Vec2 localNormal, localPoint;
    int indexA, indexB;
    float invMassA, invMassB;
    Vec2 localCenterA, localCenterB;
    float invIA, invIB;
    int type;
    float radiusA, radiusB;
    int pointCount;
} PosConstraint;

typedef struct {
    TimeStep *step;
    Position *positions;
    Velocity *velocities;
    PosConstraint *pcs;
    VelConstraint *vcs;
    int pcCap, vcCap;
    Contact **contacts;
    int count;
} ContactSolver;

static void cs_ensure(ContactSolver *s, int count)
{
    if (s->pcCap < count) {
        int newCap = s->pcCap * 2 > count ? s->pcCap * 2 : count;
        s->pcs = (PosConstraint *)realloc(s->pcs, sizeof(PosConstraint) * (size_t)newCap);
        s->pcCap = newCap;
    }
    if (s->vcCap < count) {
        int newCap = s->vcCap * 2 > count ? s->vcCap * 2 : count;
        s->vcs = (VelConstraint *)realloc(s->vcs, sizeof(VelConstraint) * (size_t)newCap);
        s->vcCap = newCap;
    }
}

static void cs_init(ContactSolver *s, TimeStep *step, Contact **contacts, int count, Position *positions,
                    Velocity *velocities)
{
    s->step = step;
    s->count = count;
    cs_ensure(s, count);
    s->positions = positions;
    s->velocities = velocities;
    s->contacts = contacts;
    for (int i = 0; i < count; ++i) {
        Contact *contact = contacts[i];
        Fixture *fixtureA = contact->fixtureA;
        Fixture *fixtureB = contact->fixtureB;
        float radiusA = fixtureA->shape.radius;
        float radiusB = fixtureB->shape.radius;
        Body *bodyA = fixtureA->body;
        Body *bodyB = fixtureB->body;
        Manifold *manifold = &contact->manifold;
        int pointCount = manifold->pointCount;
        VelConstraint *vc = &s->vcs[i];
        vc->friction = contact->friction;
        vc->restitution = contact->restitution;
        vc->tangentSpeed = contact->tangentSpeed;
        vc->indexA = bodyA->islandIndex;
        vc->indexB = bodyB->islandIndex;
        vc->invMassA = bodyA->invMass;
        vc->invMassB = bodyB->invMass;
        vc->invIA = bodyA->invI;
        vc->invIB = bodyB->invI;
        vc->contactIndex = i;
        vc->pointCount = pointCount;
        memset(&vc->K, 0, sizeof vc->K);
        memset(&vc->normalMass, 0, sizeof vc->normalMass);
        PosConstraint *pc = &s->pcs[i];
        pc->indexA = bodyA->islandIndex;
        pc->indexB = bodyB->islandIndex;
        pc->invMassA = bodyA->invMass;
        pc->invMassB = bodyB->invMass;
        pc->localCenterA = bodyA->sweep.localCenter;
        pc->localCenterB = bodyB->sweep.localCenter;
        pc->invIA = bodyA->invI;
        pc->invIB = bodyB->invI;
        pc->localNormal = manifold->localNormal;
        pc->localPoint = manifold->localPoint;
        pc->pointCount = pointCount;
        pc->radiusA = radiusA;
        pc->radiusB = radiusB;
        pc->type = manifold->type;
        for (int j = 0; j < pointCount; j++) {
            ManifoldPoint *cp = &manifold->points[j];
            VCPoint *vcp = &vc->points[j];
            if (step->warmStarting) {
                vcp->normalImpulse = step->dtRatio * cp->normalImpulse;
                vcp->tangentImpulse = step->dtRatio * cp->tangentImpulse;
            } else {
                vcp->normalImpulse = 0;
                vcp->tangentImpulse = 0;
            }
            vcp->rA = V2(0.0f, 0.0f);
            vcp->rB = V2(0.0f, 0.0f);
            vcp->normalMass = 0;
            vcp->tangentMass = 0;
            vcp->velocityBias = 0;
            pc->localPoints[j].x = cp->localPoint.x;
            pc->localPoints[j].y = cp->localPoint.y;
        }
    }
}

static void cs_warmStart(ContactSolver *s)
{
    for (int i = 0; i < s->count; ++i) {
        VelConstraint *vc = &s->vcs[i];
        int indexA = vc->indexA;
        int indexB = vc->indexB;
        float mA = vc->invMassA;
        float iA = vc->invIA;
        float mB = vc->invMassB;
        float iB = vc->invIB;
        int pointCount = vc->pointCount;
        Vec2 vA = s->velocities[indexA].v;
        float wA = s->velocities[indexA].w;
        Vec2 vB = s->velocities[indexB].v;
        float wB = s->velocities[indexB].w;
        Vec2 normal = vc->normal;
        float tangentx = 1.0f * normal.y;
        float tangenty = -1.0f * normal.x;
        for (int j = 0; j < pointCount; ++j) {
            VCPoint *vcp = &vc->points[j];
            float Px = tangentx * vcp->tangentImpulse + normal.x * vcp->normalImpulse;
            float Py = tangenty * vcp->tangentImpulse + normal.y * vcp->normalImpulse;
            wA -= iA * (vcp->rA.x * Py - vcp->rA.y * Px);
            vA.x -= Px * mA;
            vA.y -= Py * mA;
            wB += iB * (vcp->rB.x * Py - vcp->rB.y * Px);
            vB.x += Px * mB;
            vB.y += Py * mB;
        }
        s->velocities[indexA].v = vA;
        s->velocities[indexA].w = wA;
        s->velocities[indexB].v = vB;
        s->velocities[indexB].w = wB;
    }
}

static void cs_initializeVelocityConstraints(ContactSolver *s)
{
    for (int i = 0; i < s->count; ++i) {
        VelConstraint *vc = &s->vcs[i];
        PosConstraint *pc = &s->pcs[i];
        float radiusA = pc->radiusA;
        float radiusB = pc->radiusB;
        Manifold *manifold = &s->contacts[vc->contactIndex]->manifold;
        int indexA = vc->indexA;
        int indexB = vc->indexB;
        float mA = vc->invMassA;
        float mB = vc->invMassB;
        float iA = vc->invIA;
        float iB = vc->invIB;
        Vec2 localCenterA = pc->localCenterA;
        Vec2 localCenterB = pc->localCenterB;
        Vec2 cA = s->positions[indexA].c;
        float aA = s->positions[indexA].a;
        Vec2 vA = s->velocities[indexA].v;
        float wA = s->velocities[indexA].w;
        Vec2 cB = s->positions[indexB].c;
        float aB = s->positions[indexB].a;
        Vec2 vB = s->velocities[indexB].v;
        float wB = s->velocities[indexB].w;
        Transform xfA, xfB;
        xfA.q = rot_set(aA);
        xfB.q = rot_set(aB);
        xfA.p.x = cA.x - (xfA.q.c * localCenterA.x - xfA.q.s * localCenterA.y);
        xfA.p.y = cA.y - (xfA.q.s * localCenterA.x + xfA.q.c * localCenterA.y);
        xfB.p.x = cB.x - (xfB.q.c * localCenterB.x - xfB.q.s * localCenterB.y);
        xfB.p.y = cB.y - (xfB.q.s * localCenterB.x + xfB.q.c * localCenterB.y);
        WorldManifold wm;
        memset(&wm, 0, sizeof wm);
        worldManifold_init(&wm, manifold, &xfA, radiusA, &xfB, radiusB);
        vc->normal = wm.normal;
        int pointCount = vc->pointCount;
        for (int j = 0; j < pointCount; ++j) {
            VCPoint *vcp = &vc->points[j];
            vcp->rA = v2sub(wm.points[j], cA);
            vcp->rB = v2sub(wm.points[j], cB);
            float rnA = vcp->rA.x * vc->normal.y - vcp->rA.y * vc->normal.x;
            float rnB = vcp->rB.x * vc->normal.y - vcp->rB.y * vc->normal.x;
            float kNormal = mA + mB + iA * rnA * rnA + iB * rnB * rnB;
            vcp->normalMass = kNormal > 0.0f ? 1.0f / kNormal : 0.0f;
            float tangentx = 1.0f * vc->normal.y;
            float tangenty = -1.0f * vc->normal.x;
            float rtA = vcp->rA.x * tangenty - vcp->rA.y * tangentx;
            float rtB = vcp->rB.x * tangenty - vcp->rB.y * tangentx;
            float kTangent = mA + mB + iA * rtA * rtA + iB * rtB * rtB;
            vcp->tangentMass = kTangent > 0.0f ? 1.0f / kTangent : 0.0f;
            vcp->velocityBias = 0.0f;
            float tempx = vB.x + -wB * vcp->rB.y - vA.x - (-wA * vcp->rA.y);
            float tempy = vB.y + wB * vcp->rB.x - vA.y - (wA * vcp->rA.x);
            float vRel = vc->normal.x * tempx + vc->normal.y * tempy;
            if (vRel < -VELOCITY_THRESHOLD)
                vcp->velocityBias = -vc->restitution * vRel;
        }
        if (vc->pointCount == 2) {
            VCPoint *vcp1 = &vc->points[0];
            VCPoint *vcp2 = &vc->points[1];
            float rn1A = v2cross(vcp1->rA, vc->normal);
            float rn1B = v2cross(vcp1->rB, vc->normal);
            float rn2A = v2cross(vcp2->rA, vc->normal);
            float rn2B = v2cross(vcp2->rB, vc->normal);
            float k11 = mA + mB + iA * rn1A * rn1A + iB * rn1B * rn1B;
            float k22 = mA + mB + iA * rn2A * rn2A + iB * rn2B * rn2B;
            float k12 = mA + mB + iA * rn1A * rn2A + iB * rn1B * rn2B;
            if (k11 * k11 < 100.0f * (k11 * k22 - k12 * k12)) {
                vc->K.ex = V2(k11, k12);
                vc->K.ey = V2(k12, k22);
                vc->normalMass = mat22_invert(&vc->K);
            } else {
                vc->pointCount = 1;
            }
        }
    }
}

/* Mat22 block solve: one step of the 2-point LCP enumeration in ContactSolver. */
static void cs_solveVelocityConstraints(ContactSolver *s)
{
    for (int i = 0; i < s->count; ++i) {
        VelConstraint *vc = &s->vcs[i];
        int indexA = vc->indexA;
        int indexB = vc->indexB;
        float mA = vc->invMassA;
        float mB = vc->invMassB;
        float iA = vc->invIA;
        float iB = vc->invIB;
        int pointCount = vc->pointCount;
        Vec2 vA = s->velocities[indexA].v;
        float wA = s->velocities[indexA].w;
        Vec2 vB = s->velocities[indexB].v;
        float wB = s->velocities[indexB].w;
        Vec2 normal = vc->normal;
        Vec2 tangent = V2(1.0f * vc->normal.y, -1.0f * vc->normal.x);
        float friction = vc->friction;
        for (int j = 0; j < pointCount; ++j) {
            VCPoint *vcp = &vc->points[j];
            Vec2 a = vcp->rA;
            float dvx = -wB * vcp->rB.y + vB.x - vA.x + wA * a.y;
            float dvy = wB * vcp->rB.x + vB.y - vA.y - wA * a.x;
            float vt = dvx * tangent.x + dvy * tangent.y - vc->tangentSpeed;
            float lambda = vcp->tangentMass * (-vt);
            float maxFriction = friction * vcp->normalImpulse;
            float newImpulse = jb_clamp(vcp->tangentImpulse + lambda, -maxFriction, maxFriction);
            lambda = newImpulse - vcp->tangentImpulse;
            vcp->tangentImpulse = newImpulse;
            float Px = tangent.x * lambda;
            float Py = tangent.y * lambda;
            vA.x -= Px * mA;
            vA.y -= Py * mA;
            wA -= iA * (vcp->rA.x * Py - vcp->rA.y * Px);
            vB.x += Px * mB;
            vB.y += Py * mB;
            wB += iB * (vcp->rB.x * Py - vcp->rB.y * Px);
        }
        if (vc->pointCount == 1) {
            VCPoint *vcp = &vc->points[0];
            float dvx = -wB * vcp->rB.y + vB.x - vA.x + wA * vcp->rA.y;
            float dvy = wB * vcp->rB.x + vB.y - vA.y - wA * vcp->rA.x;
            float vn = dvx * normal.x + dvy * normal.y;
            float lambda = -vcp->normalMass * (vn - vcp->velocityBias);
            float a = vcp->normalImpulse + lambda;
            float newImpulse = (a > 0.0f ? a : 0.0f);
            lambda = newImpulse - vcp->normalImpulse;
            vcp->normalImpulse = newImpulse;
            float Px = normal.x * lambda;
            float Py = normal.y * lambda;
            vA.x -= Px * mA;
            vA.y -= Py * mA;
            wA -= iA * (vcp->rA.x * Py - vcp->rA.y * Px);
            vB.x += Px * mB;
            vB.y += Py * mB;
            wB += iB * (vcp->rB.x * Py - vcp->rB.y * Px);
        } else {
            VCPoint *cp1 = &vc->points[0];
            VCPoint *cp2 = &vc->points[1];
            Vec2 a, b, x, d, P1, P2, temp1, temp2, dv1, dv2;
            a = V2(cp1->normalImpulse, cp2->normalImpulse);
            dv1.x = -wB * cp1->rB.y + vB.x - vA.x + wA * cp1->rA.y;
            dv1.y = wB * cp1->rB.x + vB.y - vA.y - wA * cp1->rA.x;
            dv2.x = -wB * cp2->rB.y + vB.x - vA.x + wA * cp2->rA.y;
            dv2.y = wB * cp2->rB.x + vB.y - vA.y - wA * cp2->rA.x;
            float vn1 = dv1.x * normal.x + dv1.y * normal.y;
            float vn2 = dv2.x * normal.x + dv2.y * normal.y;
            b.x = vn1 - cp1->velocityBias;
            b.y = vn2 - cp2->velocityBias;
            Mat22 R = vc->K;
            b.x -= R.ex.x * a.x + R.ey.x * a.y;
            b.y -= R.ex.y * a.x + R.ey.y * a.y;
            for (;;) {
                x = mat22_mulV(&vc->normalMass, b);
                x.x *= -1;
                x.y *= -1;
                if (x.x >= 0.0f && x.y >= 0.0f) {
                    d = v2sub(x, a);
                    P1 = v2scale(normal, d.x);
                    P2 = v2scale(normal, d.y);
                    temp1 = v2add(P1, P2);
                    temp2 = v2scale(temp1, mA);
                    vA = v2sub(vA, temp2);
                    temp2 = v2scale(temp1, mB);
                    vB = v2add(vB, temp2);
                    wA -= iA * (v2cross(cp1->rA, P1) + v2cross(cp2->rA, P2));
                    wB += iB * (v2cross(cp1->rB, P1) + v2cross(cp2->rB, P2));
                    cp1->normalImpulse = x.x;
                    cp2->normalImpulse = x.y;
                    break;
                }
                x.x = -cp1->normalMass * b.x;
                x.y = 0.0f;
                vn1 = 0.0f;
                vn2 = R.ex.y * x.x + b.y;
                if (x.x >= 0.0f && vn2 >= 0.0f) {
                    d = v2sub(x, a);
                    P1 = v2scale(normal, d.x);
                    P2 = v2scale(normal, d.y);
                    temp1 = v2add(P1, P2);
                    temp2 = v2scale(temp1, mA);
                    vA = v2sub(vA, temp2);
                    temp2 = v2scale(temp1, mB);
                    vB = v2add(vB, temp2);
                    wA -= iA * (v2cross(cp1->rA, P1) + v2cross(cp2->rA, P2));
                    wB += iB * (v2cross(cp1->rB, P1) + v2cross(cp2->rB, P2));
                    cp1->normalImpulse = x.x;
                    cp2->normalImpulse = x.y;
                    break;
                }
                x.x = 0.0f;
                x.y = -cp2->normalMass * b.y;
                vn1 = R.ey.x * x.y + b.x;
                vn2 = 0.0f;
                if (x.y >= 0.0f && vn1 >= 0.0f) {
                    d = v2sub(x, a);
                    P1 = v2scale(normal, d.x);
                    P2 = v2scale(normal, d.y);
                    temp1 = v2add(P1, P2);
                    temp2 = v2scale(temp1, mA);
                    vA = v2sub(vA, temp2);
                    temp2 = v2scale(temp1, mB);
                    vB = v2add(vB, temp2);
                    wA -= iA * (v2cross(cp1->rA, P1) + v2cross(cp2->rA, P2));
                    wB += iB * (v2cross(cp1->rB, P1) + v2cross(cp2->rB, P2));
                    cp1->normalImpulse = x.x;
                    cp2->normalImpulse = x.y;
                    break;
                }
                x.x = 0.0f;
                x.y = 0.0f;
                vn1 = b.x;
                vn2 = b.y;
                if (vn1 >= 0.0f && vn2 >= 0.0f) {
                    d = v2sub(x, a);
                    P1 = v2scale(normal, d.x);
                    P2 = v2scale(normal, d.y);
                    temp1 = v2add(P1, P2);
                    temp2 = v2scale(temp1, mA);
                    vA = v2sub(vA, temp2);
                    temp2 = v2scale(temp1, mB);
                    vB = v2add(vB, temp2);
                    wA -= iA * (v2cross(cp1->rA, P1) + v2cross(cp2->rA, P2));
                    wB += iB * (v2cross(cp1->rB, P1) + v2cross(cp2->rB, P2));
                    cp1->normalImpulse = x.x;
                    cp2->normalImpulse = x.y;
                    break;
                }
                break;
            }
        }
        s->velocities[indexA].v = vA;
        s->velocities[indexA].w = wA;
        s->velocities[indexB].v = vB;
        s->velocities[indexB].w = wB;
    }
}

static void cs_storeImpulses(ContactSolver *s)
{
    for (int i = 0; i < s->count; i++) {
        VelConstraint *vc = &s->vcs[i];
        Manifold *manifold = &s->contacts[vc->contactIndex]->manifold;
        for (int j = 0; j < vc->pointCount; j++) {
            manifold->points[j].normalImpulse = vc->points[j].normalImpulse;
            manifold->points[j].tangentImpulse = vc->points[j].tangentImpulse;
        }
    }
}

/* PositionSolverManifold.initialize (FACE_A / FACE_B) */
static void psm_initialize(const PosConstraint *pc, const Transform *xfA, const Transform *xfB, int index,
                           Vec2 *normal, Vec2 *point, float *separation)
{
    Rot xfAq = xfA->q;
    Rot xfBq = xfB->q;
    Vec2 pcLocalPointsI = pc->localPoints[index];
    if (pc->type == MT_FACE_A) {
        normal->x = xfAq.c * pc->localNormal.x - xfAq.s * pc->localNormal.y;
        normal->y = xfAq.s * pc->localNormal.x + xfAq.c * pc->localNormal.y;
        float planePointx = (xfAq.c * pc->localPoint.x - xfAq.s * pc->localPoint.y) + xfA->p.x;
        float planePointy = (xfAq.s * pc->localPoint.x + xfAq.c * pc->localPoint.y) + xfA->p.y;
        float clipPointx = (xfBq.c * pcLocalPointsI.x - xfBq.s * pcLocalPointsI.y) + xfB->p.x;
        float clipPointy = (xfBq.s * pcLocalPointsI.x + xfBq.c * pcLocalPointsI.y) + xfB->p.y;
        float tempx = clipPointx - planePointx;
        float tempy = clipPointy - planePointy;
        *separation = tempx * normal->x + tempy * normal->y - pc->radiusA - pc->radiusB;
        point->x = clipPointx;
        point->y = clipPointy;
    } else {
        normal->x = xfBq.c * pc->localNormal.x - xfBq.s * pc->localNormal.y;
        normal->y = xfBq.s * pc->localNormal.x + xfBq.c * pc->localNormal.y;
        float planePointx = (xfBq.c * pc->localPoint.x - xfBq.s * pc->localPoint.y) + xfB->p.x;
        float planePointy = (xfBq.s * pc->localPoint.x + xfBq.c * pc->localPoint.y) + xfB->p.y;
        float clipPointx = (xfAq.c * pcLocalPointsI.x - xfAq.s * pcLocalPointsI.y) + xfA->p.x;
        float clipPointy = (xfAq.s * pcLocalPointsI.x + xfAq.c * pcLocalPointsI.y) + xfA->p.y;
        float tempx = clipPointx - planePointx;
        float tempy = clipPointy - planePointy;
        *separation = tempx * normal->x + tempy * normal->y - pc->radiusA - pc->radiusB;
        point->x = clipPointx;
        point->y = clipPointy;
        normal->x *= -1;
        normal->y *= -1;
    }
}

static int cs_solvePositionConstraints(ContactSolver *s)
{
    float minSeparation = 0.0f;
    for (int i = 0; i < s->count; ++i) {
        PosConstraint *pc = &s->pcs[i];
        int indexA = pc->indexA;
        int indexB = pc->indexB;
        float mA = pc->invMassA;
        float iA = pc->invIA;
        Vec2 localCenterA = pc->localCenterA;
        float mB = pc->invMassB;
        float iB = pc->invIB;
        Vec2 localCenterB = pc->localCenterB;
        int pointCount = pc->pointCount;
        Vec2 cA = s->positions[indexA].c;
        float aA = s->positions[indexA].a;
        Vec2 cB = s->positions[indexB].c;
        float aB = s->positions[indexB].a;
        for (int j = 0; j < pointCount; ++j) {
            Transform xfA, xfB;
            xfA.q = rot_set(aA);
            xfB.q = rot_set(aB);
            xfA.p = rot_mulV(xfA.q, localCenterA);
            xfA.p = V2(-xfA.p.x, -xfA.p.y);
            xfA.p = v2add(xfA.p, cA);
            xfB.p = rot_mulV(xfB.q, localCenterB);
            xfB.p = V2(-xfB.p.x, -xfB.p.y);
            xfB.p = v2add(xfB.p, cB);
            Vec2 normal, point;
            float separation;
            psm_initialize(pc, &xfA, &xfB, j, &normal, &point, &separation);
            Vec2 rA = v2sub(point, cA);
            Vec2 rB = v2sub(point, cB);
            minSeparation = jb_min(minSeparation, separation);
            float C = jb_clamp(BAUMGARTE * (separation + LINEAR_SLOP), -MAX_LINEAR_CORRECTION, 0.0f);
            float rnA = v2cross(rA, normal);
            float rnB = v2cross(rB, normal);
            float K = mA + mB + iA * rnA * rnA + iB * rnB * rnB;
            float impulse = K > 0.0f ? -C / K : 0.0f;
            Vec2 P = v2scale(normal, impulse);
            cA = v2sub(cA, v2scale(P, mA));
            aA -= iA * v2cross(rA, P);
            cB = v2add(cB, v2scale(P, mB));
            aB += iB * v2cross(rB, P);
        }
        s->positions[indexA].c = cA;
        s->positions[indexA].a = aA;
        s->positions[indexB].c = cB;
        s->positions[indexB].a = aB;
    }
    return minSeparation >= -3.0f * LINEAR_SLOP;
}

static int cs_solveTOIPositionConstraints(ContactSolver *s, int toiIndexA, int toiIndexB)
{
    float minSeparation = 0.0f;
    for (int i = 0; i < s->count; ++i) {
        PosConstraint *pc = &s->pcs[i];
        int indexA = pc->indexA;
        int indexB = pc->indexB;
        Vec2 localCenterA = pc->localCenterA;
        Vec2 localCenterB = pc->localCenterB;
        int pointCount = pc->pointCount;
        float mA = 0.0f, iA = 0.0f;
        if (indexA == toiIndexA || indexA == toiIndexB) {
            mA = pc->invMassA;
            iA = pc->invIA;
        }
        float mB = 0.0f, iB = 0.0f;
        if (indexB == toiIndexA || indexB == toiIndexB) {
            mB = pc->invMassB;
            iB = pc->invIB;
        }
        Vec2 cA = s->positions[indexA].c;
        float aA = s->positions[indexA].a;
        Vec2 cB = s->positions[indexB].c;
        float aB = s->positions[indexB].a;
        for (int j = 0; j < pointCount; ++j) {
            Transform xfA, xfB;
            xfA.q = rot_set(aA);
            xfB.q = rot_set(aB);
            xfA.p = rot_mulV(xfA.q, localCenterA);
            xfA.p = V2(-xfA.p.x, -xfA.p.y);
            xfA.p = v2add(xfA.p, cA);
            xfB.p = rot_mulV(xfB.q, localCenterB);
            xfB.p = V2(-xfB.p.x, -xfB.p.y);
            xfB.p = v2add(xfB.p, cB);
            Vec2 normal, point;
            float separation;
            psm_initialize(pc, &xfA, &xfB, j, &normal, &point, &separation);
            Vec2 rA = v2sub(point, cA);
            Vec2 rB = v2sub(point, cB);
            minSeparation = jb_min(minSeparation, separation);
            float C = jb_clamp(TOI_BAUMGARTE * (separation + LINEAR_SLOP), -MAX_LINEAR_CORRECTION, 0.0f);
            float rnA = v2cross(rA, normal);
            float rnB = v2cross(rB, normal);
            float K = mA + mB + iA * rnA * rnA + iB * rnB * rnB;
            float impulse = K > 0.0f ? -C / K : 0.0f;
            Vec2 P = v2scale(normal, impulse);
            cA = v2sub(cA, v2scale(P, mA));
            aA -= iA * v2cross(rA, P);
            cB = v2add(cB, v2scale(P, mB));
            aB += iB * v2cross(rB, P);
        }
        s->positions[indexA].c = cA;
        s->positions[indexA].a = aA;
        s->positions[indexB].c = cB;
        s->positions[indexB].a = aB;
    }
    return minSeparation >= -1.5f * LINEAR_SLOP;
}

/* ---------------- Island ---------------- */
struct Island {
    Body **bodies;
    int bodyCount, bodyCapacity, bodyAlloc;
    Contact **contacts;
    int contactCount, contactCapacity, contactAlloc;
    Position *positions;
    Velocity *velocities;
    int posAlloc;
    ContactSolver contactSolver;
    ContactSolver toiContactSolver;
};
typedef struct Island Island;

static void island_init(Island *isl, int bodyCapacity, int contactCapacity)
{
    isl->bodyCapacity = bodyCapacity;
    isl->contactCapacity = contactCapacity;
    isl->bodyCount = 0;
    isl->contactCount = 0;
    if (isl->bodyAlloc < bodyCapacity) {
        isl->bodies = (Body **)realloc(isl->bodies, sizeof(Body *) * (size_t)bodyCapacity);
        isl->bodyAlloc = bodyCapacity;
    }
    if (isl->contactAlloc < contactCapacity) {
        isl->contacts = (Contact **)realloc(isl->contacts, sizeof(Contact *) * (size_t)contactCapacity);
        isl->contactAlloc = contactCapacity;
    }
    if (isl->posAlloc < bodyCapacity) {
        isl->positions = (Position *)realloc(isl->positions, sizeof(Position) * (size_t)bodyCapacity);
        isl->velocities = (Velocity *)realloc(isl->velocities, sizeof(Velocity) * (size_t)bodyCapacity);
        isl->posAlloc = bodyCapacity;
    }
}

static void island_clear(Island *isl)
{
    isl->bodyCount = 0;
    isl->contactCount = 0;
}

static void island_addBody(Island *isl, Body *b)
{
    b->islandIndex = isl->bodyCount;
    isl->bodies[isl->bodyCount] = b;
    ++isl->bodyCount;
}

static void island_addContact(Island *isl, Contact *c)
{
    isl->contacts[isl->contactCount++] = c;
}

static void island_solve(Island *isl, TimeStep *step, Vec2 gravity, int allowSleep)
{
    float h = step->dt;
    for (int i = 0; i < isl->bodyCount; ++i) {
        Body *b = isl->bodies[i];
        Vec2 c = b->sweep.c;
        float a = b->sweep.a;
        Vec2 *v = &b->linearVelocity;
        float w = b->angularVelocity;
        b->sweep.c0 = b->sweep.c;
        b->sweep.a0 = b->sweep.a;
        if (b->type == BODY_DYNAMIC) {
            v->x += h * (b->gravityScale * gravity.x + b->invMass * b->force.x);
            v->y += h * (b->gravityScale * gravity.y + b->invMass * b->force.y);
            w += h * b->invI * b->torque;
            float a1 = jb_clamp(1.0f - h * b->linearDamping, 0.0f, 1.0f);
            v->x *= a1;
            v->y *= a1;
            w *= jb_clamp(1.0f - h * b->angularDamping, 0.0f, 1.0f);
        }
        isl->positions[i].c = c;
        isl->positions[i].a = a;
        isl->velocities[i].v = *v;
        isl->velocities[i].w = w;
    }

    isl->contactSolver.step = step;
    cs_init(&isl->contactSolver, step, isl->contacts, isl->contactCount, isl->positions, isl->velocities);
    cs_initializeVelocityConstraints(&isl->contactSolver);
    if (step->warmStarting)
        cs_warmStart(&isl->contactSolver);
    for (int i = 0; i < step->velocityIterations; ++i)
        cs_solveVelocityConstraints(&isl->contactSolver);
    cs_storeImpulses(&isl->contactSolver);

    for (int i = 0; i < isl->bodyCount; ++i) {
        Vec2 *c = &isl->positions[i].c;
        float a = isl->positions[i].a;
        Vec2 *v = &isl->velocities[i].v;
        float w = isl->velocities[i].w;
        float translationx = v->x * h;
        float translationy = v->y * h;
        if (translationx * translationx + translationy * translationy > MAX_TRANSLATION_SQ) {
            float ratio = MAX_TRANSLATION / sqrtf(translationx * translationx + translationy * translationy);
            v->x *= ratio;
            v->y *= ratio;
        }
        float rotation = h * w;
        if (rotation * rotation > MAX_ROTATION_SQ) {
            float ratio = MAX_ROTATION / jb_abs(rotation);
            w *= ratio;
        }
        c->x += h * v->x;
        c->y += h * v->y;
        a += h * w;
        isl->positions[i].a = a;
        isl->velocities[i].w = w;
    }

    int positionSolved = 0;
    for (int i = 0; i < step->positionIterations; ++i) {
        int contactsOkay = cs_solvePositionConstraints(&isl->contactSolver);
        if (contactsOkay) {
            positionSolved = 1;
            break;
        }
    }

    for (int i = 0; i < isl->bodyCount; ++i) {
        Body *body = isl->bodies[i];
        body->sweep.c = isl->positions[i].c;
        body->sweep.a = isl->positions[i].a;
        body->linearVelocity = isl->velocities[i].v;
        body->angularVelocity = isl->velocities[i].w;
        body_synchronizeTransform(body);
    }

    if (allowSleep) {
        float minSleepTime = FLT_MAX;
        float linTolSqr = LINEAR_SLEEP_TOL * LINEAR_SLEEP_TOL;
        float angTolSqr = ANGULAR_SLEEP_TOL * ANGULAR_SLEEP_TOL;
        for (int i = 0; i < isl->bodyCount; ++i) {
            Body *b = isl->bodies[i];
            if (b->type == BODY_STATIC)
                continue;
            if ((b->flags & BF_AUTOSLEEP) == 0 || b->angularVelocity * b->angularVelocity > angTolSqr ||
                v2dot(b->linearVelocity, b->linearVelocity) > linTolSqr) {
                b->sleepTime = 0.0f;
                minSleepTime = 0.0f;
            } else {
                b->sleepTime += h;
                minSleepTime = jb_min(minSleepTime, b->sleepTime);
            }
        }
        if (minSleepTime >= TIME_TO_SLEEP && positionSolved) {
            for (int i = 0; i < isl->bodyCount; ++i)
                body_setAwake(isl->bodies[i], 0);
        }
    }
}

static void island_solveTOI(Island *isl, TimeStep *subStep, int toiIndexA, int toiIndexB)
{
    for (int i = 0; i < isl->bodyCount; ++i) {
        isl->positions[i].c = isl->bodies[i]->sweep.c;
        isl->positions[i].a = isl->bodies[i]->sweep.a;
        isl->velocities[i].v = isl->bodies[i]->linearVelocity;
        isl->velocities[i].w = isl->bodies[i]->angularVelocity;
    }
    cs_init(&isl->toiContactSolver, subStep, isl->contacts, isl->contactCount, isl->positions,
            isl->velocities);
    for (int i = 0; i < subStep->positionIterations; ++i) {
        int contactsOkay = cs_solveTOIPositionConstraints(&isl->toiContactSolver, toiIndexA, toiIndexB);
        if (contactsOkay)
            break;
    }
    isl->bodies[toiIndexA]->sweep.c0 = isl->positions[toiIndexA].c;
    isl->bodies[toiIndexA]->sweep.a0 = isl->positions[toiIndexA].a;
    isl->bodies[toiIndexB]->sweep.c0 = isl->positions[toiIndexB].c;
    isl->bodies[toiIndexB]->sweep.a0 = isl->positions[toiIndexB].a;

    cs_initializeVelocityConstraints(&isl->toiContactSolver);
    for (int i = 0; i < subStep->velocityIterations; ++i)
        cs_solveVelocityConstraints(&isl->toiContactSolver);

    float h = subStep->dt;
    for (int i = 0; i < isl->bodyCount; ++i) {
        Vec2 *c = &isl->positions[i].c;
        float a = isl->positions[i].a;
        Vec2 *v = &isl->velocities[i].v;
        float w = isl->velocities[i].w;
        float translationx = v->x * h;
        float translationy = v->y * h;
        if (translationx * translationx + translationy * translationy > MAX_TRANSLATION_SQ) {
            float ratio = MAX_TRANSLATION / sqrtf(translationx * translationx + translationy * translationy);
            v->x *= ratio;
            v->y *= ratio;
        }
        float rotation = h * w;
        if (rotation * rotation > MAX_ROTATION_SQ) {
            float ratio = MAX_ROTATION / jb_abs(rotation);
            w *= ratio;
        }
        c->x += v->x * h;
        c->y += v->y * h;
        a += h * w;
        isl->positions[i].c = *c;
        isl->positions[i].a = a;
        isl->velocities[i].v = *v;
        isl->velocities[i].w = w;
        Body *body = isl->bodies[i];
        body->sweep.c = *c;
        body->sweep.a = a;
        body->linearVelocity = *v;
        body->angularVelocity = w;
        body_synchronizeTransform(body);
    }
}

/* ---------------- World ---------------- */
struct World {
    Vec2 gravity;
    Body *bodyList;
    int bodyCount;
    ContactManager cm;
    int flags;
    int warmStarting, continuousPhysics, subStepping, stepComplete, allowSleep;
    float invDt0;
    TimeStep step;
    TimeStep subStep;
    Island island;
    Island toiIsland;
    Body **stack;
    int stackCap;
};

static World *world_create(Vec2 gravity)
{
    World *w = (World *)calloc(1, sizeof(World));
    w->gravity = gravity;
    w->bodyList = NULL;
    w->bodyCount = 0;
    w->warmStarting = 1;
    w->continuousPhysics = 1;
    w->subStepping = 0;
    w->stepComplete = 1;
    w->allowSleep = 1;
    w->flags = WF_CLEAR_FORCES;
    w->invDt0 = 0.0f;
    bp_init(&w->cm.bp);
    w->cm.contactList = NULL;
    w->cm.contactCount = 0;
    return w;
}

static Body *world_createBody(World *w, int type, Vec2 position, float angle)
{
    Body *b = (Body *)calloc(1, sizeof(Body));
    b->world = w;
    b->flags = 0;
    b->flags |= BF_AUTOSLEEP;
    b->flags |= BF_AWAKE;
    b->flags |= BF_ACTIVE;
    b->xf.p = position;
    b->xf.q = rot_set(angle);
    b->sweep.localCenter = V2(0.0f, 0.0f);
    b->sweep.c0 = b->xf.p;
    b->sweep.c = b->xf.p;
    b->sweep.a0 = angle;
    b->sweep.a = angle;
    b->sweep.alpha0 = 0.0f;
    b->linearVelocity = V2(0.0f, 0.0f);
    b->angularVelocity = 0.0f;
    b->linearDamping = 0.0f;
    b->angularDamping = 0.0f;
    b->gravityScale = 1.0f;
    b->force = V2(0.0f, 0.0f);
    b->torque = 0.0f;
    b->sleepTime = 0.0f;
    b->type = type;
    if (b->type == BODY_DYNAMIC) {
        b->mass = 1.0f;
        b->invMass = 1.0f;
    } else {
        b->mass = 0.0f;
        b->invMass = 0.0f;
    }
    b->I = 0.0f;
    b->invI = 0.0f;
    b->fixtureList = NULL;
    b->fixtureCount = 0;
    b->contactList = NULL;
    /* prepend to the world body list */
    b->prev = NULL;
    b->next = w->bodyList;
    if (w->bodyList != NULL)
        w->bodyList->prev = b;
    w->bodyList = b;
    ++w->bodyCount;
    return b;
}

static Fixture *body_createFixture(Body *b, const Shape *shape, float density, float friction, int isSensor)
{
    World *w = b->world;
    Fixture *f = (Fixture *)calloc(1, sizeof(Fixture));
    Filter filter;
    filter.categoryBits = 0x0001;
    filter.maskBits = 0xFFFF;
    filter.groupIndex = 0;
    fixture_create(f, b, shape, density, friction, filter, isSensor);
    if (body_isActive(b))
        fixture_createProxies(f, &w->cm.bp, &b->xf);
    f->next = b->fixtureList;
    b->fixtureList = f;
    ++b->fixtureCount;
    f->body = b;
    if (f->density > 0.0f)
        body_resetMassData(b);
    w->flags |= WF_NEW_FIXTURE;
    return f;
}

static void world_findNewContacts(World *w) { bp_updatePairs(&w->cm.bp, &w->cm); }

static void world_clearForces(World *w)
{
    for (Body *body = w->bodyList; body != NULL; body = body->next) {
        body->force = V2(0.0f, 0.0f);
        body->torque = 0.0f;
    }
}

/* World.solve: island DFS, island solve, then fixture sync and new contacts. */
static void world_solve(World *w, TimeStep *step)
{
    island_init(&w->island, w->bodyCount, w->cm.contactCount);
    for (Body *b = w->bodyList; b != NULL; b = b->next)
        b->flags &= ~BF_ISLAND;
    for (Contact *c = w->cm.contactList; c != NULL; c = c->next)
        c->flags &= ~CF_ISLAND;
    int stackSize = w->bodyCount;
    if (w->stackCap < stackSize) {
        w->stack = (Body **)realloc(w->stack, sizeof(Body *) * (size_t)stackSize);
        w->stackCap = stackSize;
    }
    for (Body *seed = w->bodyList; seed != NULL; seed = seed->next) {
        if ((seed->flags & BF_ISLAND) == BF_ISLAND)
            continue;
        if (body_isAwake(seed) == 0 || body_isActive(seed) == 0)
            continue;
        if (seed->type == BODY_STATIC)
            continue;
        island_clear(&w->island);
        int stackCount = 0;
        w->stack[stackCount++] = seed;
        seed->flags |= BF_ISLAND;
        while (stackCount > 0) {
            Body *b = w->stack[--stackCount];
            island_addBody(&w->island, b);
            body_setAwake(b, 1);
            if (b->type == BODY_STATIC)
                continue;
            for (ContactEdge *ce = b->contactList; ce != NULL; ce = ce->next) {
                Contact *contact = ce->contact;
                if ((contact->flags & CF_ISLAND) == CF_ISLAND)
                    continue;
                if ((contact->flags & CF_ENABLED) == 0 || (contact->flags & CF_TOUCHING) == 0)
                    continue;
                int sensorA = contact->fixtureA->isSensor;
                int sensorB = contact->fixtureB->isSensor;
                if (sensorA || sensorB)
                    continue;
                island_addContact(&w->island, contact);
                contact->flags |= CF_ISLAND;
                Body *other = ce->other;
                if ((other->flags & BF_ISLAND) == BF_ISLAND)
                    continue;
                w->stack[stackCount++] = other;
                other->flags |= BF_ISLAND;
            }
        }
        island_solve(&w->island, step, w->gravity, w->allowSleep);
        for (int i = 0; i < w->island.bodyCount; ++i) {
            Body *b = w->island.bodies[i];
            if (b->type == BODY_STATIC)
                b->flags &= ~BF_ISLAND;
        }
    }
    for (Body *b = w->bodyList; b != NULL; b = b->next) {
        if ((b->flags & BF_ISLAND) == 0)
            continue;
        if (b->type == BODY_STATIC)
            continue;
        body_synchronizeFixtures(b, &w->cm.bp);
    }
    world_findNewContacts(w);
}

/* World.solveTOI: continuous collision against the first time of impact per step. */
static void world_solveTOI(World *w, TimeStep *step)
{
    Island *island = &w->toiIsland;
    island_init(island, 2 * MAX_TOI_CONTACTS, MAX_TOI_CONTACTS);
    if (w->stepComplete) {
        for (Body *b = w->bodyList; b != NULL; b = b->next) {
            b->flags &= ~BF_ISLAND;
            b->sweep.alpha0 = 0.0f;
        }
        for (Contact *c = w->cm.contactList; c != NULL; c = c->next) {
            c->flags &= ~(CF_TOI | CF_ISLAND);
            c->toiCount = 0;
            c->toi = 1.0f;
        }
    }
    for (;;) {
        Contact *minContact = NULL;
        float minAlpha = 1.0f;
        for (Contact *c = w->cm.contactList; c != NULL; c = c->next) {
            if ((c->flags & CF_ENABLED) == 0)
                continue;
            if (c->toiCount > MAX_SUB_STEPS)
                continue;
            float alpha = 1.0f;
            if ((c->flags & CF_TOI) != 0) {
                alpha = c->toi;
            } else {
                Fixture *fA = c->fixtureA;
                Fixture *fB = c->fixtureB;
                if (fA->isSensor || fB->isSensor)
                    continue;
                Body *bA = fA->body;
                Body *bB = fB->body;
                int typeA = bA->type;
                int typeB = bB->type;
                int activeA = body_isAwake(bA) && typeA != BODY_STATIC;
                int activeB = body_isAwake(bB) && typeB != BODY_STATIC;
                if (activeA == 0 && activeB == 0)
                    continue;
                int collideA = body_isBullet(bA) || typeA != BODY_DYNAMIC;
                int collideB = body_isBullet(bB) || typeB != BODY_DYNAMIC;
                if (collideA == 0 && collideB == 0)
                    continue;
                float alpha0 = bA->sweep.alpha0;
                if (bA->sweep.alpha0 < bB->sweep.alpha0) {
                    alpha0 = bB->sweep.alpha0;
                    sweep_advance(&bA->sweep, alpha0);
                } else if (bB->sweep.alpha0 < bA->sweep.alpha0) {
                    alpha0 = bA->sweep.alpha0;
                    sweep_advance(&bB->sweep, alpha0);
                }
                int indexA = c->indexA;
                int indexB = c->indexB;
                TOIInput input;
                proxy_set(&input.proxyA, &fA->shape);
                proxy_set(&input.proxyB, &fB->shape);
                (void)indexA;
                (void)indexB;
                input.sweepA = bA->sweep;
                input.sweepB = bB->sweep;
                input.tMax = 1.0f;
                TOIOutput toiOutput;
                toi_timeOfImpact(&toiOutput, &input);
                float beta = toiOutput.t;
                if (toiOutput.state == TOI_TOUCHING)
                    alpha = jb_min(alpha0 + (1.0f - alpha0) * beta, 1.0f);
                else
                    alpha = 1.0f;
                c->toi = alpha;
                c->flags |= CF_TOI;
            }
            if (alpha < minAlpha) {
                minContact = c;
                minAlpha = alpha;
            }
        }
        if (minContact == NULL || 1.0f - 10.0f * JB_EPSILON < minAlpha) {
            w->stepComplete = 1;
            break;
        }
        Fixture *fA = minContact->fixtureA;
        Fixture *fB = minContact->fixtureB;
        Body *bA = fA->body;
        Body *bB = fB->body;
        Sweep backup1 = bA->sweep;
        Sweep backup2 = bB->sweep;
        body_advance(bA, minAlpha);
        body_advance(bB, minAlpha);
        contact_update(minContact);
        minContact->flags &= ~CF_TOI;
        ++minContact->toiCount;
        if ((minContact->flags & CF_ENABLED) == 0 || (minContact->flags & CF_TOUCHING) == 0) {
            minContact->flags &= ~CF_ENABLED;
            bA->sweep = backup1;
            bB->sweep = backup2;
            body_synchronizeTransform(bA);
            body_synchronizeTransform(bB);
            continue;
        }
        body_setAwake(bA, 1);
        body_setAwake(bB, 1);
        island_clear(island);
        island_addBody(island, bA);
        island_addBody(island, bB);
        island_addContact(island, minContact);
        bA->flags |= BF_ISLAND;
        bB->flags |= BF_ISLAND;
        minContact->flags |= CF_ISLAND;
        Body *tempBodies[2] = {bA, bB};
        for (int i = 0; i < 2; ++i) {
            Body *body = tempBodies[i];
            if (body->type == BODY_DYNAMIC) {
                for (ContactEdge *ce = body->contactList; ce != NULL; ce = ce->next) {
                    if (island->bodyCount == island->bodyCapacity)
                        break;
                    if (island->contactCount == island->contactCapacity)
                        break;
                    Contact *contact = ce->contact;
                    if ((contact->flags & CF_ISLAND) != 0)
                        continue;
                    Body *other = ce->other;
                    if (other->type == BODY_DYNAMIC && body_isBullet(body) == 0 && body_isBullet(other) == 0)
                        continue;
                    int sensorA = contact->fixtureA->isSensor;
                    int sensorB = contact->fixtureB->isSensor;
                    if (sensorA || sensorB)
                        continue;
                    Sweep otherBackup = other->sweep;
                    if ((other->flags & BF_ISLAND) == 0)
                        body_advance(other, minAlpha);
                    contact_update(contact);
                    if ((contact->flags & CF_ENABLED) == 0) {
                        other->sweep = otherBackup;
                        body_synchronizeTransform(other);
                        continue;
                    }
                    if ((contact->flags & CF_TOUCHING) == 0) {
                        other->sweep = otherBackup;
                        body_synchronizeTransform(other);
                        continue;
                    }
                    contact->flags |= CF_ISLAND;
                    island_addContact(island, contact);
                    if ((other->flags & BF_ISLAND) != 0)
                        continue;
                    other->flags |= BF_ISLAND;
                    if (other->type != BODY_STATIC)
                        body_setAwake(other, 1);
                    island_addBody(island, other);
                }
            }
        }
        TimeStep *subStep = &w->subStep;
        subStep->dt = (1.0f - minAlpha) * step->dt;
        subStep->inv_dt = 1.0f / subStep->dt;
        subStep->dtRatio = 1.0f;
        subStep->positionIterations = 20;
        subStep->velocityIterations = step->velocityIterations;
        subStep->warmStarting = 0;
        island_solveTOI(island, subStep, bA->islandIndex, bB->islandIndex);
        for (int i = 0; i < island->bodyCount; ++i) {
            Body *body = island->bodies[i];
            body->flags &= ~BF_ISLAND;
            if (body->type != BODY_DYNAMIC)
                continue;
            body_synchronizeFixtures(body, &w->cm.bp);
            for (ContactEdge *ce = body->contactList; ce != NULL; ce = ce->next)
                ce->contact->flags &= ~(CF_TOI | CF_ISLAND);
        }
        world_findNewContacts(w);
        if (w->subStepping) {
            w->stepComplete = 0;
            break;
        }
    }
}

/* World.step */
static void world_step(World *w, float dt, int velocityIterations, int positionIterations)
{
    if ((w->flags & WF_NEW_FIXTURE) == WF_NEW_FIXTURE) {
        world_findNewContacts(w);
        w->flags &= ~WF_NEW_FIXTURE;
    }
    TimeStep *step = &w->step;
    step->dt = dt;
    step->velocityIterations = velocityIterations;
    step->positionIterations = positionIterations;
    if (dt > 0.0f)
        step->inv_dt = 1.0f / dt;
    else
        step->inv_dt = 0.0f;
    step->dtRatio = w->invDt0 * dt;
    step->warmStarting = w->warmStarting;
    cm_collide(&w->cm);
    if (w->stepComplete && step->dt > 0.0f)
        world_solve(w, step);
    if (w->continuousPhysics && step->dt > 0.0f)
        world_solveTOI(w, step);
    if (step->dt > 0.0f)
        w->invDt0 = step->inv_dt;
    if ((w->flags & WF_CLEAR_FORCES) == WF_CLEAR_FORCES)
        world_clearForces(w);
}

static void world_destroy(World *w)
{
    Body *b = w->bodyList;
    while (b != NULL) {
        Body *next = b->next;
        Fixture *f = b->fixtureList;
        while (f != NULL) {
            Fixture *fn = f->next;
            free(f);
            f = fn;
        }
        free(b);
        b = next;
    }
    Contact *c = w->cm.contactList;
    while (c != NULL) {
        Contact *next = c->next;
        free(c);
        c = next;
    }
    bp_destroy(&w->cm.bp);
    free(w->island.bodies);
    free(w->island.contacts);
    free(w->island.positions);
    free(w->island.velocities);
    free(w->island.contactSolver.pcs);
    free(w->island.contactSolver.vcs);
    free(w->island.toiContactSolver.pcs);
    free(w->island.toiContactSolver.vcs);
    free(w->toiIsland.bodies);
    free(w->toiIsland.contacts);
    free(w->toiIsland.positions);
    free(w->toiIsland.velocities);
    free(w->toiIsland.contactSolver.pcs);
    free(w->toiIsland.contactSolver.vcs);
    free(w->toiIsland.toiContactSolver.pcs);
    free(w->toiIsland.toiContactSolver.vcs);
    free(w->stack);
    free(w);
}

/* ---------------- benchmark scene (Box2dBenchmark.makeNewWorld / run) ---------------- */
static World *make_new_world(void)
{
    Vec2 gravity = V2(0.0f, -10.0f);
    World *world = world_create(gravity);
    Shape edgeShape;
    shape_edge_init(&edgeShape, V2(-40.0f, 0.0f), V2(40.0f, 0.0f));
    Body *ground = world_createBody(world, BODY_STATIC, V2(0.0f, 0.0f), 0.0f);
    body_createFixture(ground, &edgeShape, 0.0f, 0.2f, 0);

    float a = .5f;
    Shape shape;
    shape_polygon_init(&shape);
    shape_setAsBox(&shape, a, a);

    Vec2 x = V2(-7.0f, 0.75f);
    Vec2 y = V2(0.0f, 0.0f);
    Vec2 deltaX = V2(0.5625f, 1.0f);
    Vec2 deltaY = V2(1.125f, 0.0f);
    for (int i = 0; i < 10; ++i) {
        y = V2(x.x, x.y);
        for (int j = 0; j < 5; ++j) {
            Body *body = world_createBody(world, BODY_DYNAMIC, y, 0.0f);
            body_createFixture(body, &shape, 5.0f, 0.2f, 0);
            y = v2add(y, deltaY);
        }
        x = v2add(x, deltaX);
    }
    return world;
}

static World *benchmark_run(void)
{
    World *world = make_new_world();
    for (int i = 0; i < NUMBER_OF_STEPS; i++)
        world_step(world, 1.0f / 60.0f, 10, 3);
    return world;
}

/* JetStream runIteration: runFixedCount(5). Returns the world of the final run. */
static World *run_fixed_count(int count, World *previous)
{
    World *last = previous;
    for (int i = 0; i < count; i++) {
        World *w = benchmark_run();
        if (last != NULL)
            world_destroy(last);
        last = w;
    }
    return last;
}

typedef struct { double sum; double minY; double maxAbsX; int dynamicCount; int nanCount; } Checksum;

static Checksum world_checksum(const World *w)
{
    Checksum cs;
    cs.sum = 0.0;
    cs.minY = DBL_MAX;
    cs.maxAbsX = 0.0;
    cs.dynamicCount = 0;
    cs.nanCount = 0;
    for (const Body *b = w->bodyList; b != NULL; b = b->next) {
        if (b->type != BODY_DYNAMIC)
            continue;
        double x = b->xf.p.x, y = b->xf.p.y, a = b->sweep.a;
        if (isnan(x) || isnan(y) || isnan(a))
            cs.nanCount++;
        cs.sum += x + y;
        cs.sum += a;
        if (y < cs.minY)
            cs.minY = y;
        if (fabs(x) > cs.maxAbsX)
            cs.maxAbsX = fabs(x);
        cs.dynamicCount++;
    }
    return cs;
}

static double now_ms(void)
{
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (double)ts.tv_sec * 1e3 + (double)ts.tv_nsec / 1e6;
}

int main(int argc, char **argv)
{
    int iterations = DEFAULT_ITERATIONS;
    if (argc > 1) {
        char *end = NULL;
        long v = strtol(argv[1], &end, 10);
        if (end == argv[1] || v < 1) {
            fprintf(stderr, "usage: %s [iterations] [jetstream-root]\n", argv[0]);
            return 1;
        }
        iterations = (int)v;
    }
    /* argv[2] is the JetStream root; this analogue reads no files, so it is accepted and unused. */
    (void)argv;

    lut_init();

    World *last = NULL;
    double t0 = now_ms();
    for (int it = 0; it < iterations; it++)
        last = run_fixed_count(INTERNAL_ITERATIONS, last);
    double t1 = now_ms();
    printf("j2cl-box2d-wasm: %.3f ms for %d iterations\n", t1 - t0, iterations);

    Checksum cs = world_checksum(last);
    double diff = fabs(cs.sum - CHECKSUM_EXPECTED);
    int ok = cs.nanCount == 0 && cs.dynamicCount == 50 && cs.minY >= MIN_ALLOWED_Y &&
             cs.maxAbsX < 40.0 && diff <= CHECKSUM_TOLERANCE;
    if (ok) {
        printf("validation: ok (checksum %.6f, expected %.6f from jbox2d, |diff| %.2e, %d dynamic bodies, "
               "lowest y %.6f, no NaN)\n",
               cs.sum, CHECKSUM_EXPECTED, diff, cs.dynamicCount, cs.minY);
    } else {
        printf("validation: FAILED (checksum %.6f, expected %.6f, |diff| %.2e, %d dynamic bodies, "
               "lowest y %.6f, max |x| %.6f, NaN bodies %d)\n",
               cs.sum, CHECKSUM_EXPECTED, diff, cs.dynamicCount, cs.minY, cs.maxAbsX, cs.nanCount);
    }
    world_destroy(last);
    free(lut_sin);
    return ok ? 0 : 1;
}
