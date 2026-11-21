/*
 * Header-only C11 port of the JetStream dotnet RayTracer (src/dotnet/RayTracer, all .cs files),
 * restricted to the code path MainJS uses: ConfigureScene -> Scene.TwoPlanes with
 * ReflectionDepth 5, FieldOfView 120, a 1920x1080 camera render size, and
 * RenderScene with hardwareConcurrency 1 (one stripe, rendered serially).
 *
 * Float semantics follow the C# (System.Numerics-style Vector128<float> lanes, float
 * arithmetic, double-precision Math.Sqrt/Tan/Atan2/Asin/Acos-free paths). Build with
 * -std=c11 (no FMA contraction) so a*b+c is never fused.
 *
 * Quirks of the C# reproduced on purpose:
 *  - Camera.RenderScene assigns renderSize directly, so yRatio stays 1080/1920 from the
 *    constructor for every render size.
 *  - Camera ctor computes right/up from the un-normalized forward parameter.
 *  - Ray ctor normalizes its direction; refraction rays normalize twice.
 *  - Sphere normals are not normalized.
 *  - GetRefractionRay takes sqrt only when c2 < 0 (a NaN path), as in the C#.
 */
#ifndef DOTNET_RAYTRACER_H
#define DOTNET_RAYTRACER_H

#include <math.h>

#ifndef M_PI
#define M_PI 3.14159265358979323846
#endif
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

/* ---- Vector128<float> lanes; also used for Color (R,G,B,A) ---- */

typedef struct { float x, y, z, w; } dn_vec;
typedef dn_vec dn_color;

static dn_vec dn_v4(float x, float y, float z, float w) {
    dn_vec r = { x, y, z, w };
    return r;
}

static dn_vec dn_add(dn_vec a, dn_vec b) {
    return dn_v4(a.x + b.x, a.y + b.y, a.z + b.z, a.w + b.w);
}

static dn_vec dn_sub(dn_vec a, dn_vec b) {
    return dn_v4(a.x - b.x, a.y - b.y, a.z - b.z, a.w - b.w);
}

/* Componentwise product (Vector128 * Vector128, Color * Color). */
static dn_vec dn_mul(dn_vec a, dn_vec b) {
    return dn_v4(a.x * b.x, a.y * b.y, a.z * b.z, a.w * b.w);
}

/* Broadcast scalar product (Vector128 * Vector128.Create(s), Color * float). */
static dn_vec dn_scale(dn_vec a, float s) {
    return dn_v4(a.x * s, a.y * s, a.z * s, a.w * s);
}

static dn_vec dn_neg(dn_vec a) {
    return dn_v4(-a.x, -a.y, -a.z, -a.w);
}

static bool dn_veq(dn_vec a, dn_vec b) {
    return a.x == b.x && a.y == b.y && a.z == b.z && a.w == b.w;
}

/* Extensions.DotR: lanes X, Y, Z only, summed left to right. */
static float dn_dot(dn_vec a, dn_vec b) {
    float x = a.x * b.x;
    float y = a.y * b.y;
    float z = a.z * b.z;
    return x + y + z;
}

/* Extensions.Magnitude: (float)Math.Sqrt(DotR(v, v)). */
static float dn_mag(dn_vec v) {
    return (float)sqrt((double)dn_dot(v, v));
}

/* Extensions.Normalize: v / Create(Magnitude(v)). */
static dn_vec dn_norm(dn_vec v) {
    float m = dn_mag(v);
    return dn_v4(v.x / m, v.y / m, v.z / m, v.w / m);
}

/* Util.CrossProduct (w lane forced to 0). */
static dn_vec dn_cross(dn_vec l, dn_vec r) {
    return dn_v4(l.y * r.z - l.z * r.y,
                 l.z * r.x - l.x * r.z,
                 l.x * r.y - l.y * r.x,
                 0.0f);
}

