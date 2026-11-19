//! TypeScript is a source language of the engine: the compiler admits the typed tree
//! (`admit`) and keeps its declarations. A declared type is no run-time proof (TypeScript
//! is unsound), so tiers check it once where a value enters typed code, then trust it
//! (`Function::declared_parameters`).

use oxc_ast::ast::*;

use crate::typed_ir::language_bit;

mod admit;
mod checked;
mod enum_folding;
mod enums;
mod fields;
mod imports;
mod names;
mod signatures;
pub(super) use imports::{elide as elide_type_only_imports, type_only_imports};
pub(super) use admit::admit;
pub(super) use checked::{lookup as checked_parameters, parameter_sites};
pub use checked::{CheckedParameters, ParameterSite};
pub(super) use checked::register as register_checked_parameters;
pub(super) use names::{Named, TypeNames};

/// The language types (`crate::typed_ir::Type::language` bits) each formal parameter of
/// `formals` declares, by argument index; empty where none declares one. An undeclared or
/// structural type (an interface, a type literal, a generic) is `DECLARED_ANY`, as a
/// structural type admits primitives too. An optional parameter or one with a default may
/// enter as `undefined`. A checker's answer for the same parameters (`checked`) narrows each
/// set further: both are seeds of the same fact, so a value must fit both.
pub(super) fn declared_parameters(formals: &FormalParameters<'_>, checked: Option<&CheckedParameters>) -> Vec<u16> {
    let checked = checked.and_then(|checked| checked.get(formals.span.start))
        .filter(|languages| languages.len() == formals.items.len());
    if checked.is_none() && formals.items.iter().all(|item| item.type_annotation.is_none()) {
        return Vec::new();
    }
    let parameters: Vec<u16> = formals.items.iter().enumerate().map(|(index, item)| {
        let declared = item.type_annotation.as_ref()
            .map_or(DECLARED_ANY, |annotation| declared_language(&annotation.type_annotation));
        let checked = checked.map_or(DECLARED_ANY, |languages| languages[index]);
        // A checker answers the family bit (`OBJECT`) only; it admits an exact plain object or array.
        let checked = if checked & bits::OBJECT != 0 { checked | bits::OBJECT_PLAIN | bits::ARRAY } else { checked };
        let set = if declared & checked == 0 { DECLARED_ANY } else { declared & checked };
        if set != DECLARED_ANY && (item.optional || item.initializer.is_some()) {
            set | language_bit(crate::ValueType::Undefined)
        } else {
            set
        }
    }).collect();
    if parameters.iter().all(|&set| set == DECLARED_ANY) { Vec::new() } else { parameters }
}

/// No declared fact: every language type (`typed_ir::ANY_LANGUAGES`, the bits of `Type::ANY`).
pub(crate) const DECLARED_ANY: u16 = crate::typed_ir::ANY_LANGUAGES;

/// The language types a TypeScript type admits at run time, `DECLARED_ANY` where it does not
/// name a closed set. A program's own type names are not known here (`declared_language_in`).
fn declared_language(ty: &TSType<'_>) -> u16 {
    declared_language_in(ty, &TypeNames::default())
}

/// The language types a TypeScript type admits at run time, with the program's type-level names
/// (`TypeNames`) resolved.
pub(super) fn declared_language_in(ty: &TSType<'_>, names: &TypeNames<'_>) -> u16 {
    languages(ty, names, 0)
}

/// How many alias expansions a name may take before it is open (a cycle is cut here).
const MAX_ALIAS_DEPTH: u8 = 8;

