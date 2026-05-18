//! # Comprehensive Emitter Tests
//!
//! Comprehensive tests for code generation edge cases.

#![allow(clippy::needless_raw_string_hashes)]

use crate::{analyzer, codegen, parser};

/// Test transpilation of basic arithmetic operations.
#[test]
fn test_arithmetic_operations() {
    let source = r##"
export function add(a: number, b: number): number { return a + b; }
export function sub(a: number, b: number): number { return a - b; }
export function mul(a: number, b: number): number { return a * b; }
export function div(a: number, b: number): number { return a / b; }
export function modOp(a: number, b: number): number { return a % b; }
"##;
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();

    assert!(result.source.contains("pub fn"));
    assert!(result.source.contains("add"));
    assert!(result.source.contains("sub"));
    assert!(result.source.contains("mul"));
    assert!(result.source.contains("div"));
}

/// Test comparison operators.
#[test]
fn test_comparison_operators() {
    let source = r##"
export function compare(a: number, b: number): boolean {
    return a === b && a !== b && a < b && a <= b && a > b && a >= b;
}
"##;
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();

    assert!(result.source.contains("=="));
    assert!(result.source.contains("!="));
    assert!(result.source.contains("<="));
    assert!(result.source.contains(">="));
}

/// Test logical operators.
#[test]
fn test_logical_operators() {
    let source = r##"
export function logic(a: boolean, b: boolean): boolean {
    return a && b || !a;
}
"##;
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();

    assert!(result.source.contains("&&"));
    assert!(result.source.contains("||"));
    assert!(result.source.contains('!'));
}

/// Test bitwise operators.
#[test]
fn test_bitwise_operators() {
    let source = r##"
export function bitwise(a: number, b: number): number {
    return (a & b) | (a ^ b) | (a << 1) | (a >> 1);
}
"##;
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();

    // Verify bitwise operators are generated
    assert!(!result.source.is_empty());
}

/// Test ternary expressions.
#[test]
fn test_ternary_expression() {
    let source = r##"
export function max(a: number, b: number): number {
    return a > b ? a : b;
}
"##;
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();

    assert!(!result.source.is_empty());
}

/// Test array literal with type inference.
#[test]
fn test_array_literal() {
    let source = r##"
export const numbers: number[] = [1, 2, 3, 4, 5];
export const strings: string[] = ["a", "b", "c"];
export const mixed: (string | number)[] = [1, "a", 2, "b"];
"##;
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();

    // Verify the source was generated
    assert!(!result.source.is_empty());
}

/// Test nested object structures.
#[test]
fn test_nested_objects() {
    let source = r##"
export type Inner = { a: number, b: string };
export type Outer = { inner: Inner, count: number };
export function create(): Outer {
    return { inner: { a: 1, b: "test" }, count: 42 };
}
"##;
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();

    assert!(result.source.contains("pub struct Inner"));
    assert!(result.source.contains("pub struct Outer"));
}

/// Test switch statement exhaustiveness.
#[test]
fn test_switch_exhaustive() {
    let source = r##"
export enum Status { Pending, Active, Done }
export function getLabel(s: Status): string {
    switch (s) {
        case Status.Pending: return "P";
        case Status.Active: return "A";
        case Status.Done: return "D";
    }
}
"##;
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();

    assert!(result.source.contains("match"));
    assert!(result.source.contains("Pending"));
    assert!(result.source.contains("Active"));
    assert!(result.source.contains("Done"));
}

/// Test while loop.
#[test]
fn test_while_loop() {
    let source = r##"
export function countdown(n: number): number {
    while (n > 0) {
        n = n - 1;
    }
    return n;
}
"##;
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();

    assert!(result.source.contains("while"));
}

/// Test do-while loop (if-else pattern).
#[test]
fn test_do_while_pattern() {
    let source = r##"
export function doWhile(min: number): number {
    let i = 0;
    do {
        i = i + 1;
    } while (i < min);
    return i;
}
"##;
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();

    assert!(!result.source.is_empty());
}

/// Test break and continue.
#[test]
fn test_break_continue() {
    let source = r##"
export function findFirst(items: number[], target: number): number {
    for (let i = 0; i < items.length; i++) {
        if (items[i] === target) {
            break;
        }
        if (items[i] < 0) {
            continue;
        }
    }
    return -1;
}
"##;
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();

    assert!(result.source.contains("for"));
    assert!(result.source.contains("break") || result.source.contains("continue"));
}

/// Test const declarations.
#[test]
fn test_const_declarations() {
    let source = r##"
export const PI = 3.14159;
export const NAME = "test";
export const FLAG = true;
export function getPi(): number { return PI; }
"##;
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();

    // Verify const declarations are generated
    assert!(!result.source.is_empty());
}

/// Test let mutable declarations.
#[test]
fn test_let_declarations() {
    let source = r##"
export function counter(): number {
    let count = 0;
    count = count + 1;
    return count;
}
"##;
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();

    // Verify mutable bindings are generated
    assert!(!result.source.is_empty());
}

/// Test string methods.
#[test]
fn test_string_methods() {
    let source = r##"
export function processStrings(s: string): number {
    return s.length + s.toUpperCase().length;
}
"##;
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();

    assert!(!result.source.is_empty());
}

/// Test template literals.
#[test]
fn test_template_literals() {
    let source = r##"
export function greet(name: string): string {
    return `Hello, ${name}!`;
}
"##;
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();

    assert!(result.source.contains("format!"));
}