static float dn_clampf(float value, float min, float max) {
    return value > max ? max : value < min ? min : value;
}

/* Util.Lerp(float). */
static float dn_lerpf(float from, float to, float t) {
    return (from * (1 - t)) + (to * t);
}

/* Color.Lerp: clamps t, then from*(1-t) + to*t. */
static dn_color dn_color_lerp(dn_color from, dn_color to, float t) {
    t = dn_clampf(t, 0.0f, 1.0f);
    return dn_add(dn_scale(from, 1 - t), dn_scale(to, t));
}

/* Color.Limited: clamp every component to [0, 1]. */
static dn_color dn_color_limited(dn_color c) {
    return dn_v4(dn_clampf(c.x, 0, 1), dn_clampf(c.y, 0, 1),
                 dn_clampf(c.z, 0, 1), dn_clampf(c.w, 0, 1));
}

/* Util.DegreesToRadians: (float)((a / 360f) * 2 * Math.PI). */
static float dn_degrees_to_radians(float angle) {
    float t = (angle / 360.0f) * 2.0f;
    return (float)((double)t * M_PI);
}

/* (byte)(float) as .NET 9 does it: saturating, NaN -> 0. */
static uint8_t dn_to_byte(float v) {
    if (!(v > 0.0f)) return 0;
    if (v >= 255.0f) return 255;
    return (uint8_t)(int)v;
}

/* ---- Rays and intersections ---- */

typedef struct { dn_vec origin, direction; float distance; } dn_ray;

/* Ray ctor: the direction is always normalized. */
static dn_ray dn_make_ray(dn_vec origin, dn_vec direction, float distance) {
    dn_ray r;
    r.origin = origin;
    r.direction = dn_norm(direction);
    r.distance = distance;
    return r;
}

typedef struct dn_object dn_object;

typedef struct { float u, v; } dn_uv;

typedef struct {
    dn_vec point, normal, impact;
    const dn_object *hit;
    dn_color color;
    float distance;
} dn_intersection;

/* ---- Materials (SolidMaterial / CheckerboardMaterial) ---- */

typedef struct dn_material dn_material;
struct dn_material {
    dn_color (*diffuse)(const dn_material *m, float u, float v);
    dn_color a; /* solid: diffuse color; checkerboard: even color */
    dn_color b; /* checkerboard: odd color (unused for solid) */
    float reflectivity, refractivity, opacity, glossiness;
};

static dn_color dn_solid_diffuse(const dn_material *m, float u, float v) {
    (void)u; (void)v;
    return m->a;
}

static dn_color dn_checker_diffuse(const dn_material *m, float u, float v) {
    if ((u <= 0.5f && v <= .5f) || (u > 0.5f && v > 0.5f)) {
        return m->a;
    }
    return m->b;
}

static dn_material dn_solid_material(dn_color color, float reflectivity, float refractivity,
                                     float opacity, float glossiness) {
    dn_material m;
    m.diffuse = dn_solid_diffuse;
    m.a = color;
    m.b = color;
    m.reflectivity = reflectivity;
    m.refractivity = refractivity;
    m.opacity = opacity;
    m.glossiness = glossiness;
    return m;
}

static dn_material dn_checker_material(dn_color even, dn_color odd, float opacity,
                                       float reflectivity, float refractivity, float glossiness) {
    dn_material m;
    m.diffuse = dn_checker_diffuse;
    m.a = even;
    m.b = odd;
    m.reflectivity = reflectivity;
    m.refractivity = refractivity;
    m.opacity = opacity;
    m.glossiness = glossiness;
    return m;
}

/* ---- Scene objects: virtual dispatch through a vtable of function pointers ---- */

typedef struct {
    bool (*try_intersect)(const dn_object *o, dn_ray ray, dn_intersection *out);
    dn_uv (*uv)(const dn_object *o, dn_vec position);
} dn_object_vt;