fn languages(ty: &TSType<'_>, names: &TypeNames<'_>, depth: u8) -> u16 {
    use crate::ValueType as V;
    match ty {
        TSType::TSNumberKeyword(_) => language_bit(V::Number),
        TSType::TSStringKeyword(_) | TSType::TSTemplateLiteralType(_) => language_bit(V::String),
        TSType::TSBooleanKeyword(_) => language_bit(V::Boolean),
        TSType::TSBigIntKeyword(_) => language_bit(V::BigInt),
        TSType::TSSymbolKeyword(_) => language_bit(V::Symbol),
        TSType::TSUndefinedKeyword(_) | TSType::TSVoidKeyword(_) => language_bit(V::Undefined),
        TSType::TSNullKeyword(_) => language_bit(V::Null),
        TSType::TSObjectKeyword(_) => language_bit(V::Object) | language_bit(V::Function),
        TSType::TSArrayType(_) | TSType::TSTupleType(_) => bits::ARRAY,
        TSType::TSTypeReference(reference) => match &reference.type_name {
            TSTypeName::IdentifierReference(name) if matches!(name.name.as_str(), "Array" | "ReadonlyArray") => bits::ARRAY,
            _ => DECLARED_ANY,
        },
        TSType::TSTypeOperatorType(operator) if matches!(operator.operator, TSTypeOperatorOperator::Readonly) =>
            declared_language(&operator.type_annotation),
        // A literal of properties and methods is a plain object's type. A call or construct signature
        // admits functions, and an empty literal admits every primitive: neither names the plain set.
        TSType::TSTypeLiteral(literal) if !literal.members.is_empty() && literal.members.iter().all(|member|
            !matches!(member, TSSignature::TSCallSignatureDeclaration(_) | TSSignature::TSConstructSignatureDeclaration(_))) => bits::OBJECT_PLAIN,
        TSType::TSFunctionType(_) | TSType::TSConstructorType(_) => language_bit(V::Function),
        TSType::TSLiteralType(literal) => match &literal.literal {
            TSLiteral::NumericLiteral(_) => language_bit(V::Number),
            TSLiteral::StringLiteral(_) | TSLiteral::TemplateLiteral(_) => language_bit(V::String),
            TSLiteral::BooleanLiteral(_) => language_bit(V::Boolean),
            TSLiteral::BigIntLiteral(_) => language_bit(V::BigInt),
            TSLiteral::UnaryExpression(_) => language_bit(V::Number) | language_bit(V::BigInt),
        },
        TSType::TSParenthesizedType(inner) => languages(&inner.type_annotation, names, depth),
        TSType::TSUnionType(union) => union.types.iter().fold(0, |set, member| set | languages(member, names, depth)),
        TSType::TSTypeReference(reference) => reference_languages(reference, names, depth),
        _ => DECLARED_ANY,
    }
}

/// The language types a named type denotes: what the program declares it to be (an alias's
/// expansion, an interface's object or function, a class's object, an enum's values), else the
/// builtin global of that name.
fn reference_languages(reference: &TSTypeReference<'_>, names: &TypeNames<'_>, depth: u8) -> u16 {
    use crate::ValueType as V;
    let TSTypeName::IdentifierReference(identifier) = &reference.type_name else { return DECLARED_ANY };
    match names.get(identifier.name.as_str()) {
        Some(Named::Alias { annotation, open }) => {
            if open || reference.type_arguments.is_some() || depth >= MAX_ALIAS_DEPTH {
                DECLARED_ANY
            } else {
                languages(annotation, names, depth + 1)
            }
        }
        Some(Named::Interface) => language_bit(V::Object) | language_bit(V::Function),
        Some(Named::Class(_)) => language_bit(V::Object),
        Some(Named::Enum(bits)) => bits,
        Some(Named::Unknown) => DECLARED_ANY,
        None => builtin_languages(identifier.name.as_str()),
    }
}

/// The language types of a builtin global type the program does not declare: the object
/// globals, `Function` for callables. `String`, `Number` and `Boolean` admit primitives and
/// boxes alike, and `Object` admits anything, so they stay `DECLARED_ANY`.
fn builtin_languages(name: &str) -> u16 {
    use crate::ValueType as V;
    match name {
        "Function" => language_bit(V::Function),
        "Array" | "ReadonlyArray" | "Map" | "Set" | "WeakMap" | "WeakSet" | "Date" | "RegExp" | "Promise"
        | "Error" | "Uint8Array" | "Uint8ClampedArray" | "Uint16Array" | "Int16Array" | "Uint32Array"
        | "Int32Array" | "Float32Array" | "Float64Array" | "ArrayBuffer" | "DataView" => language_bit(V::Object),
        _ => DECLARED_ANY,
    }
}

