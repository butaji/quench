// jetstream-row: threejs
// jetstream-files: threejs/three.js threejs/benchmark.js
// iterations: 120
// jetstream-driver: deterministic=true
/* C analogue of JetStream's threejs row (three.js r178 benchmark.js): 8000 particles, each a
 * Group holding a CircleGeometry mesh and a LineLoop outline, animated in place and rendered
 * once per iteration. The C code does the same work per frame as the three.js path the row
 * hits: scene and camera matrix updates (compose, multiply, invert), the projection,
 * frustum culling with bounding spheres, the render list with its painter sort, and the
 * per-object render path (modelView and normal matrices, program and state checks, draw
 * bookkeeping). The fake WebGL context returns no active uniforms or attributes, so the
 * uniform and attribute uploads three.js would make are no-ops there and are not modelled.
 * The random seed is reset before each iteration as the JetStream driver does.
 * Usage: threejs [iterations]
 * Output: timing, then checksums of particle state, of the matrixWorld elements and the
 * number of draw calls, to compare with the JS run.
 */
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#define ITERATIONS 120
#define NUM_PARTICLES 8000
#define CIRCLE_SEGMENTS 32
#define INNER_WIDTH 1920.0
#define INNER_HEIGHT 1080.0
#define CAMERA_FOV 75.0
#define CAMERA_NEAR 0.1
#define CAMERA_FAR 1000.0
#define CAMERA_Z 500.0
#define CAMERA_ASPECT 1.0
#define SEED0 49734321u

/* ---------- deterministic Math.random (JetStreamDriver addDeterministicRandom) ---------- */
static uint32_t rng_seed;

static void rng_reset(void) { rng_seed = SEED0; }

static double rnd(void) {
  uint32_t s = rng_seed;
  s = (s + 0x7ed55d16u) + (s << 12);
  s = (s ^ 0xc761c23cu) ^ (s >> 19);
  s = (s + 0x165667b1u) + (s << 5);
  s = (s + 0xd3a2646cu) ^ (s << 9);
  s = (s + 0xfd7046c5u) + (s << 3);
  s = (s ^ 0xb55a4f09u) ^ (s >> 16);
  rng_seed = s;
  return (double)s / 4294967296.0;
}

static double rand_float_spread(double range) { return range * (0.5 - rnd()); }

/* THREE.MathUtils.generateUUID() draws four Math.random() values; the string itself is never
 * read by this benchmark, so only the draws are reproduced. Object3D, Material and
 * BufferGeometry each call it in their constructor. */
static void uuid_consume(void) {
  for (int k = 0; k < 4; k++) (void)rnd();
}

/* ---------- vectors and matrices (column-major, as three.js Matrix4.elements) ---------- */
typedef struct { double x, y, z; } Vec3;

static void matrix_compose(double *te, const Vec3 *p, const double *q, const Vec3 *s) {
  double x = q[0], y = q[1], z = q[2], w = q[3];
  double x2 = x + x, y2 = y + y, z2 = z + z;
  double xx = x * x2, xy = x * y2, xz = x * z2;
  double yy = y * y2, yz = y * z2, zz = z * z2;
  double wx = w * x2, wy = w * y2, wz = w * z2;
  double sx = s->x, sy = s->y, sz = s->z;
  te[0] = (1 - (yy + zz)) * sx;
  te[1] = (xy + wz) * sx;
  te[2] = (xz - wy) * sx;
  te[3] = 0;
  te[4] = (xy - wz) * sy;
  te[5] = (1 - (xx + zz)) * sy;
  te[6] = (yz + wx) * sy;
  te[7] = 0;
  te[8] = (xz + wy) * sz;
  te[9] = (yz - wx) * sz;
  te[10] = (1 - (xx + yy)) * sz;
  te[11] = 0;
  te[12] = p->x;
  te[13] = p->y;
  te[14] = p->z;
  te[15] = 1;
}

static void matrix_copy(double *te, const double *me) { memcpy(te, me, 16 * sizeof(double)); }

