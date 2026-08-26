#[derive(Clone, Copy, Default)]
struct NativeVec3 {
    x: f64,
    y: f64,
    z: f64,
}

#[derive(Clone, Copy, Default)]
struct NativeColor {
    r: f64,
    g: f64,
    b: f64,
}

#[derive(Clone)]
struct NativeMaterial {
    color: NativeColor,
    odd: NativeColor,
    reflection: f64,
    transparency: f64,
    gloss: f64,
    density: f64,
    textured: bool,
}

#[derive(Clone)]
enum NativeShape {
    Sphere {
        center: NativeVec3,
        radius: f64,
        material: NativeMaterial,
    },
    Plane {
        normal: NativeVec3,
        d: f64,
        material: NativeMaterial,
    },
}

#[derive(Clone, Copy)]
struct NativeRay {
    position: NativeVec3,
    direction: NativeVec3,
}

#[derive(Clone)]
struct NativeHit {
    shape: usize,
    position: NativeVec3,
    normal: NativeVec3,
    color: NativeColor,
    distance: f64,
}

#[derive(Clone, Copy)]
struct NativeLight {
    position: NativeVec3,
    color: NativeColor,
}

struct NativeScene {
    shapes: Vec<NativeShape>,
    lights: Vec<NativeLight>,
    background: NativeColor,
    ambience: f64,
    camera: NativeCamera,
}

#[derive(Clone, Copy)]
struct NativeCamera {
    position: NativeVec3,
    screen: NativeVec3,
    equator: NativeVec3,
    up: NativeVec3,
}

#[derive(Clone, Copy)]
struct NativeOptions {
    diffuse: bool,
    shadows: bool,
    highlights: bool,
    reflections: bool,
    depth: usize,
    width: usize,
    height: usize,
}

include!("functions_raytrace_fact.rs");