/// The language types of the elements an array type holds: `T[]`, `readonly T[]`, `Array<T>` and
/// `ReadonlyArray<T>` hold `T`s. Any other type, or an array of an open type, is `DECLARED_ANY`.
pub(super) fn element_languages(ty: &TSType<'_>, names: &TypeNames<'_>) -> u16 {
    match ty {
        TSType::TSArrayType(array) => declared_language_in(&array.element_type, names),
        TSType::TSTypeOperatorType(operator) if operator.operator == TSTypeOperatorOperator::Readonly => {
            element_languages(&operator.type_annotation, names)
        }
        TSType::TSTypeReference(reference) => match (&reference.type_name, reference.type_arguments.as_deref()) {
            (TSTypeName::IdentifierReference(name), Some(arguments))
                if matches!(name.name.as_str(), "Array" | "ReadonlyArray") && arguments.params.len() == 1 =>
            {
                declared_language_in(&arguments.params[0], names)
            }
            _ => DECLARED_ANY,
        },
        _ => DECLARED_ANY,
    }
}

#[cfg(test)]
mod tests {
    use crate::{Engine, ExecutionMode, Runtime, bytecode::Op, test_host::TestHost};

    #[test]
    fn typescript_annotations_and_assertions_do_not_override_runtime_values() {
        let source = r#"
            type Box<T> = { value: T };
            interface Item { readonly value: number }
            function identity<T>(value: T): T { return value; }
            function add(value: number): number { return value + 1; }
            const item: Item = { value: 7 };
            (item as any).value = 'changed';
            for (let i = 0; i < 256; i++) add(i);
            console.log(item.value + 1);
            console.log(add(('text' as unknown) as number));
            console.log(identity<string>('typed'));
            console.log((5 satisfies number) + 1);
        "#;
        for mode in [ExecutionMode::Interpreted, ExecutionMode::Compiled] {
            let program = Engine::specialize_typescript(source, "typed.cts").unwrap();
            let bytes: crate::KeptBytes = std::rc::Rc::new(program.encode_trusted());
            let program = crate::ResidualProgram::decode_trusted(bytes).unwrap();
            let host = TestHost::default();
            let view = host.clone();
            let mut runtime = Runtime::new(host);
            runtime.set_execution_mode(mode);
            runtime.execute(&program).unwrap();
            assert_eq!(&*view.lines(), &["changed1", "text1", "typed", "6"]);
        }
    }

    #[test]
    fn type_only_wrappers_keep_references_and_names() {
        // TypeScript erases `!`, `as`, `satisfies` and `<T>` at run time: a call through one
        // keeps `this`, `typeof` through one tolerates a missing name, and an anonymous
        // function under one takes its binding's name.
        let source = r#"
            const obj = { v: 7, m() { return this?.v; } };
            console.log(obj.m!(), (obj.m as any)(), (<any>obj.m)(), (obj.m satisfies Function)(), obj['m']!());
            console.log(typeof (missingName as any), typeof missingName!, typeof (<any>missingName));
            const f = (() => 1) as () => number;
            const g = (function () {}) satisfies Function;
            console.log(f.name, g.name);
        "#;
        for mode in [ExecutionMode::Interpreted, ExecutionMode::Compiled] {
            let program = Engine::specialize_typescript(source, "wrappers.cts").unwrap();
            let host = TestHost::default();
            let view = host.clone();
            let mut runtime = Runtime::new(host);
            runtime.set_execution_mode(mode);
            runtime.execute(&program).unwrap();
            assert_eq!(&*view.lines(), &["7 7 7 7 7", "undefined undefined undefined", "f g"]);
        }
    }

