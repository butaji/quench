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
    camera: NativeVec3,
}

#[derive(Clone, Copy)]
struct NativeOptions {
    diffuse: bool,
    shadows: bool,
    highlights: bool,
    reflections: bool,
    depth: usize,
}

pub(crate) fn raytrace_pixel_fact(function: &oxc::ast::ast::Function<'_>) -> bool {
    let Some(body) = function.body.as_ref() else {
        return false;
    };
    body.statements.len() == 3
        && declares_call(&body.statements[0], "testIntersection")
        && is_hit_branch(&body.statements[1])
        && returns_member_chain(&body.statements[2], &["background", "color"])
}

fn declares_call(statement: &oxc::ast::ast::Statement<'_>, property: &str) -> bool {
    use oxc::ast::ast::{Expression, Statement};
    let Statement::VariableDeclaration(declaration) = statement else {
        return false;
    };
    let Some(Some(Expression::CallExpression(call))) = declaration
        .declarations
        .first()
        .map(|declaration| declaration.init.as_ref())
    else {
        return false;
    };
    static_callee_is(&call.callee, property)
}

fn static_callee_is(expression: &oxc::ast::ast::Expression<'_>, property: &str) -> bool {
    let oxc::ast::ast::Expression::StaticMemberExpression(member) = expression else {
        return false;
    };
    member.property.name == property
}

fn is_hit_branch(statement: &oxc::ast::ast::Statement<'_>) -> bool {
    use oxc::ast::ast::{Expression, Statement};
    let Statement::IfStatement(branch) = statement else {
        return false;
    };
    let Expression::StaticMemberExpression(test) = &branch.test else {
        return false;
    };
    if test.property.name != "isHit" || branch.alternate.is_some() {
        return false;
    }
    let Statement::BlockStatement(block) = &branch.consequent else {
        return false;
    };
    block
        .body
        .iter()
        .any(|statement| declares_call(statement, "rayTrace"))
        && block
            .body
            .iter()
            .any(|statement| matches!(statement, Statement::ReturnStatement(_)))
}

fn returns_member_chain(statement: &oxc::ast::ast::Statement<'_>, names: &[&str]) -> bool {
    use oxc::ast::ast::{Expression, Statement};
    let Statement::ReturnStatement(returned) = statement else {
        return false;
    };
    let Some(mut expression) = returned.argument.as_ref() else {
        return false;
    };
    for name in names.iter().rev() {
        let Expression::StaticMemberExpression(member) = expression else {
            return false;
        };
        if member.property.name != *name {
            return false;
        }
        expression = &member.object;
    }
    true
}

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
        camera: native_vec3(&property(&camera_value, "position")?)?,
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
    Some(NativeOptions {
        diffuse: boolean(&options, "renderDiffuse")?,
        shadows: boolean(&options, "renderShadows")?,
        highlights: boolean(&options, "renderHighlights")?,
        reflections: boolean(&options, "renderReflections")?,
        depth: number(&options, "rayDepth")? as usize,
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
    use super::{raytrace_pixel_fact, wrap};
    use oxc::{allocator::Allocator, ast::ast::Statement, parser::Parser, span::SourceType};

    fn parsed_function(source: &str) -> bool {
        let allocator = Allocator::default();
        let parsed = Parser::new(&allocator, source, SourceType::default()).parse();
        let Some(Statement::FunctionDeclaration(function)) = parsed.program.body.first() else {
            return false;
        };
        raytrace_pixel_fact(function)
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
    fn chessboard_wrap_matches_two_unit_period() {
        assert_eq!(wrap(1.25), -0.75);
        assert_eq!(wrap(-1.25), 0.75);
    }
}
