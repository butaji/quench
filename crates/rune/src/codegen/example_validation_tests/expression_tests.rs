//! # Example Expression Tests
//!
//! Tests for expressions, types, and pattern examples.

use crate::{analyzer, codegen, parser};

/// Test hello_world example transpilation.
#[test]
fn test_example_hello_world_basic() {
    let source = "
export type Greeting = {
    message: string,
    count: number,
};

export function createGreeting(name: string): Greeting {
    return {
        message: `Hello, ${name}!`,
        count: name.length,
    };
}

export function greet(names: string[]): void {
    for (const name of names) {
        console.log(createGreeting(name).message);
    }
}
";
    let file = parser::parse_file_from_str(source, "hello.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();
    assert!(!result.source.is_empty());
    assert!(result.source.contains("Greeting"));
    assert!(result.source.contains("create_greeting"));
    assert!(result.source.contains("greet"));
}

/// Test calculator example with tagged unions.
#[test]
fn test_example_calculator_expr() {
    let source = "
export type Expr =
    | { tag: \"Number\"; value: number }
    | { tag: \"Add\"; left: Expr; right: Expr }
    | { tag: \"Sub\"; left: Expr; right: Expr }
    | { tag: \"Mul\"; left: Expr; right: Expr }
    | { tag: \"Div\"; left: Expr; right: Expr };

export type CalcResult =
    | { ok: true; value: number }
    | { ok: false; error: string };

export function evalExpr(expr: Expr): number {
    switch (expr.tag) {
        case \"Number\": return expr.value;
        case \"Add\": return evalExpr(expr.left) + evalExpr(expr.right);
        case \"Sub\": return evalExpr(expr.left) - evalExpr(expr.right);
        case \"Mul\": return evalExpr(expr.left) * evalExpr(expr.right);
        case \"Div\": return evalExpr(expr.left) / evalExpr(expr.right);
    }
}

export function safeDiv(a: number, b: number): CalcResult {
    if (b === 0) {
        return { ok: false, error: \"division by zero\" };
    }
    return { ok: true, value: a / b };
}
";
    let file = parser::parse_file_from_str(source, "calc.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();
    assert!(!result.source.is_empty());
    assert!(result.source.contains("eval_expr") || result.source.contains("evalExpr"));
    assert!(result.source.contains("safe_div") || result.source.contains("safeDiv"));
}

/// Test todox keyboard handler.
#[test]
fn test_example_todox_keyboard() {
    let source = "
export type KeyboardMessage =
    | { tag: \"Move\"; dx: number; dy: number }
    | { tag: \"Quit\" }
    | { tag: \"Write\"; text: string }
    | { tag: \"Toggle\" }
    | { tag: \"Add\" }
    | { tag: \"Delete\" }
    | { tag: \"Filter\" };

export function handleMessage(msg: KeyboardMessage): void {
    switch (msg.tag) {
        case \"Move\":
            console.log(\"Moving\");
            break;
        case \"Quit\":
            console.log(\"Quitting\");
            break;
        case \"Write\":
            console.log(msg.text);
            break;
        case \"Toggle\":
        case \"Add\":
        case \"Delete\":
        case \"Filter\":
            break;
    }
}
";
    let file = parser::parse_file_from_str(source, "keyboard.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();
    assert!(!result.source.is_empty());
    assert!(result.source.contains("KeyboardMessage"));
    assert!(result.source.contains("handle_message"));
}

/// Test native import pattern.
#[test]
fn test_native_import_transpilation() {
    let source = "
import { fastSqrt, batchAdd } from \"native:math\";
import { handleSignal } from \"native:handlers\";

export function calculate(values: number[]): number[] {
    return values.map(v => fastSqrt(v));
}

export function sumAll(values: number[]): number[] {
    return batchAdd(values, values);
}
";
    let file = parser::parse_file_from_str(source, "native.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();
    assert!(!result.source.is_empty());
    assert!(result.source.contains("native"));
    assert!(result.source.contains("fast_sqrt"));
    assert!(result.source.contains("batch_add"));
    assert!(result.source.contains("calculate"));
}

/// Test Option patterns.
#[test]
fn test_option_patterns() {
    let source = "
export function findById<T extends { id: number }>(
    items: T[],
    id: number
): T | null {
    for (const item of items) {
        if (item.id === id) {
            return item;
        }
    }
    return null;
}

export function orDefault<T>(value: T | null, defaultValue: T): T {
    if (value !== null) {
        return value;
    }
    return defaultValue;
}

export function isPresent<T>(value: T | null): boolean {
    return value !== null;
}
";
    let file = parser::parse_file_from_str(source, "option.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();
    assert!(!result.source.is_empty());
    assert!(result.source.contains("find_by_id"));
    assert!(result.source.contains("or_default"));
    assert!(result.source.contains("is_present"));
}

/// Test Result/Either patterns.
#[test]
fn test_result_patterns() {
    let source = "
export type ParseResult =
    | { ok: true; value: number }
    | { ok: false; error: string };

export function parseInt(s: string): ParseResult {
    const n = parseFloat(s);
    if (isNaN(n)) {
        return { ok: false, error: \"invalid number\" };
    }
    return { ok: true, value: n };
}

export function divide(a: number, b: number): ParseResult {
    if (b === 0) {
        return { ok: false, error: \"division by zero\" };
    }
    return { ok: true, value: a / b };
}
";
    let file = parser::parse_file_from_str(source, "result.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();
    assert!(!result.source.is_empty());
    assert!(result.source.contains("parse_int") || result.source.contains("parseInt"));
    assert!(result.source.contains("divide"));
}