    #[test]
    fn enums_run_as_typescript_defines_them() {
        let source = r#"
            enum Color { Red, Green = 5, Blue }
            enum S { A = "a", C = A }
            enum Bits { R = 1 << 0, W = 1 << 1, RW = R | W, Not = ~W }
            const base = 10;
            enum Dyn { X = base * 2, Y = X + 1 }
            enum Color { Black = 100 }
            declare enum Gone { Q }
            const enum K { One = 1, Two }
            function f() { enum Local { P, Q } return Local.Q + Local[0]; }
            console.log(JSON.stringify(Color), JSON.stringify(S), JSON.stringify(Bits));
            console.log(JSON.stringify(Dyn), K.Two, f(), typeof Gone);
        "#;
        for mode in [ExecutionMode::Interpreted, ExecutionMode::Compiled] {
            let program = Engine::specialize_typescript(source, "enums.cts").unwrap();
            let host = TestHost::default();
            let view = host.clone();
            let mut runtime = Runtime::new(host);
            runtime.set_execution_mode(mode);
            runtime.execute(&program).unwrap();
            assert_eq!(&*view.lines(), &[
                r#"{"0":"Red","5":"Green","6":"Blue","100":"Black","Red":0,"Green":5,"Blue":6,"Black":100} {"A":"a","C":"a"} {"1":"R","2":"W","3":"RW","R":1,"W":2,"RW":3,"Not":-3,"-3":"Not"}"#,
                r#"{"20":"X","21":"Y","X":20,"Y":21} 2 1P undefined"#,
            ]);
        }
        // TypeScript rejects a member without initializer after a computed one.
        assert!(Engine::specialize_typescript("const b = 1; enum E { A = b, B }", "enum-error.cts").is_err());
    }

    #[test]
    fn declared_parameter_types_are_kept_as_facts() {
        let source = "function add(a: number, c: Shape, d: number | bigint, e, b?: string) { return a; }\nconst f = (x: boolean) => x;\nfunction g(p, q) { return p; }";
        let program = Engine::specialize_typescript(source, "declared.cts").unwrap();
        let declared = |name: &str| (0..program.functions.len()).filter_map(|index| program.function_get(index))
            .find(|function| function.name.is_some_and(|atom| program.atoms.get(atom as usize) == Some(name)))
            .map(|function| function.declared_parameters.to_vec());
        use crate::ValueType as V;
        let bit = |kind: V| crate::typed_ir::Languages::of(kind).bits();
        assert_eq!(declared("add").unwrap(), vec![bit(V::Number), super::DECLARED_ANY,
            bit(V::Number) | bit(V::BigInt), super::DECLARED_ANY, bit(V::String) | bit(V::Undefined)]);
        assert_eq!(declared("g").unwrap(), Vec::<u16>::new());
    }

    #[test]
    fn checked_parameter_types_narrow_the_declared_facts() {
        let source = "type Meters = number;\nfunction move(by: Meters, label: string, extra) { return by; }\nconst twice = [1, 2].map(x => x * 2);\nfunction plain(p) { return p; }";
        let sites = Engine::typescript_parameter_sites(source).unwrap();
        let start = |needle: &str| source.find(needle).unwrap() as u32;
        assert_eq!(sites, vec![
            super::ParameterSite { formals_start: start("(by"), parameters: vec![start("by"), start("label"), start("extra")] },
            super::ParameterSite { formals_start: start("x =>"), parameters: vec![start("x =>")] },
            super::ParameterSite { formals_start: start("(p)"), parameters: vec![start("p)")] },
        ]);
        use crate::ValueType as V;
        let bit = |kind: V| crate::typed_ir::Languages::of(kind).bits();
        // What a checker answers: the alias is a number, the callback parameter is a number,
        // and its `string` answer for `label` agrees with the annotation.
        let mut checked = super::CheckedParameters::default();
        checked.insert(start("(by"), vec![bit(V::Number), bit(V::String), super::DECLARED_ANY].into());
        checked.insert(start("x =>"), vec![bit(V::Number)].into());
        checked.insert(start("(p)"), vec![bit(V::Object) | bit(V::Function)].into());
        Engine::register_checked_typescript_parameters(source, checked);
        let program = Engine::specialize_typescript(source, "checked.cts").unwrap();
        let declared = |name: Option<&str>| (0..program.functions.len()).filter_map(|index| program.function_get(index))
            .find(|function| function.name.map(|atom| program.atoms.get(atom as usize).map(|text| &*text)) == name.map(Some)
                && (name.is_some() || function.is_arrow))
            .map(|function| function.declared_parameters.to_vec());
        assert_eq!(declared(Some("move")).unwrap(), vec![bit(V::Number), bit(V::String), super::DECLARED_ANY]);
        assert_eq!(declared(Some("plain")).unwrap(), vec![bit(V::Object) | bit(V::Function)]);
        assert_eq!(declared(None).unwrap(), vec![bit(V::Number)]);
        // Another text compiles without them.
        let other = Engine::specialize_typescript(&format!("{source}\n"), "checked.cts").unwrap();
        assert!(other.functions.iter().enumerate().filter_map(|(index, _)| other.function_get(index))
            .filter(|function| function.is_arrow).all(|function| function.declared_parameters.is_empty()));
    }