static void matrix_multiply(double *te, const double *ae, const double *be) {
  double a11 = ae[0], a12 = ae[4], a13 = ae[8], a14 = ae[12];
  double a21 = ae[1], a22 = ae[5], a23 = ae[9], a24 = ae[13];
  double a31 = ae[2], a32 = ae[6], a33 = ae[10], a34 = ae[14];
  double a41 = ae[3], a42 = ae[7], a43 = ae[11], a44 = ae[15];
  double b11 = be[0], b12 = be[4], b13 = be[8], b14 = be[12];
  double b21 = be[1], b22 = be[5], b23 = be[9], b24 = be[13];
  double b31 = be[2], b32 = be[6], b33 = be[10], b34 = be[14];
  double b41 = be[3], b42 = be[7], b43 = be[11], b44 = be[15];
  te[0] = a11 * b11 + a12 * b21 + a13 * b31 + a14 * b41;
  te[4] = a11 * b12 + a12 * b22 + a13 * b32 + a14 * b42;
  te[8] = a11 * b13 + a12 * b23 + a13 * b33 + a14 * b43;
  te[12] = a11 * b14 + a12 * b24 + a13 * b34 + a14 * b44;
  te[1] = a21 * b11 + a22 * b21 + a23 * b31 + a24 * b41;
  te[5] = a21 * b12 + a22 * b22 + a23 * b32 + a24 * b42;
  te[9] = a21 * b13 + a22 * b23 + a23 * b33 + a24 * b43;
  te[13] = a21 * b14 + a22 * b24 + a23 * b34 + a24 * b44;
  te[2] = a31 * b11 + a32 * b21 + a33 * b31 + a34 * b41;
  te[6] = a31 * b12 + a32 * b22 + a33 * b32 + a34 * b42;
  te[10] = a31 * b13 + a32 * b23 + a33 * b33 + a34 * b43;
  te[14] = a31 * b14 + a32 * b24 + a33 * b34 + a34 * b44;
  te[3] = a41 * b11 + a42 * b21 + a43 * b31 + a44 * b41;
  te[7] = a41 * b12 + a42 * b22 + a43 * b32 + a44 * b42;
  te[11] = a41 * b13 + a42 * b23 + a43 * b33 + a44 * b43;
  te[15] = a41 * b14 + a42 * b24 + a43 * b34 + a44 * b44;
}

static void matrix_invert(double *te) {
  double n11 = te[0], n21 = te[1], n31 = te[2], n41 = te[3];
  double n12 = te[4], n22 = te[5], n32 = te[6], n42 = te[7];
  double n13 = te[8], n23 = te[9], n33 = te[10], n43 = te[11];
  double n14 = te[12], n24 = te[13], n34 = te[14], n44 = te[15];
  double t11 = n23 * n34 * n42 - n24 * n33 * n42 + n24 * n32 * n43 - n22 * n34 * n43 - n23 * n32 * n44 + n22 * n33 * n44;
  double t12 = n14 * n33 * n42 - n13 * n34 * n42 - n14 * n32 * n43 + n12 * n34 * n43 + n13 * n32 * n44 - n12 * n33 * n44;
  double t13 = n13 * n24 * n42 - n14 * n23 * n42 + n14 * n22 * n43 - n12 * n24 * n43 - n13 * n22 * n44 + n12 * n23 * n44;
  double t14 = n14 * n23 * n32 - n13 * n24 * n32 - n14 * n22 * n33 + n12 * n24 * n33 + n13 * n22 * n34 - n12 * n23 * n34;
  double det = n11 * t11 + n21 * t12 + n31 * t13 + n41 * t14;
  if (det == 0) { memset(te, 0, 16 * sizeof(double)); return; }
  double detInv = 1 / det;
  te[0] = t11 * detInv;
  te[1] = (n24 * n33 * n41 - n23 * n34 * n41 - n24 * n31 * n43 + n21 * n34 * n43 + n23 * n31 * n44 - n21 * n33 * n44) * detInv;
  te[2] = (n22 * n34 * n41 - n24 * n32 * n41 + n24 * n31 * n42 - n21 * n34 * n42 - n22 * n31 * n44 + n21 * n32 * n44) * detInv;
  te[3] = (n23 * n32 * n41 - n22 * n33 * n41 - n23 * n31 * n42 + n21 * n33 * n42 + n22 * n31 * n43 - n21 * n32 * n43) * detInv;
  te[4] = t12 * detInv;
  te[5] = (n13 * n34 * n41 - n14 * n33 * n41 + n14 * n31 * n43 - n11 * n34 * n43 - n13 * n31 * n44 + n11 * n33 * n44) * detInv;
  te[6] = (n14 * n32 * n41 - n12 * n34 * n41 - n14 * n31 * n42 + n11 * n34 * n42 + n12 * n31 * n44 - n11 * n32 * n44) * detInv;
  te[7] = (n12 * n33 * n41 - n13 * n32 * n41 + n13 * n31 * n42 - n11 * n33 * n42 - n12 * n31 * n43 + n11 * n32 * n43) * detInv;
  te[8] = t13 * detInv;
  te[9] = (n14 * n23 * n41 - n13 * n24 * n41 - n14 * n21 * n43 + n11 * n24 * n43 + n13 * n21 * n44 - n11 * n23 * n44) * detInv;
  te[10] = (n12 * n24 * n41 - n14 * n22 * n41 + n14 * n21 * n42 - n11 * n24 * n42 - n12 * n21 * n44 + n11 * n22 * n44) * detInv;
  te[11] = (n13 * n22 * n41 - n12 * n23 * n41 - n13 * n21 * n42 + n11 * n23 * n42 + n12 * n21 * n43 - n11 * n22 * n43) * detInv;
  te[12] = t14 * detInv;
  te[13] = (n13 * n24 * n31 - n14 * n23 * n31 + n14 * n21 * n33 - n11 * n24 * n33 - n13 * n21 * n34 + n11 * n23 * n34) * detInv;
  te[14] = (n14 * n22 * n31 - n12 * n24 * n31 - n14 * n21 * n32 + n11 * n24 * n32 + n12 * n21 * n34 - n11 * n22 * n34) * detInv;
  te[15] = (n12 * n23 * n31 - n13 * n22 * n31 + n13 * n21 * n32 - n11 * n23 * n32 - n12 * n21 * n33 + n11 * n22 * n33) * detInv;
}

