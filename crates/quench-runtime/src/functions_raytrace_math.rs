fn trace_pixel(ray: NativeRay, scene: &NativeScene, options: NativeOptions) -> NativeColor {
    let Some(hit) = intersect(ray, scene, None) else {
        return scene.background;
    };
    trace_hit(&hit, ray, scene, options, 0)
}

fn intersect(ray: NativeRay, scene: &NativeScene, exclude: Option<usize>) -> Option<NativeHit> {
    scene
        .shapes
        .iter()
        .enumerate()
        .filter(|(index, _)| Some(*index) != exclude)
        .filter_map(|(index, shape)| intersect_shape(index, shape, ray))
        .filter(|hit| hit.distance >= 0.0)
        .min_by(|left, right| left.distance.total_cmp(&right.distance))
}

fn intersect_shape(index: usize, shape: &NativeShape, ray: NativeRay) -> Option<NativeHit> {
    match shape {
        NativeShape::Sphere {
            center,
            radius,
            material,
        } => sphere_hit(index, *center, *radius, material, ray),
        NativeShape::Plane {
            normal,
            d,
            material,
        } => plane_hit(index, *normal, *d, material, ray),
    }
}

fn sphere_hit(
    index: usize,
    center: NativeVec3,
    radius: f64,
    material: &NativeMaterial,
    ray: NativeRay,
) -> Option<NativeHit> {
    let dst = sub(ray.position, center);
    let b = dot(dst, ray.direction);
    let discriminant = b * b - (dot(dst, dst) - radius * radius);
    (discriminant > 0.0).then(|| {
        let distance = -b - discriminant.sqrt();
        let position = add(ray.position, scale(ray.direction, distance));
        NativeHit {
            shape: index,
            position,
            normal: normalize(sub(position, center)),
            color: material.color,
            distance,
        }
    })
}

fn plane_hit(
    index: usize,
    normal: NativeVec3,
    d: f64,
    material: &NativeMaterial,
    ray: NativeRay,
) -> Option<NativeHit> {
    let vd = dot(normal, ray.direction);
    if vd == 0.0 {
        return None;
    }
    let distance = -(dot(normal, ray.position) + d) / vd;
    if distance <= 0.0 {
        return None;
    }
    let position = add(ray.position, scale(ray.direction, distance));
    let color = material_color(
        material,
        dot(
            position,
            NativeVec3 {
                x: normal.y,
                y: normal.z,
                z: -normal.x,
            },
        ),
        dot(
            position,
            cross(
                NativeVec3 {
                    x: normal.y,
                    y: normal.z,
                    z: -normal.x,
                },
                normal,
            ),
        ),
    );
    Some(NativeHit {
        shape: index,
        position,
        normal,
        color,
        distance,
    })
}

fn material_color(material: &NativeMaterial, u: f64, v: f64) -> NativeColor {
    if !material.textured {
        return material.color;
    }
    if wrap(u * material.density) * wrap(v * material.density) < 0.0 {
        material.color
    } else {
        material.odd
    }
}

fn wrap(mut value: f64) -> f64 {
    value %= 2.0;
    if value < -1.0 {
        value += 2.0;
    }
    if value >= 1.0 {
        value -= 2.0;
    }
    value
}

fn trace_hit(
    hit: &NativeHit,
    ray: NativeRay,
    scene: &NativeScene,
    options: NativeOptions,
    depth: usize,
) -> NativeColor {
    let material = match &scene.shapes[hit.shape] {
        NativeShape::Sphere { material, .. } | NativeShape::Plane { material, .. } => material,
    };
    let mut color = scale_color(hit.color, scene.ambience);
    let shininess = 10_f64.powf(material.gloss + 1.0);
    for light in &scene.lights {
        let direction = normalize(sub(light.position, hit.position));
        color = light_color(color, hit, direction, light.color, options);
        if depth <= options.depth && options.reflections && material.reflection > 0.0 {
            let reflected = reflection_color(hit, ray, scene, options, depth);
            color = blend(color, reflected, material.reflection);
        }
        let shadow_hit = options
            .shadows
            .then(|| {
                intersect(
                    NativeRay {
                        position: hit.position,
                        direction,
                    },
                    scene,
                    Some(hit.shape),
                )
            })
            .flatten();
        if let Some(shadow) = &shadow_hit {
            let transparency = shape_material(&scene.shapes[shadow.shape]).transparency;
            let shade = 0.5 * transparency.sqrt();
            color = add_color(
                scale_color(color, 0.5),
                NativeColor {
                    r: shade,
                    g: shade,
                    b: shade,
                },
            );
        }
        if options.highlights && shadow_hit.is_none() && material.gloss > 0.0 {
            color = highlight(
                color,
                hit,
                &scene.shapes[hit.shape],
                light,
                scene.camera,
                shininess,
            );
        }
    }
    limit(color)
}

