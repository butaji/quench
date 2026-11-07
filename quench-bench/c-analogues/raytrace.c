// C analogue of JetStream 3's raytrace.js (Adam Burmister's ray tracer, ES6 version):
// the same scene, options, arithmetic order and per-render check (checkNumber === 2321),
// with vectors and colors as value structs and shapes/materials as tagged structs.
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <time.h>

typedef struct { double x, y, z; } Vector;
typedef struct { double red, green, blue; } Color;

static Vector vec(double x, double y, double z) { return (Vector){x, y, z}; }
static Vector vadd(Vector v, Vector w) { return vec(w.x + v.x, w.y + v.y, w.z + v.z); }
static Vector vsub(Vector v, Vector w) { return vec(v.x - w.x, v.y - w.y, v.z - w.z); }
static Vector vscale(Vector v, double w) { return vec(v.x * w, v.y * w, v.z * w); }
static double vmagnitude(Vector v) { return sqrt((v.x * v.x) + (v.y * v.y) + (v.z * v.z)); }
static Vector vnormalize(Vector v) { double m = vmagnitude(v); return vec(v.x / m, v.y / m, v.z / m); }
static Vector vcross(Vector v, Vector w) {
  return vec(-v.z * w.y + v.y * w.z, v.z * w.x - v.x * w.z, -v.y * w.x + v.x * w.y);
}
static double vdot(Vector v, Vector w) { return v.x * w.x + v.y * w.y + v.z * w.z; }

static Color color(double r, double g, double b) { return (Color){r, g, b}; }
static Color cadd(Color a, Color b) { return color(a.red + b.red, a.green + b.green, a.blue + b.blue); }
static Color cmul(Color a, Color b) { return color(a.red * b.red, a.green * b.green, a.blue * b.blue); }
static Color cscale(Color a, double f) { return color(a.red * f, a.green * f, a.blue * f); }
static double clamp01(double v) { return v > 0 ? (v > 1 ? 1 : v) : 0; }
static Color climit(Color c) { return color(clamp01(c.red), clamp01(c.green), clamp01(c.blue)); }
static Color caddscalar(Color a, double s) { return climit(color(a.red + s, a.green + s, a.blue + s)); }
static Color cblend(Color a, Color b, double w) { return cadd(cscale(a, 1 - w), cscale(b, w)); }
static int cbrightness(Color c) {
  int r = (int)floor(c.red * 255), g = (int)floor(c.green * 255), b = (int)floor(c.blue * 255);
  return (r * 77 + g * 150 + b * 29) >> 8;
}

typedef enum { SOLID, CHESSBOARD } MaterialKind;
typedef struct {
  MaterialKind kind;
  double reflection, transparency, gloss;
  int hasTexture;
  Color color, colorEven, colorOdd;
  double density;
} Material;

static double wrapUp(double t) {
  t = fmod(t, 2);
  if (t < -1) t += 2;
  if (t >= 1) t -= 2;
  return t;
}
static Color materialColor(const Material *m, double u, double v) {
  if (m->kind == SOLID) return m->color;
  double t = wrapUp(u * m->density) * wrapUp(v * m->density);
  return t < 0 ? m->colorEven : m->colorOdd;
}

typedef enum { SPHERE, PLANE } ShapeKind;
typedef struct { ShapeKind kind; Vector position; Material material; double radius, d; } Shape;
typedef struct { Vector position, direction; } Ray;
typedef struct {
  int isHit, hitCount;
  const Shape *shape;
  Vector position, normal;
  Color color;
  double distance;
} Info;

static Info newInfo(void) {
  Info info = {0};
  info.color = color(0, 0, 0);
  return info;
}

static Info intersect(const Shape *s, Ray ray) {
  Info info = newInfo();
  info.shape = s;
  if (s->kind == SPHERE) {
    Vector dst = vsub(ray.position, s->position);
    double B = vdot(dst, ray.direction);
    double C = vdot(dst, dst) - (s->radius * s->radius);
    double D = (B * B) - C;
    if (D > 0) {
      info.isHit = 1;
      info.distance = (-B) - sqrt(D);
      info.position = vadd(ray.position, vscale(ray.direction, info.distance));
      info.normal = vnormalize(vsub(info.position, s->position));
      info.color = materialColor(&s->material, 0, 0);
    }
    return info;
  }
  double Vd = vdot(s->position, ray.direction);
  if (Vd == 0) return info;
  double t = -(vdot(s->position, ray.position) + s->d) / Vd;
  if (t <= 0) return info;
  info.isHit = 1;
  info.position = vadd(ray.position, vscale(ray.direction, t));
  info.normal = s->position;
  info.distance = t;
  if (s->material.hasTexture) {
    Vector vU = vec(s->position.y, s->position.z, -s->position.x);
    Vector vV = vcross(vU, s->position);
    info.color = materialColor(&s->material, vdot(info.position, vU), vdot(info.position, vV));
  } else {
    info.color = materialColor(&s->material, 0, 0);
  }
  return info;
}

typedef struct { Vector position, lookAt, up, equator, screen; } Camera;
typedef struct { Vector position; Color color; } Light;
typedef struct {
  Camera camera;
  Color background; double ambience;
  Shape shapes[3]; int shapeCount;
  Light lights[2]; int lightCount;
} Scene;

static Ray getRay(const Camera *c, double vx, double vy) {
  Vector pos = vsub(c->screen, vsub(vscale(c->equator, vx), vscale(c->up, vy)));
  pos.y *= -1;
  Vector dir = vsub(pos, c->position);
  return (Ray){pos, vnormalize(dir)};
}