/* One tagged struct covers Sphere, InfinitePlane and Disc; the vtable picks behavior. */
struct dn_object {
    const dn_object_vt *vt;
    dn_vec position;
    dn_material material;
    /* Sphere */
    float radius;
    /* InfinitePlane / Disc */
    dn_vec normal;      /* normalized in ctor */
    dn_vec u_dir, v_dir;
    float cell_width;
    float disc_radius;
};

static dn_uv dn_plane_uv(const dn_object *o, dn_vec position) {
    dn_vec uvPosition = dn_add(o->position, position);

    float uMag = dn_dot(uvPosition, o->u_dir);
    float u = dn_mag(dn_scale(o->u_dir, uMag));
    if (uMag < 0) {
        u += o->cell_width / 2.0f;
    }
    u = fmodf(u, o->cell_width) / o->cell_width;

    float vMag = dn_dot(uvPosition, o->v_dir);
    float v = dn_mag(dn_scale(o->v_dir, vMag));
    if (vMag < 0) {
        v += o->cell_width / 2.0f;
    }
    v = fmodf(v, o->cell_width) / o->cell_width;

    dn_uv r = { u, v };
    return r;
}

static bool dn_plane_try(const dn_object *o, dn_ray ray, dn_intersection *out) {
    dn_vec vecDirection = ray.direction;
    dn_vec rayToPlaneDirection = dn_sub(ray.origin, o->position);

    float D = dn_dot(o->normal, vecDirection);
    float N = -dn_dot(o->normal, rayToPlaneDirection);

    if (fabsf(D) <= .0005f) {
        return false;
    }

    float sI = N / D;
    if (sI < 0 || sI > ray.distance) {
        return false;
    }

    dn_vec intersectionPoint = dn_add(ray.origin, dn_scale(vecDirection, sI));
    dn_uv uv = o->vt->uv(o, intersectionPoint);
    dn_color color = o->material.diffuse(&o->material, uv.u, uv.v);

    out->point = intersectionPoint;
    out->normal = o->normal;
    out->impact = ray.direction;
    out->hit = o;
    out->color = color;
    out->distance = dn_mag(dn_sub(ray.origin, intersectionPoint));
    return true;
}

static bool dn_disc_within(const dn_object *o, dn_vec location) {
    float distanceFromCenter = dn_mag(dn_sub(o->position, location));
    return distanceFromCenter <= o->disc_radius;
}

static bool dn_disc_try(const dn_object *o, dn_ray ray, dn_intersection *out) {
    if (dn_plane_try(o, ray, out) && dn_disc_within(o, out->point)) {
        return true;
    }
    return false;
}

static dn_uv dn_sphere_uv(const dn_object *o, dn_vec hitPosition) {
    dn_vec toCenter = dn_norm(dn_sub(hitPosition, o->position));

    float u = (float)(0.5 + (atan2((double)toCenter.z, (double)toCenter.x) / (2 * M_PI)));
    float v = (float)(0.5 - (asin((double)toCenter.y)) / M_PI);

    dn_uv r = { u, v };
    return r;
}

static bool dn_sphere_try(const dn_object *o, dn_ray ray, dn_intersection *out) {
    dn_vec rayToSphere = dn_sub(ray.origin, o->position);
    float B = dn_dot(rayToSphere, ray.direction);
    float C = dn_dot(rayToSphere, rayToSphere) - (o->radius * o->radius);
    float D = B * B - C;

    if (D > 0) {
        float distance = -B - (float)sqrt((double)D);
        dn_vec hitPosition = dn_add(ray.origin, dn_scale(ray.direction, distance));
        dn_vec normal = dn_sub(hitPosition, o->position);
        dn_uv uv = o->vt->uv(o, hitPosition);

        out->point = hitPosition;
        out->normal = normal;
        out->impact = ray.direction;
        out->hit = o;
        out->color = o->material.diffuse(&o->material, uv.u, uv.v);
        out->distance = distance;
        return true;
    }
    return false;
}