static double matrix_determinant(const double *te) {
  double n11 = te[0], n12 = te[4], n13 = te[8], n14 = te[12];
  double n21 = te[1], n22 = te[5], n23 = te[9], n24 = te[13];
  double n31 = te[2], n32 = te[6], n33 = te[10], n34 = te[14];
  double n41 = te[3], n42 = te[7], n43 = te[11], n44 = te[15];
  return n41 * (+n14 * n23 * n32 - n13 * n24 * n32 - n14 * n22 * n33 + n12 * n24 * n33 + n13 * n22 * n34 - n12 * n23 * n34) +
         n42 * (+n11 * n23 * n34 - n11 * n24 * n33 + n14 * n21 * n33 - n13 * n21 * n34 + n13 * n24 * n31 - n14 * n23 * n31) +
         n43 * (+n11 * n24 * n32 - n11 * n22 * n34 - n14 * n21 * n32 + n12 * n21 * n34 + n14 * n22 * n31 - n12 * n24 * n31) +
         n44 * (-n13 * n22 * n31 - n11 * n23 * n32 + n11 * n22 * n33 + n13 * n21 * n32 - n12 * n21 * n33 + n12 * n23 * n31);
}

/* Matrix3.getNormalMatrix(m4): setFromMatrix4, invert, transpose. Stored row-major as in Matrix3. */
static void normal_matrix(double *out, const double *me) {
  double te[9] = {me[0], me[4], me[8], me[1], me[5], me[9], me[2], me[6], me[10]};
  double n11 = te[0], n21 = te[1], n31 = te[2], n12 = te[3], n22 = te[4], n32 = te[5], n13 = te[6], n23 = te[7], n33 = te[8];
  double t11 = n33 * n22 - n32 * n23, t12 = n32 * n13 - n33 * n12, t13 = n23 * n12 - n22 * n13;
  double det = n11 * t11 + n21 * t12 + n31 * t13;
  double inv[9];
  if (det == 0) { memset(inv, 0, sizeof inv); }
  else {
    double detInv = 1 / det;
    inv[0] = t11 * detInv;
    inv[1] = (n31 * n23 - n33 * n21) * detInv;
    inv[2] = (n32 * n21 - n31 * n22) * detInv;
    inv[3] = t12 * detInv;
    inv[4] = (n33 * n11 - n31 * n13) * detInv;
    inv[5] = (n31 * n12 - n32 * n11) * detInv;
    inv[6] = t13 * detInv;
    inv[7] = (n21 * n13 - n23 * n11) * detInv;
    inv[8] = (n22 * n11 - n21 * n12) * detInv;
  }
  for (int r = 0; r < 3; r++)
    for (int c = 0; c < 3; c++) out[r * 3 + c] = inv[c * 3 + r];   /* transpose */
}

static double max_scale_on_axis(const double *te) {
  double sx = te[0] * te[0] + te[1] * te[1] + te[2] * te[2];
  double sy = te[4] * te[4] + te[5] * te[5] + te[6] * te[6];
  double sz = te[8] * te[8] + te[9] * te[9] + te[10] * te[10];
  double m = sx > sy ? sx : sy;
  m = m > sz ? m : sz;
  return sqrt(m);
}

static void vec3_apply_matrix4(Vec3 *v, const double *e) {
  double x = v->x, y = v->y, z = v->z;
  double w = 1 / (e[3] * x + e[7] * y + e[11] * z + e[15]);
  v->x = (e[0] * x + e[4] * y + e[8] * z + e[12]) * w;
  v->y = (e[1] * x + e[5] * y + e[9] * z + e[13]) * w;
  v->z = (e[2] * x + e[6] * y + e[10] * z + e[14]) * w;
}