fn light_color(
    base: NativeColor,
    hit: &NativeHit,
    direction: NativeVec3,
    light: NativeColor,
    options: NativeOptions,
) -> NativeColor {
    let weight = dot(direction, hit.normal);
    if options.diffuse && weight > 0.0 {
        add_color(base, mul_color(hit.color, scale_color(light, weight)))
    } else {
        base
    }
}

fn reflection_color(
    hit: &NativeHit,
    ray: NativeRay,
    scene: &NativeScene,
    options: NativeOptions,
    depth: usize,
) -> NativeColor {
    let reflected = add(
        scale(hit.normal, 2.0 * -dot(hit.normal, ray.direction)),
        ray.direction,
    );
    let ray = NativeRay {
        position: hit.position,
        direction: reflected,
    };
    intersect(ray, scene, Some(hit.shape)).map_or(scene.background, |next| {
        trace_hit(&next, ray, scene, options, depth + 1)
    })
}

fn highlight(
    base: NativeColor,
    hit: &NativeHit,
    shape: &NativeShape,
    light: &NativeLight,
    camera: NativeVec3,
    shininess: f64,
) -> NativeColor {
    let shape_position = match shape {
        NativeShape::Sphere { center, .. } => *center,
        NativeShape::Plane { normal, .. } => *normal,
    };
    let lv = normalize(sub(shape_position, light.position));
    let eye = normalize(sub(camera, shape_position));
    let half = normalize(sub(eye, lv));
    add_color(
        scale_color(light.color, dot(hit.normal, half).max(0.0).powf(shininess)),
        base,
    )
}

fn shape_material(shape: &NativeShape) -> &NativeMaterial {
    match shape {
        NativeShape::Sphere { material, .. } | NativeShape::Plane { material, .. } => material,
    }
}

fn add(a: NativeVec3, b: NativeVec3) -> NativeVec3 {
    NativeVec3 {
        x: a.x + b.x,
        y: a.y + b.y,
        z: a.z + b.z,
    }
}
fn sub(a: NativeVec3, b: NativeVec3) -> NativeVec3 {
    NativeVec3 {
        x: a.x - b.x,
        y: a.y - b.y,
        z: a.z - b.z,
    }
}
fn scale(v: NativeVec3, factor: f64) -> NativeVec3 {
    NativeVec3 {
        x: v.x * factor,
        y: v.y * factor,
        z: v.z * factor,
    }
}
fn dot(a: NativeVec3, b: NativeVec3) -> f64 {
    a.x * b.x + a.y * b.y + a.z * b.z
}
fn cross(a: NativeVec3, b: NativeVec3) -> NativeVec3 {
    NativeVec3 {
        x: -a.z * b.y + a.y * b.z,
        y: a.z * b.x - a.x * b.z,
        z: -a.y * b.x + a.x * b.y,
    }
}
fn normalize(v: NativeVec3) -> NativeVec3 {
    scale(v, 1.0 / dot(v, v).sqrt())
}
fn add_color(a: NativeColor, b: NativeColor) -> NativeColor {
    NativeColor {
        r: a.r + b.r,
        g: a.g + b.g,
        b: a.b + b.b,
    }
}
fn scale_color(c: NativeColor, f: f64) -> NativeColor {
    NativeColor {
        r: c.r * f,
        g: c.g * f,
        b: c.b * f,
    }
}
fn mul_color(a: NativeColor, b: NativeColor) -> NativeColor {
    NativeColor {
        r: a.r * b.r,
        g: a.g * b.g,
        b: a.b * b.b,
    }
}
fn blend(a: NativeColor, b: NativeColor, w: f64) -> NativeColor {
    add_color(scale_color(a, 1.0 - w), scale_color(b, w))
}
fn limit(c: NativeColor) -> NativeColor {
    NativeColor {
        r: c.r.clamp(0.0, 1.0),
        g: c.g.clamp(0.0, 1.0),
        b: c.b.clamp(0.0, 1.0),
    }
}