static const dn_object_vt dn_vt_sphere = { dn_sphere_try, dn_sphere_uv };
static const dn_object_vt dn_vt_plane = { dn_plane_try, dn_plane_uv };
static const dn_object_vt dn_vt_disc = { dn_disc_try, dn_plane_uv };

static dn_object dn_make_sphere(dn_vec position, dn_material material, float radius) {
    dn_object o;
    memset(&o, 0, sizeof o);
    o.vt = &dn_vt_sphere;
    o.position = position;
    o.material = material;
    o.radius = radius;
    return o;
}

/* InfinitePlane ctor. The C# reads its normalDirection parameter (not the field) below,
 * so the un-normalized argument feeds the cross products. */
static dn_object dn_make_plane_common(const dn_object_vt *vt, dn_vec position, dn_material material,
                                      dn_vec normalDirection, float cellWidth) {
    static const dn_vec fwd = { 0.0f, 0.0f, 1.0f, 0.0f };
    static const dn_vec right = { 1.0f, 0.0f, 0.0f, 0.0f };
    dn_object o;
    memset(&o, 0, sizeof o);
    o.vt = vt;
    o.position = position;
    o.material = material;
    o.normal = dn_norm(normalDirection);
    if (dn_veq(normalDirection, fwd)) {
        o.u_dir = dn_neg(right);
    } else if (dn_veq(normalDirection, dn_neg(fwd))) {
        o.u_dir = right;
    } else {
        o.u_dir = dn_norm(dn_cross(normalDirection, fwd));
    }
    o.v_dir = dn_neg(dn_norm(dn_cross(normalDirection, o.u_dir)));
    o.cell_width = cellWidth;
    return o;
}

static dn_object dn_make_plane(dn_vec position, dn_material material, dn_vec normalDirection, float cellWidth) {
    return dn_make_plane_common(&dn_vt_plane, position, material, normalDirection, cellWidth);
}

static dn_object dn_make_disc(dn_vec position, dn_material material, dn_vec normalDirection,
                              float radius, float cellWidth) {
    dn_object o = dn_make_plane_common(&dn_vt_disc, position, material, normalDirection, cellWidth);
    o.disc_radius = radius;
    return o;
}

/* ---- Lights ---- */

typedef struct {
    dn_vec position;
    float intensity;
    dn_color color;
} dn_light;

static float dn_light_intensity_at(const dn_light *l, float distance) {
    if (distance > l->intensity) return 0;
    return 1 - (distance / l->intensity);
}

/* ---- Camera ---- */

typedef struct {
    dn_vec position;
    dn_vec forward, up, right;
    dn_vec screenPosition;
    float fieldOfView;
    float yRatio;
    int reflectionDepth;
} dn_camera;

static void dn_camera_recalc_fov(dn_camera *c) {
    float screenDistance = 1.0f / (float)tan((double)(dn_degrees_to_radians(c->fieldOfView) / 2.0f));
    c->screenPosition = dn_add(c->position, dn_scale(c->forward, screenDistance));
}

/* Camera(position, forward, worldUp, fov, renderSize) with renderSize only feeding yRatio,
 * as the C# RenderSize setter does. */
static void dn_camera_init(dn_camera *c, dn_vec position, dn_vec forward, dn_vec worldUp,
                           float fov, int renderWidth, int renderHeight) {
    c->position = position;
    c->reflectionDepth = 5;
    c->forward = dn_norm(forward);
    c->right = dn_norm(dn_cross(worldUp, forward));
    c->up = dn_neg(dn_norm(dn_cross(c->right, forward)));
    c->fieldOfView = fov;
    c->yRatio = (float)renderHeight / (float)renderWidth;
    dn_camera_recalc_fov(c);
}