/* Vector4.applyMatrix4 on (x, y, z, w): no divide by w. */
static void vec4_apply_matrix4(double v[4], const double *e) {
  double x = v[0], y = v[1], z = v[2], w = v[3];
  v[0] = e[0] * x + e[4] * y + e[8] * z + e[12] * w;
  v[1] = e[1] * x + e[5] * y + e[9] * z + e[13] * w;
  v[2] = e[2] * x + e[6] * y + e[10] * z + e[14] * w;
  v[3] = e[3] * x + e[7] * y + e[11] * z + e[15] * w;
}

/* ---------- geometry, materials, objects ---------- */
typedef struct {
  float *position;          /* Float32BufferAttribute, 3 per vertex */
  int position_count;
  uint16_t *index;          /* NULL when non-indexed */
  int index_count;
  float *normal, *uv;
  int has_bounding_sphere;  /* computed lazily on first frustum test, as three.js does */
  Vec3 bs_center;
  double bs_radius;
} Geometry;

typedef struct {
  int id;
  int version;
  int is_line;              /* LineBasicMaterial when set, else MeshBasicMaterial */
  double color[3];
  /* renderer-side properties (materialProperties) */
  int props_version;
  int props_output_color_space, props_vertex_alphas, props_vertex_tangents, props_tone_mapping;
  int props_morph_targets_count;
  int props_receive_shadow;
  void *props_program;
} Material;

enum { KIND_GROUP, KIND_MESH, KIND_LINE };

typedef struct Object3D Object3D;
struct Object3D {
  int kind;
  int id;
  int render_order;
  int visible, frustum_culled, receive_shadow;
  Vec3 position;
  double quat[4];           /* x, y, z, w */
  Vec3 scale;
  double matrix[16], matrix_world[16];
  double model_view[16], normal_m[9];
  int matrix_world_needs_update;
  Object3D *parent;
  Object3D **children;
  int nchildren, cap;
  Geometry *geometry;
  Material *material;
  double vel[2];            /* Group.velocity (Vector2) */
};

typedef struct {
  Object3D *object;
  Geometry *geometry;
  Material *material;
  int group_order;
  double z;
} RenderItem;

typedef struct {
  double plane[6][4];       /* normal.xyz, constant */
} Frustum;

typedef struct {
  double projection[16];
  double matrix_world_inverse[16];
  double matrix_world[16];
  double matrix[16];
  Vec3 position;
  double quat[4];
  Vec3 scale;
  int id;
} Camera;

/* ---------- renderer state ---------- */
static Object3D *scene;
static Camera camera;
static Object3D *particles_group[NUM_PARTICLES];
static Frustum frustum;
static RenderItem *render_tmp;
static double proj_screen[16];
static int next_object_id = 1, next_material_id = 1;

static void *program_mesh, *program_line;     /* two shader programs, compiled once */
static void *current_program;
static int current_material_id = -1;
static Camera *current_camera;
static Geometry *bound_geometry;
static int state_line_width = 1;
static int info_calls, info_triangles, info_lines, info_frame;

/* The JS fake WebGL context: draw calls are empty functions. */
static volatile int gl_draws;
static void gl_draw_elements(int mode, int count) { (void)mode; (void)count; gl_draws++; }
static void gl_draw_arrays(int mode, int count) { (void)mode; (void)count; gl_draws++; }

static Object3D *object_new(int kind) {
  Object3D *o = calloc(1, sizeof *o);
  o->kind = kind;
  o->id = next_object_id++;
  o->visible = 1;
  o->frustum_culled = 1;
  o->quat[3] = 1;
  o->scale = (Vec3){1, 1, 1};
  o->matrix_world_needs_update = 0;
  return o;
}

static void object_add(Object3D *parent, Object3D *child) {
  if (parent->nchildren == parent->cap) {
    parent->cap = parent->cap ? parent->cap * 2 : 4;
    parent->children = realloc(parent->children, parent->cap * sizeof *parent->children);
  }
  parent->children[parent->nchildren++] = child;
  child->parent = parent;
}

static Geometry *geometry_new(void) { return calloc(1, sizeof(Geometry)); }

static Material *material_new(int is_line, double color) {
  Material *m = calloc(1, sizeof *m);
  m->id = next_material_id++;
  m->is_line = is_line;
  m->color[0] = m->color[1] = m->color[2] = color;
  m->props_version = -1;
  return m;
}

