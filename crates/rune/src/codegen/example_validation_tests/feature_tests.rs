//! # Example Feature Tests
//!
//! Tests for various TypeScript features.

use crate::{analyzer, codegen, parser};

/// Test array methods.
#[test]
fn test_array_methods() {
    let source = "
export function processNumbers(nums: number[]): {
    sum: number,
    filtered: number[],
    doubled: number[],
} {
    const sum = nums.reduce((acc, n) => acc + n, 0);
    const filtered = nums.filter(n => n > 0);
    const doubled = nums.map(n => n * 2);
    return { sum, filtered, doubled };
}

export function findMax(nums: number[]): number | null {
    if (nums.length === 0) {
        return null;
    }
    return nums.reduce((a, b) => a > b ? a : b);
}
";
    let file = parser::parse_file_from_str(source, "arrays.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();
    assert!(!result.source.is_empty());
    assert!(result.source.contains("process_numbers"));
    assert!(result.source.contains("find_max"));
    assert!(result.source.contains("reduce"));
}

/// Test string methods.
#[test]
fn test_string_methods() {
    let source = "
export function processText(text: string): {
    upper: string,
    trimmed: string,
    words: string[],
    len: number,
} {
    return {
        upper: text.toUpperCase(),
        trimmed: text.trim(),
        words: text.split(\" \"),
        len: text.length,
    };
}

export function slugify(text: string): string {
    return text
        .toLowerCase()
        .trim()
        .replace(/\\s+/g, \"-\")
        .replace(/[^\\w-]+/g, \"\");
}
";
    let file = parser::parse_file_from_str(source, "strings.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();
    assert!(!result.source.is_empty());
    assert!(result.source.contains("process_text"));
    assert!(result.source.contains("slugify"));
    assert!(result.source.contains("to_uppercase") || result.source.contains("toUpperCase"));
}

/// Test closure capture patterns.
#[test]
fn test_closure_capture() {
    let source = "
export function createCounter(): () => number {
    let count = 0;
    const increment = () => {
        count = count + 1;
    };
    const get = () => count;
    return get;
}

export function createAdder(a: number): (b: number) => number {
    return (b) => a + b;
}
";
    let file = parser::parse_file_from_str(source, "closures.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();
    assert!(!result.source.is_empty());
    assert!(result.source.contains("create_counter"));
    assert!(result.source.contains("create_adder"));
}

/// Test control flow patterns.
#[test]
fn test_control_flow() {
    let source = "
export function fizzbuzz(n: number): string[] {
    const result: string[] = [];
    for (let i = 1; i <= n; i++) {
        if (i % 15 === 0) {
            result.push(\"FizzBuzz\");
        } else if (i % 3 === 0) {
            result.push(\"Fizz\");
        } else if (i % 5 === 0) {
            result.push(\"Buzz\");
        } else {
            result.push(String(i));
        }
    }
    return result;
}

export function binarySearch(
    arr: number[],
    target: number
): number {
    let left = 0;
    let right = arr.length - 1;
    
    while (left <= right) {
        const mid = Math.floor((left + right) / 2);
        if (arr[mid] === target) {
            return mid;
        } else if (arr[mid] < target) {
            left = mid + 1;
        } else {
            right = mid - 1;
        }
    }
    return -1;
}
";
    let file = parser::parse_file_from_str(source, "control.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();
    assert!(!result.source.is_empty());
    assert!(result.source.contains("fizzbuzz"));
    assert!(result.source.contains("binary_search"));
    assert!(result.source.contains("while"));
    assert!(result.source.contains("if"));
}

/// Test object operations.
#[test]
fn test_object_operations() {
    let source = "
export type Config = {
    name: string,
    value: number,
    enabled: boolean,
};

export function mergeConfig(
    base: Config,
    overrides: Partial<Config>
): Config {
    return {
        ...base,
        ...overrides,
    };
}

export function pick<T extends object, K extends keyof T>(
    obj: T,
    keys: K[]
): Pick<T, K> {
    const result: Partial<T> = {};
    for (const key of keys) {
        result[key] = obj[key];
    }
    return result as Pick<T, K>;
}
";
    let file = parser::parse_file_from_str(source, "objects.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();
    assert!(!result.source.is_empty());
    assert!(result.source.contains("Config"));
    assert!(result.source.contains("merge_config"));
    assert!(result.source.contains("pick"));
}

/// Test map operations.
#[test]
fn test_map_operations() {
    let source = "
export type Entry = {
    key: string,
    value: number,
};

export function groupBy<T>(
    items: T[],
    keyFn: (item: T) => string
): Map<string, T[]> {
    const result = new Map<string, T[]>();
    for (const item of items) {
        const key = keyFn(item);
        const group = result.get(key);
        if (group) {
            group.push(item);
        } else {
            result.set(key, [item]);
        }
    }
    return result;
}
";
    let file = parser::parse_file_from_str(source, "maps.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();
    assert!(!result.source.is_empty());
    assert!(result.source.contains("Entry"));
    assert!(result.source.contains("group_by"));
    assert!(result.source.contains("Map"));
}