    /// The declared return languages of the function `f` in `source` (`DECLARED_ANY` if undeclared).
    fn declared_return(source: &str) -> u16 {
        let program = Engine::specialize_typescript(source, "names.cts").unwrap();
        let declared = program.declared_types.as_deref().expect("a typed program keeps its declarations");
        let start = source.find("function f").unwrap() as u32;
        declared.declared_signature(start).map_or(super::DECLARED_ANY, |signature| signature.returns)
    }

    #[test]
    fn named_types_declare_their_language_sets() {
        use crate::ValueType as V;
        let bit = |kind: V| crate::typed_ir::Languages::of(kind).bits();
        assert_eq!(declared_return("type N = number;\nfunction f(): N { return 1; }"), bit(V::Number));
        assert_eq!(declared_return("type N = number;\ntype M = N;\nfunction f(): M { return 1; }"), bit(V::Number));
        assert_eq!(declared_return("interface I { x: number }\nfunction f(): I { return { x: 1 }; }"),
            bit(V::Object) | bit(V::Function));
        assert_eq!(declared_return("interface I { x: number }\ninterface I { y: string }\nfunction f(): I { return { x: 1, y: '' }; }"),
            bit(V::Object) | bit(V::Function));
        assert_eq!(declared_return("enum E { A, B }\nfunction f(): E { return E.A; }"), bit(V::Number));
        assert_eq!(declared_return("enum E { A = 'a', B = 'b' }\nfunction f(): E { return E.A; }"), bit(V::String));
        assert_eq!(declared_return("enum E { A = 'a', B = 2 }\nfunction f(): E { return E.B; }"),
            bit(V::Number) | bit(V::String));
        assert_eq!(declared_return("function f(): Map<string, number> { return new Map(); }"), bit(V::Object));
        assert_eq!(declared_return("function f(): Function { return () => 1; }"), bit(V::Function));
        assert_eq!(declared_return("function f(): String { return 'x'; }"), super::DECLARED_ANY);
    }

    #[test]
    fn duplicate_and_shadowed_type_names_stay_open() {
        let open = super::DECLARED_ANY;
        assert_eq!(declared_return("class D {}\ninterface D {}\nfunction f(): D { return null as any; }"), open);
        assert_eq!(declared_return("type N = number;\nfunction f<N>(x: N): N { return x; }"), open);
        assert_eq!(declared_return("type Box<T> = T;\nfunction f(): Box<number> { return 1; }"), open);
        assert_eq!(declared_return("type A = B;\ntype B = A;\nfunction f(): A { return 1 as any; }"), open);
        assert_eq!(declared_return("type Map = number;\nfunction f(): Map { return 1; }"),
            crate::typed_ir::Languages::of(crate::ValueType::Number).bits());
    }

    #[test]
    fn javascript_parse_goal_still_rejects_type_annotations() {
        assert!(Engine::specialize("const value: number = 1;", "named.cts").is_err());
    }