/* new THREE.CircleGeometry(radius, 32) as in three.js, then the outline points (vertices 1..33). */
static void circle_geometry(Geometry *g, Geometry *outline, double radius) {
  int segments = CIRCLE_SEGMENTS;
  int nv = segments + 2;
  double *verts = malloc((size_t)nv * 3 * sizeof(double));
  double *norms = malloc((size_t)nv * 3 * sizeof(double));
  double *uvs = malloc((size_t)nv * 2 * sizeof(double));
  verts[0] = verts[1] = verts[2] = 0;
  norms[0] = norms[1] = 0; norms[2] = 1;
  uvs[0] = uvs[1] = 0.5;
  for (int s = 0, i = 3; s <= segments; s++, i += 3) {
    double segment = 0 + (double)s / segments * (M_PI * 2);
    verts[i] = radius * cos(segment);
    verts[i + 1] = radius * sin(segment);
    verts[i + 2] = 0;
    norms[i] = 0; norms[i + 1] = 0; norms[i + 2] = 1;
    uvs[(i / 3) * 2] = (verts[i] / radius + 1) / 2;
    uvs[(i / 3) * 2 + 1] = (verts[i + 1] / radius + 1) / 2;
  }
  uint16_t *idx = malloc((size_t)segments * 3 * sizeof(uint16_t));
  for (int i = 1, k = 0; i <= segments; i++, k += 3) {
    idx[k] = (uint16_t)i; idx[k + 1] = (uint16_t)(i + 1); idx[k + 2] = 0;
  }
  g->position = malloc((size_t)nv * 3 * sizeof(float));
  g->normal = malloc((size_t)nv * 3 * sizeof(float));
  g->uv = malloc((size_t)nv * 2 * sizeof(float));
  for (int i = 0; i < nv * 3; i++) { g->position[i] = (float)verts[i]; g->normal[i] = (float)norms[i]; }
  for (int i = 0; i < nv * 2; i++) g->uv[i] = (float)uvs[i];
  g->position_count = nv;
  g->index = idx;
  g->index_count = segments * 3;
  /* outline: positions of vertices 1..segments+1 (the JS loop from i = 3), as a new geometry */
  outline->position_count = segments + 1;
  outline->position = malloc((size_t)(segments + 1) * 3 * sizeof(float));
  memcpy(outline->position, g->position + 3, (size_t)(segments + 1) * 3 * sizeof(float));
  free(verts); free(norms); free(uvs);
}

/* BufferGeometry.computeBoundingSphere() */
static void geometry_compute_bounding_sphere(Geometry *g) {
  double mn[3] = {INFINITY, INFINITY, INFINITY}, mx[3] = {-INFINITY, -INFINITY, -INFINITY};
  for (int i = 0; i < g->position_count; i++)
    for (int k = 0; k < 3; k++) {
      double v = g->position[i * 3 + k];
      if (v < mn[k]) mn[k] = v;
      if (v > mx[k]) mx[k] = v;
    }
  Vec3 c = {(mn[0] + mx[0]) * 0.5, (mn[1] + mx[1]) * 0.5, (mn[2] + mx[2]) * 0.5};
  double maxRadiusSq = 0;
  for (int i = 0; i < g->position_count; i++) {
    double dx = c.x - g->position[i * 3], dy = c.y - g->position[i * 3 + 1], dz = c.z - g->position[i * 3 + 2];
    double d = dx * dx + dy * dy + dz * dz;
    if (d > maxRadiusSq) maxRadiusSq = d;
  }
  g->bs_center = c;
  g->bs_radius = sqrt(maxRadiusSq);
  g->has_bounding_sphere = 1;
}

/* ---------- scene setup (benchmark.js createGeometryParticle + initialize) ---------- */
static Object3D *create_particle(double size) {
  /* uuids drawn by the 3 BufferGeometry, 2 Material, 2 Object3D (Mesh, LineLoop) and Group
   * constructors, in the order createGeometryParticle creates them */
  for (int k = 0; k < 7; k++) uuid_consume();
  double visibleHeight = 2 * tan(CAMERA_FOV * M_PI / 360) * CAMERA_Z;
  double radius = (size / INNER_HEIGHT) * visibleHeight / 2;
  Geometry *circleGeo = geometry_new();
  Geometry *outlineGeo = geometry_new();
  circle_geometry(circleGeo, outlineGeo, radius);
  Material *circleMat = material_new(0, 1.0);          /* MeshBasicMaterial, color 0xffffff */
  Object3D *circle = object_new(KIND_MESH);
  circle->geometry = circleGeo;
  circle->material = circleMat;
  Material *outlineMat = material_new(1, 0.0);         /* LineBasicMaterial, color 0x000000 */
  Object3D *outline = object_new(KIND_LINE);
  outline->geometry = outlineGeo;
  outline->material = outlineMat;
  Object3D *group = object_new(KIND_GROUP);
  object_add(group, circle);
  object_add(group, outline);
  return group;
}

static void initialize(void) {
  double visibleWidth = 2 * tan(CAMERA_FOV * M_PI / 360) * CAMERA_Z * CAMERA_ASPECT;
  for (int i = 0; i < NUM_PARTICLES; i++) {
    double size = 10 + rnd() * 80;
    Object3D *p = create_particle(size);
    p->position = (Vec3){rand_float_spread(visibleWidth), rand_float_spread(visibleWidth / CAMERA_ASPECT), 0};
    p->vel[0] = (rnd() - 0.5) * 8;
    p->vel[1] = (rnd() - 0.5) * 8;
    object_add(scene, p);
    particles_group[i] = p;
  }
}