static dn_ray dn_camera_get_ray(const dn_camera *c, float viewPortX, float viewPortY) {
    dn_vec rayWorldPosition = dn_add(c->screenPosition,
        dn_add(dn_scale(c->right, viewPortX),
               dn_scale(dn_scale(c->up, viewPortY), c->yRatio)));
    dn_vec direction = dn_sub(rayWorldPosition, c->position);
    return dn_make_ray(rayWorldPosition, direction, INFINITY);
}

static dn_ray dn_reflection_ray(dn_vec origin, dn_vec normal, dn_vec impactDirection) {
    float c1 = dn_dot(dn_neg(normal), impactDirection);
    dn_vec reflectionDirection = dn_add(impactDirection, dn_scale(normal, 2 * c1));
    return dn_make_ray(dn_add(origin, dn_scale(reflectionDirection, .01f)), reflectionDirection, INFINITY);
}

static dn_ray dn_refraction_ray(dn_vec origin, dn_vec normal, dn_vec previousDirection, float refractivity) {
    float c1 = dn_dot(normal, previousDirection);
    float c2 = 1 - refractivity * refractivity * (1 - c1 * c1);
    if (c2 < 0)
        c2 = (float)sqrt((double)c2);
    dn_vec refractionDirection = dn_scale(
        dn_sub(dn_scale(normal, refractivity * c1 - c2), dn_scale(previousDirection, refractivity)),
        -1.0f);
    /* The C# passes refractionDirection.Normalize() to the Ray ctor, which normalizes again. */
    return dn_make_ray(origin, dn_norm(refractionDirection), INFINITY);
}

/* ---- Scene ---- */

#define DN_MAX_OBJECTS 8
#define DN_MAX_LIGHTS 2

typedef struct {
    dn_object objects[DN_MAX_OBJECTS];
    int n_objects;
    dn_light lights[DN_MAX_LIGHTS];
    int n_lights;
    dn_camera camera;
    dn_color background;
    float ambient_intensity;
    dn_color ambient_color;
} dn_scene;

static void dn_scene_add_object(dn_scene *s, dn_object o) {
    s->objects[s->n_objects++] = o;
}

static void dn_scene_add_light(dn_scene *s, dn_light l) {
    s->lights[s->n_lights++] = l;
}

/* Scene.TwoPlanes, in the C# order (object order decides ties in the closest-hit search). */
static void dn_build_two_planes(dn_scene *s) {
    static const dn_color white = { 1, 1, 1, 1 };
    static const dn_color grey = { .6f, .6f, .6f, 1 };
    static const dn_color yellow = { 1, 1, 0, 1 };
    static const dn_color red = { 1, 0, 0, 1 };
    static const dn_color silver = { 192.0f / 255.0f, 192.0f / 255.0f, 192.0f / 255.0f, 255.0f / 255.0f };
    static const dn_color sky = { 102.0f / 255.0f, 152.0f / 255.0f, 1.0f, 1.0f };

    memset(s, 0, sizeof *s);
    s->background = sky;
    s->ambient_intensity = 0.10f;
    s->ambient_color = white;

    dn_camera_init(&s->camera, dn_v4(0.0f, 2.0f, -2.0f, 0), dn_v4(0, -.25f, 1.0f, 0),
                   dn_v4(0, 1, 0, 0), 90.0f, 1920, 1080);
    /* ConfigureScene: ReflectionDepth = 5, FieldOfView = 120 (setter recalculates). */
    s->camera.reflectionDepth = 5;
    s->camera.fieldOfView = 120.0f;
    dn_camera_recalc_fov(&s->camera);

    dn_scene_add_object(s, dn_make_plane(dn_v4(0, -5.0f, 0, 0),
        dn_checker_material(white, grey, 1.0f, .1f, 0.0f, 2.0f), dn_v4(0, 1, 0, 0), 10.0f));
    dn_scene_add_object(s, dn_make_plane(dn_v4(0, 12.0f, 0, 0),
        dn_checker_material(white, grey, 1.0f, .1f, 0.0f, 12.0f), dn_v4(0, -1, 0, 0), 7.0f));

    dn_scene_add_object(s, dn_make_sphere(dn_v4(-5, -2.5f, 8, 0),
        dn_solid_material(yellow, .2f, 0.0f, 1.0f, 1.0f), 2.0f));
    dn_scene_add_object(s, dn_make_sphere(dn_v4(3.0f, 1, 12, 0),
        dn_solid_material(red, .35f, 0.0f, .8f, 1.0f), 2.0f));
    dn_scene_add_object(s, dn_make_sphere(dn_v4(3.0f, -4, 9, 0),
        dn_solid_material(dn_v4(.33f, .8f, 1.0f, 1.0f), .2222f, 0.0f, 1.0f, 1.0f), 1.0f));
    dn_scene_add_object(s, dn_make_sphere(dn_v4(0.0f, 0, 6, 0),
        dn_solid_material(grey, 0.0f, 1.0f, 0.15f, 1.0f), 1.0f));

    dn_scene_add_object(s, dn_make_disc(dn_v4(-25.0f, 2, 25, 0),
        dn_solid_material(silver, .63f, 0.0f, 1.0f, 1.0f), dn_v4(1.0f, 0, -1.0f, 0), 5.0f, 2.0f));

    dn_light light;
    light.position = dn_v4(0.0f, 5, 12, 0);
    light.intensity = 150.0f;
    light.color = white;
    dn_scene_add_light(s, light);
}