typedef struct { double canvasWidth, canvasHeight; int rayDepth; int checkNumber; } Engine;

static Info testIntersection(const Scene *scene, Ray ray, const Shape *exclude) {
  int hitCount = 0;
  Info best = newInfo();
  best.distance = 2000;
  for (int i = 0; i < scene->shapeCount; i++) {
    const Shape *shape = &scene->shapes[i];
    if (shape != exclude) {
      Info info = intersect(shape, ray);
      if (info.isHit && info.distance >= 0 && info.distance < best.distance) {
        best = info;
        hitCount++;
      }
    }
  }
  best.hitCount = hitCount;
  return best;
}

static Color rayTrace(const Engine *e, Info *info, Ray ray, const Scene *scene, int depth) {
  Color c = cscale(info->color, scene->ambience);
  double shininess = pow(10, info->shape->material.gloss + 1);
  for (int i = 0; i < scene->lightCount; i++) {
    const Light *light = &scene->lights[i];
    Vector v = vnormalize(vsub(light->position, info->position));
    double L = vdot(v, info->normal);
    if (L > 0) c = cadd(c, cmul(info->color, cscale(light->color, L)));
    if (depth <= e->rayDepth && info->shape->material.reflection > 0) {
      double c1 = -vdot(info->normal, ray.direction);
      Ray reflectionRay = {info->position, vadd(vscale(info->normal, 2 * c1), ray.direction)};
      Info refl = testIntersection(scene, reflectionRay, info->shape);
      if (refl.isHit && refl.distance > 0) refl.color = rayTrace(e, &refl, reflectionRay, scene, depth + 1);
      else refl.color = scene->background;
      c = cblend(c, refl.color, info->shape->material.reflection);
    }
    Ray shadowRay = {info->position, v};
    Info shadowInfo = testIntersection(scene, shadowRay, info->shape);
    if (shadowInfo.isHit && shadowInfo.shape != info->shape) {
      Color vA = cscale(c, 0.5);
      double dB = 0.5 * pow(shadowInfo.shape->material.transparency, 0.5);
      c = caddscalar(vA, dB);
    }
    if (!shadowInfo.isHit && info->shape->material.gloss > 0) {
      Vector Lv = vnormalize(vsub(info->shape->position, light->position));
      Vector E = vnormalize(vsub(scene->camera.position, info->shape->position));
      Vector H = vnormalize(vsub(E, Lv));
      double glossWeight = pow(fmax(vdot(info->normal, H), 0), shininess);
      c = cadd(cscale(light->color, glossWeight), c);
    }
  }
  return climit(c);
}

static Material solid(Color c, double reflection, double transparency, double gloss) {
  Material m = {SOLID, reflection, transparency, gloss, 1, c, {0}, {0}, 0};
  return m;
}

static void renderScene(void) {
  Scene scene;
  scene.camera.position = vec(0, 0, -15);
  scene.camera.lookAt = vec(-0.2, 0, 5);
  scene.camera.up = vec(0, 1, 0);
  scene.camera.equator = vcross(vnormalize(scene.camera.lookAt), scene.camera.up);
  scene.camera.screen = vadd(scene.camera.position, scene.camera.lookAt);
  scene.background = color(0.5, 0.5, 0.5);
  scene.ambience = 0.4;
  scene.shapes[0] = (Shape){SPHERE, vec(-1.5, 1.5, 2), solid(color(0, 0.5, 0.5), 0.3, 0, 2), 1.5, 0};
  scene.shapes[1] = (Shape){SPHERE, vec(1, 0.25, 1), solid(color(0.9, 0.9, 0.9), 0.1, 0, 1.5), 0.5, 0};
  Material chess = {CHESSBOARD, 0.2, 0, 1, 1, {0}, color(1, 1, 1), color(0, 0, 0), 0.7};
  scene.shapes[2] = (Shape){PLANE, vnormalize(vec(0.1, 0.9, -0.5)), chess, 0, 1.2};
  scene.shapeCount = 3;
  scene.lights[0] = (Light){vec(5, 10, -1), color(0.8, 0.8, 0.8)};
  scene.lights[1] = (Light){vec(-3, 5, -15), color(0.8, 0.8, 0.8)};
  scene.lightCount = 2;

  Engine e = {100.0 / 5, 100.0 / 5, 2, 0};
  for (int x = 0; x < e.canvasWidth; x++) {
    for (int y = 0; y < e.canvasHeight; y++) {
      double xp = x * 1 / e.canvasWidth * 2 - 1;
      double yp = y * 1 / e.canvasHeight * 2 - 1;
      Ray ray = getRay(&scene.camera, xp, yp);
      Info info = testIntersection(&scene, ray, NULL);
      Color c = info.isHit ? rayTrace(&e, &info, ray, &scene, 0) : scene.background;
      if (x == y) e.checkNumber += cbrightness(c);
    }
  }
  if (e.checkNumber != 2321) {
    fprintf(stderr, "Scene rendered incorrectly: %d\n", e.checkNumber);
    exit(1);
  }
}

int main(int argc, char **argv) {
  int iterations = argc > 1 ? atoi(argv[1]) : 120;
  struct timespec a, b;
  clock_gettime(CLOCK_MONOTONIC, &a);
  for (int i = 0; i < iterations; i++) for (int j = 0; j < 15; j++) renderScene();
  clock_gettime(CLOCK_MONOTONIC, &b);
  printf("raytrace %d iterations %.3f ms\n", iterations, (b.tv_sec - a.tv_sec) * 1e3 + (b.tv_nsec - a.tv_nsec) / 1e6);
  return 0;
}