/* ---------- camera (PerspectiveCamera(75, 1, 0.1, 1000), position.z = 500) ---------- */
static void make_perspective(double *te, double left, double right, double top, double bottom, double near, double far) {
  double x = 2 * near / (right - left);
  double y = 2 * near / (top - bottom);
  double a = (right + left) / (right - left);
  double b = (top + bottom) / (top - bottom);
  double c = -(far + near) / (far - near);
  double d = (-2 * far * near) / (far - near);
  te[0] = x; te[4] = 0; te[8] = a; te[12] = 0;
  te[1] = 0; te[5] = y; te[9] = b; te[13] = 0;
  te[2] = 0; te[6] = 0; te[10] = c; te[14] = d;
  te[3] = 0; te[7] = 0; te[11] = -1; te[15] = 0;
}

static void camera_setup(void) {
  camera.id = next_object_id++;
  camera.position = (Vec3){0, 0, 0};
  camera.position.z = CAMERA_Z;
  camera.quat[3] = 1;
  camera.scale = (Vec3){1, 1, 1};
  /* updateProjectionMatrix(), called by the constructor */
  double near = CAMERA_NEAR;
  double top = near * tan((M_PI / 180) * 0.5 * CAMERA_FOV) / 1.0;
  double height = 2 * top;
  double width = CAMERA_ASPECT * height;
  double left = -0.5 * width;
  make_perspective(camera.projection, left, left + width, top, top - height, near, CAMERA_FAR);
}

static void camera_update_matrix_world(void) {
  matrix_compose(camera.matrix, &camera.position, camera.quat, &camera.scale);
  matrix_copy(camera.matrix_world, camera.matrix);          /* no parent */
  matrix_copy(camera.matrix_world_inverse, camera.matrix_world);
  matrix_invert(camera.matrix_world_inverse);
}

/* ---------- Object3D.updateMatrixWorld (matrixAutoUpdate on every object) ---------- */
static void update_matrix_world(Object3D *o, int force) {
  matrix_compose(o->matrix, &o->position, o->quat, &o->scale);
  o->matrix_world_needs_update = 1;
  if (o->matrix_world_needs_update || force) {
    if (o->parent == NULL) matrix_copy(o->matrix_world, o->matrix);
    else matrix_multiply(o->matrix_world, o->parent->matrix_world, o->matrix);
    o->matrix_world_needs_update = 0;
    force = 1;
  }
  for (int i = 0; i < o->nchildren; i++) update_matrix_world(o->children[i], force);
}

/* ---------- Frustum ---------- */
static void plane_normalize(double *p) {
  double inv = 1.0 / sqrt(p[0] * p[0] + p[1] * p[1] + p[2] * p[2]);
  p[0] *= inv; p[1] *= inv; p[2] *= inv;
  p[3] *= inv;
}

static void frustum_set_from_projection(const double *me) {
  double (*pl)[4] = frustum.plane;
  double me0 = me[0], me1 = me[1], me2 = me[2], me3 = me[3];
  double me4 = me[4], me5 = me[5], me6 = me[6], me7 = me[7];
  double me8 = me[8], me9 = me[9], me10 = me[10], me11 = me[11];
  double me12 = me[12], me13 = me[13], me14 = me[14], me15 = me[15];
  double c[6][4] = {
    {me3 - me0, me7 - me4, me11 - me8, me15 - me12},
    {me3 + me0, me7 + me4, me11 + me8, me15 + me12},
    {me3 + me1, me7 + me5, me11 + me9, me15 + me13},
    {me3 - me1, me7 - me5, me11 - me9, me15 - me13},
    {me3 - me2, me7 - me6, me11 - me10, me15 - me14},
    {me3 + me2, me7 + me6, me11 + me10, me15 + me14},
  };
  for (int i = 0; i < 6; i++) {
    memcpy(pl[i], c[i], sizeof c[i]);
    plane_normalize(pl[i]);
  }
}

static int frustum_intersects_sphere(const Vec3 *center, double radius) {
  double negRadius = -radius;
  for (int i = 0; i < 6; i++) {
    const double *p = frustum.plane[i];
    double distance = p[0] * center->x + p[1] * center->y + p[2] * center->z + p[3];
    if (distance < negRadius) return 0;
  }
  return 1;
}

static int frustum_intersects_object(Object3D *o) {
  Geometry *g = o->geometry;
  if (!g->has_bounding_sphere) geometry_compute_bounding_sphere(g);
  Vec3 c = g->bs_center;
  double r = g->bs_radius;
  vec3_apply_matrix4(&c, o->matrix_world);
  r = r * max_scale_on_axis(o->matrix_world);
  return frustum_intersects_sphere(&c, r);
}