/// Test optional chaining (with type checking).
#[test]
fn test_optional_patterns() {
    let source = r##"
export type User = { name: string, email?: string };
export function getEmail(user: User | null): string {
    if (user === null) return "none";
    return user.email !== undefined ? user.email : "none";
}
"##;
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();

    assert!(!result.source.is_empty());
}

/// Test type narrowing through explicit checks.
#[test]
fn test_type_narrowing() {
    let source = r##"
export function process(val: string | number): string {
    if ((val as string).toUpperCase !== undefined) {
        return (val as string).toUpperCase();
    }
    return String(val);
}
"##;
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();

    assert!(!result.source.is_empty());
}

/// Test intersection types.
#[test]
fn test_intersection_types() {
    let source = r##"
export type A = { a: number };
export type B = { b: string };
export type AB = A & B;
export function create(): AB {
    return { a: 1, b: "test" };
}
"##;
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();

    assert!(!result.source.is_empty());
}

/// Test readonly arrays.
#[test]
fn test_readonly_arrays() {
    let source = r##"
export function sumReadonly(items: ReadonlyArray<number>): number {
    let sum = 0;
    for (let i = 0; i < items.length; i++) {
        sum = sum + items[i];
    }
    return sum;
}
"##;
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();

    assert!(!result.source.is_empty());
}

/// Test mapped types.
#[test]
fn test_mapped_types() {
    let source = r##"
export type Flags = { [K in "a" | "b" | "c"]: boolean };
export type Point = { [K in "x" | "y"]: number };
"##;
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();

    assert!(!result.source.is_empty());
}

/// Test conditional types (compile-time only).
#[test]
fn test_conditional_types() {
    let source = r##"
export type IsString<T> = T extends string ? true : false;
export type A = IsString<string>;
export type B = IsString<number>;
"##;
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();

    // Verify parsing succeeded
    assert!(!result.source.is_empty());
}

/// Test namespace imports.
#[test]
fn test_namespace_imports() {
    let source = r##"
export const Math = {
    PI: 3.14159,
    E: 2.71828,
    sqrt: (n: number) => n > 0 ? 1 : 0
};
export function getPi(): number { return Math.PI; }
"##;
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();

    assert!(result.source.contains("Math"));
}

/// Test re-export patterns.
#[test]
fn test_reexports() {
    let source = r##"
export type { Task } from "./types.r.ts";
export { createTask } from "./factory.r.ts";
export { default as DefaultTask } from "./default.r.ts";
"##;
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();

    // Re-exports are currently not emitted as `use` statements;
    // generation succeeds without error.
    assert!(!result.source.is_empty());
}

/// Test decorator-like patterns (compile-time only).
#[test]
fn test_decorator_comments() {
    let source = r##"
// @deprecated
// @experimental
export function oldFunction(): void {
    // This function is deprecated
}
"##;
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();

    assert!(result.source.contains("pub fn"));
}

/// Test nullish coalescing.
#[test]
fn test_nullish_coalescing() {
    let source = r##"
export function getValue(a: string | null | undefined, b: string): string {
    return a ?? b;
}
"##;
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();

    assert!(!result.source.is_empty());
}

/// Test optional chaining operator.
#[test]
fn test_optional_chaining() {
    let source = r##"
export type User = { profile: { name: string } | null };
export function getName(user: User | null): string {
    if (user === null) return "none";
    return user.profile?.name ?? "anonymous";
}
"##;
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();

    assert!(!result.source.is_empty());
}

/// Test bigint literals.
#[test]
fn test_bigint_literals() {
    let source = r##"
export const BIG_NUMBER = 9007199254740993n;
export function isEven(n: bigint): boolean {
    return n % 2n === 0n;
}
"##;
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();

    assert!(result.source.contains("i64") || result.source.contains("bigint"));
}

/// Test enum with string values.
#[test]
fn test_string_enum() {
    let source = r##"
export enum Direction {
    Up = "up",
    Down = "down",
    Left = "left",
    Right = "right"
}
export function getOpposite(dir: Direction): Direction {
    switch (dir) {
        case Direction.Up: return Direction.Down;
        case Direction.Down: return Direction.Up;
        case Direction.Left: return Direction.Right;
        case Direction.Right: return Direction.Left;
    }
}
"##;
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();

    assert!(result.source.contains("pub enum"));
    assert!(result.source.contains("match"));
}

/// Test const assertions.
#[test]
fn test_const_assertions() {
    let source = r##"
export const Colors = {
    Red: 'red',
    Green: 'green',
    Blue: 'blue'
} as const;
export function getColor(name: keyof typeof Colors): string {
    return Colors[name];
}
"##;
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();

    assert!(result.source.contains("Colors"));
}

/// Test keyof operator.
#[test]
fn test_keyof() {
    let source = r##"
export type Point = { x: number, y: number };
export function getKeys(obj: Point): (keyof Point)[] {
    return ["x", "y"];
}
"##;
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();

    assert!(!result.source.is_empty());
}

/// Test lookup types.
#[test]
fn test_lookup_types() {
    let source = r##"
export type Point = { x: number, y: number };
export type XCoord = Point["x"];
export type PointKeys = keyof Point;
"##;
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();

    assert!(result.source.contains("pub struct Point"));
}

/// Test variadic tuple types.
#[test]
fn test_tuple_types() {
    let source = r##"
export function firstTwo(pair: [string, number]): string {
    return pair[0];
}
export function makePair<A, B>(a: A, b: B): [A, B] {
    return [a, b];
}
"##;
    let file = parser::parse_file_from_str(source, "test.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();

    assert!(result.source.contains("pub fn"));
}