    fn run_typescript(source: &str) -> Vec<String> {
        let mut outputs = Vec::new();
        for mode in [ExecutionMode::Interpreted, ExecutionMode::Compiled] {
            let program = Engine::specialize_typescript(source, "admitted.cts").unwrap();
            let host = TestHost::default();
            let view = host.clone();
            let mut runtime = Runtime::new(host);
            runtime.set_execution_mode(mode);
            runtime.execute(&program).unwrap();
            outputs.push(view.lines().join("|"));
        }
        assert_eq!(outputs[0], outputs[1]);
        outputs.swap_remove(0).split('|').map(str::to_owned).collect()
    }

    #[test]
    fn declarations_without_runtime_meaning_are_admitted() {
        let lines = run_typescript(r#"
            interface Point { x: number }
            type Alias = Point | string;
            declare const ambient: number;
            declare function external(): void;
            declare namespace Types { const t: number; }
            function pick(value: string): string;
            function pick(value: number): number;
            function pick(value: any) { return value; }
            abstract class Base {
                abstract area(): number;
                abstract readonly name: string;
                declare tag: string;
                [key: string]: unknown;
                describe() { return this.area(); }
            }
            class Square extends Base { name = "sq"; constructor(private side: number) { super(); } area(): number; area() { return this.side ** 2; } }
            function local() { interface Inner { y: number } type T = number; const v: T = 2; return v; }
            const box: { v?: number } = { v: 1 };
            let count: number | undefined = 1;
            (box as any).v = 2; box.v!++; count! += 4; (<number>count) *= 2;
            console.log([pick(1), pick("s"), new Square(3).describe(), local(), "tag" in new Square(1), box.v, count].join(" "));
        "#);
        assert_eq!(lines, ["1 s 9 2 false 3 10"]);
    }

    #[test]
    fn enums_namespaces_and_parameter_properties_have_their_runtime_meaning() {
        let lines = run_typescript(r#"
            enum E { A, B, C = A + 10, D = "d" }
            const enum Flags { X = 1 << 3, Y = X | 1 }
            function shadow(Flags: { Y: number }) { return Flags.Y; }
            enum Computed { Len = "abc".length, Next }
            enum E { Late = 20 }
            namespace N { export const k = 1; export function g() { return k + 1; } export namespace M { export enum Q { R } } }
            namespace N { export const k2 = 2; }
            namespace A.B.C { export const deep = "deep"; }
            class P { constructor(public x: number, protected readonly y = 2, z?: number) {} sum() { return this.x + this.y; } }
            class Q extends P { extra = 5; constructor(x: number, private w: string) { super(x); } }
            const q = new Q(1, "w");
            import alias = N.M;
            console.log([E.A, E[1], E.C, E[10], E.D, E.Late, E[20], Flags.Y, Computed.Len, Computed.Next, Computed[4]].join(" "));
            console.log([N.k, N.g(), N.M.Q.R, N.M.Q[0], N.k2, A.B.C.deep, alias.Q.R, shadow({ Y: 7 })].join(" "));
            console.log([new P(3).sum(), q.sum(), Object.keys(q).join()].join(" "));
        "#);
        assert_eq!(lines, [
            "0 B 10 C d 20 Late 9 3 4 Next",
            "1 2 0 R 2 deep 0 7",
            "5 3 x,y,extra,w",
        ]);
    }

    #[test]
    fn the_declared_environment_is_kept_with_the_program() {
        use crate::bytecode::{DeclaredKind, EnumConstant};
        let source = "interface Point { x: number }\ntype Id = string;\nfunction f(a: string): void;\nfunction f(a: any) {}\nenum E { A = 2, B, C = \"c\" }\nabstract class K { abstract m(): void; }";
        let program = Engine::specialize_typescript(source, "declared.cts").unwrap();
        let bytes: crate::KeptBytes = std::rc::Rc::new(program.encode_trusted());
        let program = crate::ResidualProgram::decode_trusted(bytes).unwrap();
        let declared = program.declared_types.as_deref().expect("a typed program keeps its declarations");
        let kinds: Vec<(DeclaredKind, Option<&str>)> = declared.declared.iter().map(|entry| (entry.kind, entry.name.as_deref())).collect();
        assert_eq!(kinds, [
            (DeclaredKind::Interface, Some("Point")),
            (DeclaredKind::TypeAlias, Some("Id")),
            (DeclaredKind::FunctionSignature, Some("f")),
            (DeclaredKind::Enum, Some("E")),
            (DeclaredKind::ClassMember, Some("m")),
        ]);
        let point = &declared.declared[0];
        assert_eq!(&source[point.start as usize..point.end as usize], "interface Point { x: number }");
        assert_eq!(declared.enum_constant("E", "B"), Some(&EnumConstant::Number(3.0)));
        assert_eq!(declared.enum_constant("E", "C"), Some(&EnumConstant::String("c".into())));
        assert!(Engine::specialize("const plain = 1;", "plain.cjs").unwrap().declared_types.is_none());
    }

    #[test]
    fn const_enum_reads_are_their_constants() {
        let lines = run_typescript("const enum Dir { Up = 1, Down = Up * 2, Name = \"dir\" }\nconst read = () => Dir.Down + Dir[\"Up\"];\nconsole.log(read() + Dir.Name + read.toString().includes(\"Dir\"));");
        assert_eq!(lines, ["3dirtrue"]);
    }

    /// The opcodes of the function named `name` in a compiled program.
    fn function_ops(program: &crate::ResidualProgram, name: &str) -> Vec<Op> {
        let function = (0..program.functions.len()).filter_map(|index| program.function_get(index))
            .find(|function| function.name.is_some_and(|atom| program.atoms.get(atom as usize) == Some(name)))
            .unwrap();
        function.instructions().map(|instruction| instruction.op()).collect()
    }

    #[test]
    fn plain_enum_reads_fold_to_their_constants() {
        let source = "function run() { enum Color { Red, Green = 5, Name = \"n\" } let total = 0; for (let i = 0; i < 3; i++) total += Color.Green; return total + Color.Name; }\nconsole.log(run());";
        let program = Engine::specialize_typescript(source, "plain-enum.cts").unwrap();
        assert!(!function_ops(&program, "run").contains(&Op::GetField));
        assert_eq!(run_typescript(source), ["15n"]);
    }

    #[test]
    fn plain_enum_reads_stay_live_when_something_can_change_them() {
        let source = r#"
            function written() {
                enum Color { Red, Green = 5 }
                for (let i = 0; i < 2; i++) (Color as any).Green = i + 7;
                return Color.Green;
            }
            function passed() {
                enum Color { Green = 5 }
                const show = (value: any) => value.Green;
                let total = 0;
                for (let i = 0; i < 2; i++) total += show(Color) + Color.Green;
                return total;
            }
            console.log([written(), passed()].join(" "));
        "#;
        let control = "function w() { enum Color { Green = 5 } (Color as any).Green = 7; return Color.Green; }";
        let program = Engine::specialize_typescript(control, "written.cts").unwrap();
        assert!(function_ops(&program, "w").contains(&Op::GetField));
        assert_eq!(run_typescript(source), ["8 20"]);
    }

    #[test]
    fn exported_plain_enums_stay_live() {
        let source = "export enum Color { Green = 5 }\n(Color as any).Green = 9;\nconsole.log(Color.Green);";
        let program = Engine::specialize_typescript_module(source, "exported-enum.mts").unwrap();
        let host = TestHost::default();
        let view = host.clone();
        let mut runtime = Runtime::new(host);
        runtime.execute(&program).unwrap();
        assert_eq!(&*view.lines(), &["9"]);
    }

    #[test]
    fn decorators_have_typescripts_meaning_and_metadata() {
        let source = r#"
const log: string[] = [];
const meta: string[] = [];
const named = (v: any): string => Array.isArray(v) ? v.map(named).join("/") : v === undefined ? "u" : v.name;
(Reflect as any).metadata = (k: string, v: any) => (target: any, key?: any) => { meta.push(k + ":" + (key ?? "class") + ":" + named(v)); };
function tag(name: string) { log.push("eval " + name); return (target: any, key?: any, desc?: any) => { log.push("apply " + name + " " + String(key)); }; }
function sealed<T extends Function>(c: T): any { log.push("class " + c.name); return class extends (c as any) { extra = 1 }; }
function inject(token: string) { return (target: any, key: any, index: number) => { log.push("param " + token + " " + index + " " + String(key)); }; }
function upper(target: any, key: string, desc: PropertyDescriptor) { const f = desc.value; return { ...desc, value: function (this: any, ...a: any[]) { return String(f.apply(this, a)).toUpperCase(); } }; }
class Dep {}
interface Shape {}
@sealed
class Service {
    @tag("field") name: string = "svc";
    constructor(@inject("dep") private dep: Dep, count: number) {}
    @tag("a") @tag("b") method(x: number, s: Shape): string { return "hi " + x; }
    @upper greet(n: string) { return "hello " + n; }
    @tag("static") static make(): Service { return new Service(new Dep(), 1); }
}
const s = new Service(new Dep(), 2);
console.log([log.join(","), s.greet("x"), (s as any).extra, s.name, (s as any).dep instanceof Dep, meta.join(",")].join(" ; "));
"#;
        assert_eq!(run_typescript(source), vec![[
            "eval field,apply field name,eval a,eval b,apply b method,apply a method,eval static,apply static make,param dep 0 undefined,class Service",
            "HELLO X", "1", "svc", "true",
            "design:type:name:String,design:returntype:method:String,design:paramtypes:method:Number/Object,design:type:method:Function,\
design:returntype:greet:u,design:paramtypes:greet:String,design:type:greet:Function,\
design:returntype:make:Service,design:paramtypes:make:,design:type:make:Function,design:paramtypes:class:Dep/Number",
        ].join(" ; ")]);
    }

    #[test]
    fn decorators_without_a_static_key_still_need_a_runtime_meaning() {
        for source in ["const Box = @sealed class {}", "class Box { @log [key]() {} }", "class Box { @log #hidden() {} }"] {
            assert!(Engine::specialize_typescript(source, "decorated.cts").is_err(), "{source}");
        }
    }
}

#[cfg(test)]
mod module_tests {
    use crate::Engine;

    #[test]
    fn typed_module_analysis_does_not_share_javascript_cache_entries() {
        let source = "export type Shape = number; export const value: number = 1;";
        for typed_first in [false, true] {
            let name = if typed_first { "typed-first" } else { "javascript-first" };
            for typescript in [typed_first, !typed_first, typed_first] {
                let analysis = Engine::static_module_analysis_with(source, name, typescript, |analyze| analyze());
                assert_eq!(analysis.early_error.is_none(), typescript);
                assert_eq!(analysis.plan.is_some(), typescript);
            }
        }
    }

    #[test]
    fn type_only_module_syntax_does_not_make_a_module() {
        for (source, module) in [
            ("export interface Café { value: '🙂' }\u{2028}export type Alias = string;\r\nconst value: number = 1;", false),
            ("import type { Shape } from './side.mts'; import fs = require('fs'); export = fs;", false),
            ("import { type Shape } from './side.mts';", true),
            ("export { type Shape } from './side.mts';", true),
            ("export const value: number = 1;", true),
        ] {
            assert_eq!(Engine::typescript_has_module_syntax(source), Some(module), "{source}");
        }
        assert!(Engine::typescript_has_module_syntax("export interface {").is_none());
    }

    #[cfg(feature = "aot")]
    #[test]
    fn native_module_key_keeps_language_in_its_compile_goal() {
        let source = "export const value: number = 1;";
        let mut compiler = crate::NativeJsCompiler::new(true);
        let javascript = crate::NativeSourceKey::new("same-name", &["specialized", crate::STATIC_MODULE_GOAL, source]).unwrap();
        let typed = crate::NativeSourceKey::new("same-name", &["specialized", crate::STATIC_TYPESCRIPT_MODULE_GOAL, source]).unwrap();
        assert!(compiler.compile_source(&javascript).is_err());
        assert!(compiler.compile_source(&typed).is_ok());
    }
}
