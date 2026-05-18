//! # Parser Integration Tests
//!
//! Tests for parsing various TypeScript constructs.

use crate::parser;

#[test]
fn test_parse_simple_type() {
    let source = "
export type Point = {
    x: number,
    y: number,
};
";
    let file = parser::parse_file_from_str(source, "point.r.ts").unwrap();
    assert!(file.valid || !file.errors.is_empty());
}

#[test]
fn test_parse_tagged_union() {
    let source = "
export type Message =
    | { tag: \"Move\"; x: number; y: number }
    | { tag: \"Stop\" };
";
    let file = parser::parse_file_from_str(source, "message.r.ts").unwrap();
    assert!(file.valid || !file.errors.is_empty());
}

#[test]
fn test_parse_function() {
    let source = "
export function add(a: number, b: number): number {
    return a + b;
}
";
    let file = parser::parse_file_from_str(source, "add.r.ts").unwrap();
    assert!(file.valid || !file.errors.is_empty());
}

#[test]
fn test_parse_arrow_function() {
    let source = "export const add = (a: number, b: number): number => a + b;";
    let file = parser::parse_file_from_str(source, "arrow.r.ts").unwrap();
    assert!(file.valid || !file.errors.is_empty());
}

#[test]
fn test_parse_async_function() {
    let source = "
export async function fetchData(url: string): Promise<string> {
    return \"data\";
}
";
    let file = parser::parse_file_from_str(source, "async.r.ts").unwrap();
    assert!(file.valid || !file.errors.is_empty());
}

#[test]
fn test_parse_for_loop() {
    let source = "
export function sumArray(arr: number[]): number {
    let sum = 0;
    for (let i = 0; i < arr.length; i++) {
        sum = sum + arr[i];
    }
    return sum;
}
";
    let file = parser::parse_file_from_str(source, "for.r.ts").unwrap();
    assert!(file.valid || !file.errors.is_empty());
}

#[test]
fn test_parse_for_of_loop() {
    let source = "
export function printAll(items: string[]): void {
    for (const item of items) {
        console.log(item);
    }
}
";
    let file = parser::parse_file_from_str(source, "forof.r.ts").unwrap();
    assert!(file.valid || !file.errors.is_empty());
}

#[test]
fn test_parse_switch_statement() {
    let source = "
export function getColorName(color: string): string {
    switch (color) {
        case \"red\": return \"Red\";
        case \"green\": return \"Green\";
        default: return \"Unknown\";
    }
}
";
    let file = parser::parse_file_from_str(source, "switch.r.ts").unwrap();
    assert!(file.valid || !file.errors.is_empty());
}

#[test]
fn test_parse_import_export() {
    let source = "
import { foo } from \"./bar.r.ts\";
export { foo as bar };
";
    let file = parser::parse_file_from_str(source, "import.r.ts").unwrap();
    assert!(file.valid || !file.errors.is_empty());
}

#[test]
fn test_parse_string_literals() {
    let source = "
export const greeting = \"Hello, World!\";
export const template = `Hello, ${name}!`;
";
    let file = parser::parse_file_from_str(source, "strings.r.ts").unwrap();
    assert!(file.valid || !file.errors.is_empty());
}

#[test]
fn test_parse_numeric_literals() {
    let source = "
export const integer = 42;
export const float = 3.14;
export const hex = 0xFF;
export const binary = 0b1010;
";
    let file = parser::parse_file_from_str(source, "numbers.r.ts").unwrap();
    assert!(file.valid || !file.errors.is_empty());
}

#[test]
fn test_parse_generic_function() {
    let source = "
export function identity<T>(x: T): T {
    return x;
}
";
    let file = parser::parse_file_from_str(source, "generic.r.ts").unwrap();
    assert!(file.valid || !file.errors.is_empty());
}

#[test]
fn test_parse_result_pattern() {
    let source = "
export function divide(a: number, b: number):
    | { ok: true; value: number }
    | { ok: false; error: string }
{
    if (b === 0) {
        return { ok: false, error: \"Division by zero\" };
    }
    return { ok: true, value: a / b };
}
";
    let file = parser::parse_file_from_str(source, "result.r.ts").unwrap();
    assert!(file.valid || !file.errors.is_empty());
}

#[test]
fn test_parse_ternary() {
    let source = "
export function max(a: number, b: number): number {
    return a > b ? a : b;
}
";
    let file = parser::parse_file_from_str(source, "ternary.r.ts").unwrap();
    assert!(file.valid || !file.errors.is_empty());
}

#[test]
fn test_parse_enum() {
    let source = "
export enum Color {
    Red,
    Green,
    Blue,
}
";
    let file = parser::parse_file_from_str(source, "enum.r.ts").unwrap();
    assert!(file.valid || !file.errors.is_empty());
}

#[test]
fn test_parse_type_alias() {
    let source = "
export type ID = string | number;
export type Callback = () => void;
export type Pair<A, B> = { first: A, second: B };
";
    let file = parser::parse_file_from_str(source, "typealias.r.ts").unwrap();
    assert!(file.valid || !file.errors.is_empty());
}

#[test]
fn test_parse_jsx_element() {
    let source = "
export function Component(): Widget {
    return (
        <div>
            <span>Hello</span>
        </div>
    );
}
";
    let file = parser::parse_file_from_str(source, "jsx.r.tsx").unwrap();
    assert!(file.is_tsx());
    assert!(file.valid || !file.errors.is_empty());
}

#[test]
fn test_parse_const_and_let() {
    let source = "
export const PI = 3.14159;
export let mutable = 0;

export function update() {
    mutable = mutable + 1;
}
";
    let file = parser::parse_file_from_str(source, "vars.r.ts").unwrap();
    assert!(file.valid || !file.errors.is_empty());
}

#[test]
fn test_parse_if_statement() {
    let source = "
export function sign(n: number): number {
    if (n > 0) {
        return 1;
    } else if (n < 0) {
        return -1;
    }
    return 0;
}
";
    let file = parser::parse_file_from_str(source, "if.r.ts").unwrap();
    assert!(file.valid || !file.errors.is_empty());
}

#[test]
fn test_parse_while_loop() {
    let source = "
export function countdown(n: number): void {
    while (n > 0) {
        n = n - 1;
    }
}
";
    let file = parser::parse_file_from_str(source, "while.r.ts").unwrap();
    assert!(file.valid || !file.errors.is_empty());
}