/* ---------- projectObject: render list ---------- */
typedef struct { RenderItem *items; int n; } RenderList;
static RenderList opaque_list;

static void project_object(Object3D *o, int group_order) {
  if (!o->visible) return;
  if (o->kind == KIND_GROUP) {
    group_order = o->render_order;
  } else if (o->kind == KIND_MESH || o->kind == KIND_LINE) {
    if (!o->frustum_culled || frustum_intersects_object(o)) {
      Geometry *g = o->geometry;
      if (!g->has_bounding_sphere) geometry_compute_bounding_sphere(g);
      double v4[4] = {g->bs_center.x, g->bs_center.y, g->bs_center.z, 1};
      vec4_apply_matrix4(v4, o->matrix_world);
      vec4_apply_matrix4(v4, proj_screen);
      Material *m = o->material;
      /* MeshBasicMaterial and LineBasicMaterial are visible and opaque */
      RenderItem *it = &opaque_list.items[opaque_list.n++];
      it->object = o;
      it->geometry = g;
      it->material = m;
      it->group_order = group_order;
      it->z = v4[2];
    }
  }
  for (int i = 0; i < o->nchildren; i++) project_object(o->children[i], group_order);
}

/* painterSortStable: groupOrder, renderOrder, material.id, z, id. */
static int item_cmp(const RenderItem *a, const RenderItem *b) {
  if (a->group_order != b->group_order) return a->group_order - b->group_order;
  if (a->object->render_order != b->object->render_order) return a->object->render_order - b->object->render_order;
  if (a->material->id != b->material->id) return a->material->id - b->material->id;
  if (a->z != b->z) return a->z < b->z ? -1 : 1;
  return a->object->id - b->object->id;
}

/* Natural merge sort (TimSort-like on runs: a presorted list costs n - 1 comparisons). */
static void sort_items(RenderItem *a, int n, RenderItem *tmp) {
  int *starts = malloc(((size_t)n + 1) * sizeof(int));
  int nruns = 0;
  if (n > 0) starts[nruns++] = 0;
  for (int i = 1; i < n; i++)
    if (item_cmp(&a[i - 1], &a[i]) > 0) starts[nruns++] = i;
  starts[nruns] = n;
  while (nruns > 1) {
    int out = 0;
    for (int r = 0; r < nruns; r += 2) {
      int lo = starts[r], mid = (r + 1 < nruns) ? starts[r + 1] : n, hi = (r + 2 < nruns) ? starts[r + 2] : n;
      if (mid == hi) {
        memcpy(tmp + lo, a + lo, (size_t)(hi - lo) * sizeof *a);
      } else {
        int i = lo, j = mid, k = lo;
        while (i < mid && j < hi) {
          if (item_cmp(&a[j], &a[i]) < 0) tmp[k++] = a[j++];
          else tmp[k++] = a[i++];
        }
        while (i < mid) tmp[k++] = a[i++];
        while (j < hi) tmp[k++] = a[j++];
      }
      starts[out++] = lo;
    }
    starts[out] = n;
    memcpy(a, tmp, (size_t)n * sizeof *a);
    nruns = out;
  }
  free(starts);
}

/* ---------- renderer: setProgram, renderBufferDirect, renderObject ---------- */
static void *get_program(Material *m) {
  m->props_output_color_space = 1;
  m->props_vertex_alphas = 0;
  m->props_vertex_tangents = 0;
  m->props_tone_mapping = 0;
  m->props_morph_targets_count = 0;
  m->props_program = m->is_line ? program_line : program_mesh;
  return m->props_program;
}

static void set_program(Object3D *o, Material *m) {
  int needsProgramChange = 0;
  if (m->version == m->props_version) {
    if (m->props_output_color_space != 1) needsProgramChange = 1;
    else if (m->props_vertex_alphas != 0) needsProgramChange = 1;
    else if (m->props_vertex_tangents != 0) needsProgramChange = 1;
    else if (m->props_tone_mapping != 0) needsProgramChange = 1;
    else if (m->props_morph_targets_count != 0) needsProgramChange = 1;
  } else {
    needsProgramChange = 1;
    m->props_version = m->version;
  }
  void *program = m->props_program;
  if (needsProgramChange) program = get_program(m);
  int refreshProgram = 0, refreshMaterial = 0;
  if (current_program != program) {
    current_program = program;
    refreshProgram = refreshMaterial = 1;
  }
  if (m->id != current_material_id) {
    current_material_id = m->id;
    refreshMaterial = 1;
  }
  if (refreshProgram || current_camera != &camera) {
    current_camera = &camera;             /* projection/view uniforms (no active uniforms: no-op) */
    refreshMaterial = 1;
  }
  if (refreshMaterial || m->props_receive_shadow != o->receive_shadow) {
    m->props_receive_shadow = o->receive_shadow;   /* material uniforms (no-op) */
  }
}