/* Camera.TryCalculateIntersection: closest hit, first object wins ties (strict <). */
static bool dn_scene_intersect(const dn_scene *s, dn_ray ray, const dn_object *excluded, dn_intersection *out) {
    float closestDistance = INFINITY;
    dn_intersection closest;
    memset(&closest, 0, sizeof closest);
    for (int i = 0; i < s->n_objects; i++) {
        const dn_object *o = &s->objects[i];
        dn_intersection hit;
        if (o != excluded && o->vt->try_intersect(o, ray, &hit)) {
            if (hit.distance < closestDistance) {
                closestDistance = hit.distance;
                closest = hit;
            }
        }
    }
    if (closestDistance == INFINITY) {
        memset(out, 0, sizeof *out);
        return false;
    }
    *out = closest;
    return true;
}

static dn_color dn_recursive_color(const dn_scene *s, const dn_intersection *x, int depth) {
    /* Ambient light */
    dn_color color = dn_color_lerp(dn_v4(0, 0, 0, 1), dn_mul(x->color, s->ambient_color), s->ambient_intensity);

    for (int li = 0; li < s->n_lights; li++) {
        const dn_light *light = &s->lights[li];
        dn_color lightContribution = dn_v4(0, 0, 0, 0);
        dn_vec towardsLight = dn_norm(dn_sub(light->position, x->point));
        float lightDistance = dn_mag(dn_sub(x->point, light->position));

        /* Diffuse */
        float lightEffectiveness = dn_dot(towardsLight, x->normal);
        if (lightEffectiveness > 0.0f) {
            float intensity = dn_light_intensity_at(light, lightDistance);
            lightContribution = dn_add(lightContribution,
                dn_scale(dn_mul(dn_scale(x->color, intensity), light->color), lightEffectiveness));
        }

        /* Shadow */
        dn_ray shadowRay = dn_make_ray(x->point, towardsLight, INFINITY);
        dn_intersection shadowIntersection;
        if (dn_scene_intersect(s, shadowRay, x->hit, &shadowIntersection) &&
            shadowIntersection.distance < lightDistance) {
            float transparency = 1.0f - shadowIntersection.hit->material.opacity;
            float lightPassThrough = dn_lerpf(.25f, 1.0f, transparency);
            lightContribution = dn_color_lerp(lightContribution, dn_v4(0, 0, 0, 0), 1 - lightPassThrough);
        }

        color = dn_add(color, lightContribution);
    }

    if (depth < s->camera.reflectionDepth) {
        /* Reflection */
        float objectReflectivity = x->hit->material.reflectivity;
        if (objectReflectivity > 0.0f) {
            dn_ray reflectionRay = dn_reflection_ray(x->point, x->normal, x->impact);
            dn_intersection reflectionIntersection;
            if (dn_scene_intersect(s, reflectionRay, x->hit, &reflectionIntersection)) {
                color = dn_color_lerp(color,
                    dn_recursive_color(s, &reflectionIntersection, depth + 1), objectReflectivity);
            }
        }

        /* Refraction */
        float objectRefractivity = x->hit->material.refractivity;
        if (objectRefractivity > 0.0f) {
            dn_ray refractionRay = dn_refraction_ray(x->point, x->normal, x->impact, objectRefractivity);
            dn_intersection refractionIntersection;
            if (dn_scene_intersect(s, refractionRay, x->hit, &refractionIntersection)) {
                dn_color refractedColor = dn_recursive_color(s, &refractionIntersection, depth + 1);
                color = dn_color_lerp(color, refractedColor, 1 - x->hit->material.opacity);
            }
        }
    }

    return dn_color_limited(color);
}

