//! Declared class fields: the language types each class field's annotation admits, kept with
//! the program's declared environment (`DeclaredTypes::fields`). A declaration is a seed, not a
//! proof (TypeScript is unsound), so a tier checks it once where a value enters and trusts it.

use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};

use crate::bytecode::DeclaredTypes;
use crate::typed_ir::language_bit;
use super::{DECLARED_ANY, TypeNames, declared_language_in, element_languages};

/// Record each annotated instance and static field of every class in `program`, and each
/// annotated parameter property of a constructor (an instance field named by its parameter).
/// Run before admission, so the modifiers and signature-only members are still in place.
pub(super) fn collect<'a>(program: &Program<'a>, names: &TypeNames<'_>, declarations: &mut DeclaredTypes) {
    Collect { declarations, names }.visit_program(program);
}

struct Collect<'d> {
    declarations: &'d mut DeclaredTypes,
    names: &'d TypeNames<'d>,
}

impl<'a> Visit<'a> for Collect<'_> {
    fn visit_class(&mut self, class: &Class<'a>) {
        // The class's span start: the explicit or implicit constructor's `Function` source span
        // starts at the same offset (`compile::class`), so the object model can key fields by it.
        let class_start = class.span.start;
        for element in &class.body.body {
            match element {
                ClassElement::PropertyDefinition(property) => self.property(class_start, property),
                ClassElement::MethodDefinition(method) if method.kind.is_constructor() => {
                    for parameter in &method.value.params.items {
                        self.parameter_property(class_start, parameter);
                    }
                }
                _ => {}
            }
        }
        walk::walk_class(self, class);
    }
}

impl Collect<'_> {
    /// A class property with a non-computed identifier key and a type annotation. Private
    /// `#names`, computed keys, `declare` and abstract fields are not recorded: the first two
    /// have no name the object model reads, the last two have no run-time slot of their own.
    /// An optional field admits `undefined`.
    fn property(&mut self, class_start: u32, property: &PropertyDefinition<'_>) {
        if property.computed || property.declare || property.r#type == PropertyDefinitionType::TSAbstractPropertyDefinition {
            return;
        }
        let PropertyKey::StaticIdentifier(identifier) = &property.key else { return };
        let Some(annotation) = &property.type_annotation else { return };
        let mut languages = declared_language_in(&annotation.type_annotation, self.names);
        let elements = element_languages(&annotation.type_annotation, self.names);
        if property.optional {
            languages |= language_bit(crate::ValueType::Undefined);
        }
        if languages != DECLARED_ANY || elements != DECLARED_ANY {
            self.declarations.declare_field(class_start, identifier.name.as_str(), languages, elements, property.r#static);
        }
    }

    /// A constructor parameter with an accessibility, `readonly` or `override` modifier: a
    /// parameter property, which declares an instance field of its name.
    fn parameter_property(&mut self, class_start: u32, parameter: &FormalParameter<'_>) {
        if parameter.accessibility.is_none() && !parameter.readonly && !parameter.r#override {
            return;
        }
        let (Some(name), Some(annotation)) = (parameter.pattern.get_identifier_name(), &parameter.type_annotation) else { return };
        let languages = declared_language_in(&annotation.type_annotation, self.names);
        let elements = element_languages(&annotation.type_annotation, self.names);
        if languages != DECLARED_ANY || elements != DECLARED_ANY {
            self.declarations.declare_field(class_start, &name.to_string(), languages, elements, false);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::bytecode::DeclaredTypes;
    use crate::{Engine, ValueType as V};

    fn fields_of(source: &str) -> Vec<(String, u16, bool)> {
        let program = Engine::specialize_typescript(source, "fields.cts").unwrap();
        let bytes: crate::KeptBytes = std::rc::Rc::new(program.encode_trusted());
        let program = crate::ResidualProgram::decode_trusted(bytes).unwrap();
        let declared = program.declared_types.as_deref().expect("a typed program keeps its declarations");
        declared.fields.iter().map(|field| (field.name.to_string(), field.languages, field.is_static)).collect()
    }

    #[test]
    fn class_fields_and_parameter_properties_are_declared_types() {
        let source = "class P { x: number; y: string | undefined; constructor(public z: boolean) {} }";
        let bit = |kind: V| crate::typed_ir::Languages::of(kind).bits();
        assert_eq!(fields_of(source), [
            ("x".to_string(), bit(V::Number), false),
            ("y".to_string(), bit(V::String) | bit(V::Undefined), false),
            ("z".to_string(), bit(V::Boolean), false),
        ]);
    }

    #[test]
    fn array_field_element_types_are_declared() {
        let source = "class P { v: Array<number> = []; w: readonly string[] = []; }";
        let program = Engine::specialize_typescript(source, "elements.cts").unwrap();
        let declared: &DeclaredTypes = program.declared_types.as_deref().unwrap();
        let p = source.find("class P").unwrap() as u32;
        assert_eq!(declared.declared_field_elements(p, "v"), Some(crate::typed_ir::Languages::of(V::Number).bits()));
        assert_eq!(declared.declared_field_elements(p, "w"), Some(crate::typed_ir::Languages::of(V::String).bits()));
        assert_eq!(declared.declared_field(p, "v"), None);
    }

    #[test]
    fn declared_field_lookup_reads_instance_fields_of_one_class() {
        let source = "class P { static s: number; x: number; }\nclass Q { x: string; }";
        let program = Engine::specialize_typescript(source, "lookup.cts").unwrap();
        let declared: &DeclaredTypes = program.declared_types.as_deref().unwrap();
        let p = source.find("class P").unwrap() as u32;
        let q = source.find("class Q").unwrap() as u32;
        assert_eq!(declared.declared_field(p, "x"), Some(crate::typed_ir::Languages::of(V::Number).bits()));
        assert_eq!(declared.declared_field(q, "x"), Some(crate::typed_ir::Languages::of(V::String).bits()));
        assert_eq!(declared.declared_field(p, "s"), None);
        assert_eq!(declared.declared_field(p, "missing"), None);
    }
}