static int state_front_cw = -1, state_cull = -1;

static void render_buffer_direct(Object3D *o, Geometry *g, Material *m) {
  int frontFaceCW = (o->kind == KIND_MESH) && matrix_determinant(o->matrix_world) < 0;
  set_program(o, m);
  if (state_front_cw != frontFaceCW) state_front_cw = frontFaceCW;
  if (state_cull != 1) state_cull = 1;
  int drawStart = 0;
  int drawEnd = g->index ? g->index_count : g->position_count;
  int drawCount = drawEnd - drawStart;
  if (bound_geometry != g) bound_geometry = g;      /* bindingStates.setup */
  if (o->kind == KIND_MESH) {
    if (g->index) gl_draw_elements(4 /* TRIANGLES */, drawCount);
    else gl_draw_arrays(4, drawCount);
    info_triangles += drawCount / 3;
  } else {
    if (state_line_width != 1) state_line_width = 1;
    gl_draw_arrays(2 /* LINE_LOOP */, drawCount);
    info_lines += drawCount;
  }
  info_calls++;
}

static void render_object(RenderItem *it) {
  Object3D *o = it->object;
  matrix_multiply(o->model_view, camera.matrix_world_inverse, o->matrix_world);
  normal_matrix(o->normal_m, o->model_view);
  render_buffer_direct(o, it->geometry, it->material);
}

/* ---------- WebGLRenderer.render(scene, camera) ---------- */
static void render_frame(void) {
  update_matrix_world(scene, 0);
  camera_update_matrix_world();
  matrix_multiply(proj_screen, camera.projection, camera.matrix_world_inverse);
  frustum_set_from_projection(proj_screen);
  opaque_list.n = 0;
  project_object(scene, 0);
  sort_items(opaque_list.items, opaque_list.n, render_tmp);
  info_calls = 0;                                    /* info.reset() (autoReset) */
  info_triangles = 0;
  info_lines = 0;
  info_frame++;
  /* background.render(scene): Color background, a clear (no-op on the fake context) */
  for (int i = 0; i < opaque_list.n; i++) render_object(&opaque_list.items[i]);
}

/* ---------- benchmark.js ---------- */
static int initialized;

static void animate(void) {
  double visibleWidth = 2 * tan(CAMERA_FOV * M_PI / 360) * CAMERA_Z * CAMERA_ASPECT;
  double visibleHeight = visibleWidth / CAMERA_ASPECT;
  for (int i = 0; i < NUM_PARTICLES; i++) {
    Object3D *p = particles_group[i];
    p->position.x += p->vel[0];
    p->position.y += p->vel[1];
    if (fabs(p->position.x) > visibleWidth / 2) p->vel[0] *= -1;
    if (fabs(p->position.y) > visibleHeight / 2) p->vel[1] *= -1;
  }
  render_frame();
}

static double matrix_checksum(const Object3D *o) {
  double s = 0;
  for (int i = 0; i < 16; i++) s += o->matrix_world[i];
  for (int i = 0; i < o->nchildren; i++) s += matrix_checksum(o->children[i]);
  return s;
}

int main(int argc, char **argv) {
  int iterations = argc > 1 ? atoi(argv[1]) : ITERATIONS;
  scene = object_new(KIND_GROUP);
  camera_setup();
  opaque_list.items = malloc((size_t)NUM_PARTICLES * 2 * sizeof(RenderItem));
  render_tmp = malloc((size_t)NUM_PARTICLES * 2 * sizeof(RenderItem));
  struct timespec a, b;
  clock_gettime(CLOCK_MONOTONIC, &a);
  for (int i = 0; i < iterations; i++) {
    rng_reset();                                   /* driver: Math.random.__resetSeed() */
    if (!initialized) {
      initialize();
      initialized = 1;
    }
    animate();
  }
  clock_gettime(CLOCK_MONOTONIC, &b);
  double ms = (b.tv_sec - a.tv_sec) * 1e3 + (b.tv_nsec - a.tv_nsec) / 1e6;

  double pstate = 0;
  for (int i = 0; i < NUM_PARTICLES; i++) {
    Object3D *p = particles_group[i];
    pstate += p->position.x;
    pstate += p->position.y;
    pstate += p->vel[0];
    pstate += p->vel[1];
  }
  printf("threejs %d iterations %.3f ms (particles %.17g, matrices %.17g, draws %d)\n", iterations, ms,
         pstate, matrix_checksum(scene), info_calls);
  return 0;
}