static dn_color dn_trace(const dn_scene *s, dn_ray ray) {
    dn_intersection intersection;
    if (dn_scene_intersect(s, ray, NULL, &intersection)) {
        return dn_recursive_color(s, &intersection, 0);
    }
    return s->background;
}

/* RenderScene with hardwareConcurrency 1: one stripe, rows written from y = height-1 down to 0. */
static void dn_render_rgba_into(const dn_scene *s, uint8_t *dest, int width, int height) {
    size_t offset = 0;
    for (int y = height - 1; y >= 0; y--) {
        for (int x = 0; x < width; x++) {
            float viewPortX = ((2 * x) / (float)width) - 1;
            float viewPortY = ((2 * y) / (float)height) - 1;
            dn_color color = dn_trace(s, dn_camera_get_ray(&s->camera, viewPortX, viewPortY));

            dest[offset++] = dn_to_byte(color.x * 255);
            dest[offset++] = dn_to_byte(color.y * 255);
            dest[offset++] = dn_to_byte(color.z * 255);
            dest[offset++] = 255;
        }
    }
}

/* PrepareToRender + Render: builds the scene and returns a freshly allocated
 * width*height*4 RGBA buffer (caller frees). Returns NULL on allocation failure. */
static uint8_t *dn_render_rgba(int width, int height) {
    dn_scene *scene = malloc(sizeof *scene);
    if (!scene) return NULL;
    dn_build_two_planes(scene);

    size_t bytes = (size_t)width * (size_t)height * 4;
    uint8_t *buffer = malloc(bytes ? bytes : 1);
    if (buffer) {
        dn_render_rgba_into(scene, buffer, width, height);
    }
    free(scene);
    return buffer;
}

/* FNV-1a, 64-bit. */
static uint64_t dn_fnv1a64(const uint8_t *data, size_t n) {
    uint64_t h = 0xcbf29ce484222325ULL;
    for (size_t i = 0; i < n; i++) {
        h ^= data[i];
        h *= 0x100000001b3ULL;
    }
    return h;
}

/* Renders one frame exactly as MainJS does (PrepareToRender then Render) and returns the
 * FNV-1a 64-bit hash of the RGBA buffer. Returns 0 if allocation fails. */
static uint64_t dotnet_render(int width, int height) {
    uint8_t *buffer = dn_render_rgba(width, height);
    if (!buffer) return 0;
    uint64_t hash = dn_fnv1a64(buffer, (size_t)width * (size_t)height * 4);
    free(buffer);
    return hash;
}

#endif /* DOTNET_RAYTRACER_H */