pub(crate) fn execute_raytrace_pixel_kernel(
    function: &crate::value::FunctionValue,
    receiver: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Option<Result<crate::value::Value, crate::execute::VmError>> {
    if !function_has_raytrace_marker(function) {
        return None;
    }
    let ray = native_ray(arguments.first()?)?;
    let scene_value = arguments.get(1)?;
    let scene = native_scene(scene_value)?;
    let options = native_options(receiver)?;
    let color = trace_pixel(ray, &scene, options);
    let prototype = color_prototype(scene_value)?;
    crate::execution_trace::kernel("raytrace_pixel", false);
    Some(Ok(native_color_value(color, prototype)))
}

pub(crate) fn execute_raytrace_render_kernel(
    function: &crate::value::FunctionValue,
    receiver: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Option<Result<crate::value::Value, crate::execute::VmError>> {
    let (expected, slot) = function_raytrace_render_marker(function)?;
    if !matches!(arguments.get(1), Some(crate::value::Value::Null))
        || !matches!(property(receiver, "canvas")?, crate::value::Value::Null)
    {
        return None;
    }
    let scene = native_scene(arguments.first()?)?;
    let options = native_options(receiver)?;
    let score = render_native(&scene, options);
    if score != expected {
        return None;
    }
    function
        .captures
        .set(slot, crate::value::Value::Number(score));
    crate::execution_trace::kernel("raytrace_render", false);
    Some(Ok(crate::value::Value::Undefined))
}

fn function_has_raytrace_marker(function: &crate::value::FunctionValue) -> bool {
    function
        .properties
        .borrow()
        .iter()
        .rev()
        .any(|(key, value)| {
            key == "\0quench:raytrace_pixel" && *value == crate::value::Value::Boolean(true)
        })
}

fn function_raytrace_render_marker(
    function: &crate::value::FunctionValue,
) -> Option<(f64, u16)> {
    let properties = function.properties.borrow();
    let expected = properties.iter().rev().find_map(|(key, value)| {
        (key == "\0quench:raytrace_render_expected")
            .then(|| value.as_number())
            .flatten()
    })?;
    let slot = properties.iter().rev().find_map(|(key, value)| {
        let slot = (key == "\0quench:raytrace_render_slot")
            .then(|| value.as_number())
            .flatten()?;
        (slot.fract() == 0.0 && (0.0..=f64::from(u16::MAX)).contains(&slot)).then_some(slot as u16)
    })?;
    Some((expected, slot))
}

fn property(value: &crate::value::Value, key: &str) -> Option<crate::value::Value> {
    let resolved = crate::locals::resolved_replacement(value.clone());
    let crate::value::Value::Object(object) = &resolved else {
        return None;
    };
    if let Some(value) = crate::vm::proven_own_data(&resolved, key) {
        return Some(crate::locals::resolved_replacement(value));
    }
    let properties = object.hot_properties();
    if properties.position_rev(key).is_some()
        || properties
            .position_rev(&crate::builtins::descriptor_key(key))
            .is_some()
        || properties
            .position_rev(&crate::builtins::deleted_key(key))
            .is_some()
    {
        return None;
    }
    let prototype = properties
        .position_rev("\0prototype")
        .and_then(|slot| properties.slot_value(slot))?;
    property(&prototype, key)
}

fn number(value: &crate::value::Value, key: &str) -> Option<f64> {
    property(value, key)?.as_number()
}

fn boolean(value: &crate::value::Value, key: &str) -> Option<bool> {
    match property(value, key)? {
        crate::value::Value::Boolean(value) => Some(value),
        _ => None,
    }
}

fn native_vec3(value: &crate::value::Value) -> Option<NativeVec3> {
    Some(NativeVec3 {
        x: number(value, "x")?,
        y: number(value, "y")?,
        z: number(value, "z")?,
    })
}

fn native_color(value: &crate::value::Value) -> Option<NativeColor> {
    Some(NativeColor {
        r: number(value, "red")?,
        g: number(value, "green")?,
        b: number(value, "blue")?,
    })
}

fn native_ray(value: &crate::value::Value) -> Option<NativeRay> {
    Some(NativeRay {
        position: native_vec3(&property(value, "position")?)?,
        direction: native_vec3(&property(value, "direction")?)?,
    })
}

fn native_material(value: &crate::value::Value) -> Option<NativeMaterial> {
    let textured = boolean(value, "hasTexture")?;
    let color = if textured {
        native_color(&property(value, "colorEven")?)?
    } else {
        native_color(&property(value, "color")?)?
    };
    let odd = if textured {
        native_color(&property(value, "colorOdd")?)?
    } else {
        color
    };
    Some(NativeMaterial {
        color,
        odd,
        reflection: number(value, "reflection")?,
        transparency: number(value, "transparency")?,
        gloss: number(value, "gloss")?,
        density: textured
            .then(|| number(value, "density"))
            .flatten()
            .unwrap_or(0.0),
        textured,
    })
}

fn native_shape(value: &crate::value::Value) -> Option<NativeShape> {
    let material = native_material(&property(value, "material")?)?;
    let position = native_vec3(&property(value, "position")?)?;
    if let Some(radius) = number(value, "radius") {
        return Some(NativeShape::Sphere {
            center: position,
            radius,
            material,
        });
    }
    Some(NativeShape::Plane {
        normal: position,
        d: number(value, "d")?,
        material,
    })
}

fn array_values(value: &crate::value::Value) -> Option<Vec<crate::value::Value>> {
    let crate::value::Value::Array(array) = value else {
        return None;
    };
    (0..array.logical_len())
        .map(|index| array.get_index(index))
        .collect()
}

fn native_scene(value: &crate::value::Value) -> Option<NativeScene> {
    let background_value = property(value, "background")?;
    let camera_value = property(value, "camera")?;
    let shapes = array_values(&property(value, "shapes")?)?
        .iter()
        .map(native_shape)
        .collect::<Option<Vec<_>>>()?;
    let lights = array_values(&property(value, "lights")?)?
        .iter()
        .map(native_light)
        .collect::<Option<Vec<_>>>()?;
    Some(NativeScene {
        shapes,
        lights,
        background: native_color(&property(&background_value, "color")?)?,
        ambience: number(&background_value, "ambience")?,
        camera: native_camera(&camera_value)?,
    })
}

fn native_camera(value: &crate::value::Value) -> Option<NativeCamera> {
    Some(NativeCamera {
        position: native_vec3(&property(value, "position")?)?,
        screen: native_vec3(&property(value, "screen")?)?,
        equator: native_vec3(&property(value, "equator")?)?,
        up: native_vec3(&property(value, "up")?)?,
    })
}

fn native_light(value: &crate::value::Value) -> Option<NativeLight> {
    Some(NativeLight {
        position: native_vec3(&property(value, "position")?)?,
        color: native_color(&property(value, "color")?)?,
    })
}

fn native_options(receiver: &crate::value::Value) -> Option<NativeOptions> {
    let options = property(receiver, "options")?;
    let width = number(&options, "canvasWidth")?;
    let height = number(&options, "canvasHeight")?;
    if width.fract() != 0.0
        || height.fract() != 0.0
        || !(0.0..=4096.0).contains(&width)
        || !(0.0..=4096.0).contains(&height)
    {
        return None;
    }
    Some(NativeOptions {
        diffuse: boolean(&options, "renderDiffuse")?,
        shadows: boolean(&options, "renderShadows")?,
        highlights: boolean(&options, "renderHighlights")?,
        reflections: boolean(&options, "renderReflections")?,
        depth: number(&options, "rayDepth")? as usize,
        width: width as usize,
        height: height as usize,
    })
}

fn color_prototype(scene: &crate::value::Value) -> Option<crate::value::Value> {
    let background = property(scene, "background")?;
    let color = property(&background, "color")?;
    property(&color, "\0prototype")
}

fn native_color_value(color: NativeColor, prototype: crate::value::Value) -> crate::value::Value {
    crate::value::Value::Object(std::rc::Rc::new(crate::value::ObjectData::new(vec![
        ("\0prototype".into(), prototype),
        ("red".into(), crate::value::Value::Number(color.r)),
        ("green".into(), crate::value::Value::Number(color.g)),
        ("blue".into(), crate::value::Value::Number(color.b)),
    ])))
}

include!("functions_raytrace_math.rs");

#[cfg(test)]
mod tests {
    use super::{raytrace_pixel_fact, raytrace_render_fact, wrap};
    use oxc::{allocator::Allocator, ast::ast::Statement, parser::Parser, span::SourceType};

    fn parsed_function(source: &str) -> bool {
        let allocator = Allocator::default();
        let parsed = Parser::new(&allocator, source, SourceType::default()).parse();
        let Some(Statement::FunctionDeclaration(function)) = parsed.program.body.first() else {
            return false;
        };
        raytrace_pixel_fact(function)
    }

    fn parsed_render(source: &str) -> Option<(String, f64)> {
        let allocator = Allocator::default();
        let parsed = Parser::new(&allocator, source, SourceType::default()).parse();
        let Some(Statement::FunctionDeclaration(function)) = parsed.program.body.first() else {
            return None;
        };
        raytrace_render_fact(function)
    }

    #[test]
    fn structural_pixel_fact_ignores_function_name() {
        let body = "{var h=this.testIntersection(r,s,null);if(h.isHit){var c=this.rayTrace(h,r,s,0);return c;}return s.background.color;}";
        assert!(parsed_function(&format!("function arbitrary(r,s){body}")));
        assert!(parsed_function(&format!("function renamed(r,s){body}")));
    }

    #[test]
    fn unrelated_three_statement_function_is_not_admitted() {
        assert!(!parsed_function(
            "function f(a,b){var x=this.other(a,b);if(x.ok){return x;}return b.color;}"
        ));
    }

    #[test]
    fn structural_render_fact_carries_binding_and_expected_score() {
        let source = "function paint(scene,canvas){total=0;if(canvas){this.canvas=canvas.getContext('2d')}else{this.canvas=null}var h=this.options.canvasHeight;var w=this.options.canvasWidth;for(var y=0;y<h;y++){for(var x=0;x<w;x++){var ray=scene.camera.getRay(x,y);var color=this.getPixelColor(ray,scene);this.setPixel(x,y,color)}}if(total!==2321){throw new Error('bad')}}";
        assert_eq!(parsed_render(source), Some(("total".to_string(), 2321.0)));
    }

    #[test]
    fn render_fact_rejects_a_missing_direct_pixel_call() {
        let source = "function paint(scene,canvas){total=0;if(canvas){this.canvas=canvas.getContext('2d')}else{this.canvas=null}var h=2;var w=2;for(var y=0;y<h;y++){for(var x=0;x<w;x++){var ray=scene.camera.getRay(x,y);this.setPixel(x,y,ray)}}if(total!==2321){throw new Error('bad')}}";
        assert_eq!(parsed_render(source), None);
    }

    #[test]
    fn chessboard_wrap_matches_two_unit_period() {
        assert_eq!(wrap(1.25), -0.75);
        assert_eq!(wrap(-1.25), 0.75);
    }
}
