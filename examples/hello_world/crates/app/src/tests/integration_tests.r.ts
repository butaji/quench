// integration_tests.r.ts - Integration tests for Rune compiler
// This file validates the full transpilation pipeline

// Test 1: Basic function transpilation
export function testBasicFunction(): boolean {
    const add = (a: number, b: number): number => a + b;
    return add(2, 3) === 5;
}

// Test 2: String operations
export function testStringOps(): boolean {
    const name = "Hello";
    const greeting = name + ", World!";
    return greeting.length === 13;
}

// Test 3: Array operations
export function testArrayOps(): boolean {
    const nums: number[] = [1, 2, 3, 4, 5];
    let sum = 0;
    for (let i = 0; i < nums.length; i++) {
        sum = sum + nums[i];
    }
    return sum === 15;
}

// Test 4: Object spread
export function testObjectSpread(): boolean {
    const base = { x: 1, y: 2 };
    const extended = { ...base, z: 3 };
    return extended.x === 1 && extended.y === 2 && extended.z === 3;
}

// Test 5: Tagged union pattern matching
export type Shape =
    | { tag: "Circle"; radius: number }
    | { tag: "Rectangle"; width: number; height: number }
    | { tag: "Triangle"; base: number; height: number };

export function testTaggedUnion(): boolean {
    const circle: Shape = { tag: "Circle", radius: 5 };
    
    switch (circle.tag) {
        case "Circle":
            return circle.radius === 5;
        case "Rectangle":
            return false;
        case "Triangle":
            return false;
    }
}

// Test 6: Result pattern
export function testResultPattern(): boolean {
    function divide(a: number, b: number):
        | { ok: true; value: number }
        | { ok: false; error: string }
    {
        if (b === 0) {
            return { ok: false, error: "Division by zero" };
        }
        return { ok: true, value: a / b };
    }

    const result = divide(10, 2);
    return result.ok && result.value === 5;
}

// Test 7: Option pattern
export function testOptionPattern(): boolean {
    function find(arr: number[], target: number): number | null {
        for (let i = 0; i < arr.length; i++) {
            if (arr[i] === target) {
                return i;
            }
        }
        return null;
    }

    const index = find([1, 2, 3], 2);
    return index !== null && index === 1;
}

// Test 8: Generic function
export function testGeneric(): boolean {
    function identity<T>(x: T): T {
        return x;
    }
    
    return identity(42) === 42 && identity("hello") === "hello";
}

// Test 9: Enum-like tagged union
export type Status =
    | { tag: "Idle" }
    | { tag: "Loading" }
    | { tag: "Success"; data: string }
    | { tag: "Error"; message: string };

export function testEnumUnion(): number {
    const status: Status = { tag: "Success", data: "loaded" };
    
    switch (status.tag) {
        case "Idle": return 0;
        case "Loading": return 1;
        case "Success": return status.data.length;
        case "Error": return -1;
    }
}

// Test 10: Closures with capture
export function testClosure(): boolean {
    let counter = 0;
    const increment = () => {
        counter = counter + 1;
    };
    
    increment();
    increment();
    return counter === 2;
}

// Test 11: Deeply nested expressions
export function testNestedExpr(): number {
    return ((1 + 2) * (3 + 4)) - ((5 + 6) / (7 + 8));
}

// Test 12: Conditional expressions
export function testConditional(): string {
    const x = 10;
    return x > 5 ? "big" : "small";
}

// Test 13: Null coalescing pattern
export function testNullCoalesce(): string {
    const value: string | null = null;
    const fallback = value !== null ? value : "default";
    return fallback;
}

// Test 14: Method chaining on arrays
export function testArrayMethods(): number {
    const nums: number[] = [1, 2, 3, 4, 5];
    const doubled: number[] = [];
    
    for (let i = 0; i < nums.length; i++) {
        doubled.push(nums[i] * 2);
    }
    
    let sum = 0;
    for (let i = 0; i < doubled.length; i++) {
        sum = sum + doubled[i];
    }
    
    return sum;
}

// Test 15: Type alias
export type Point = {
    x: number;
    y: number;
};

export function testTypeAlias(): boolean {
    const p: Point = { x: 10, y: 20 };
    return p.x === 10 && p.y === 20;
}

// Test 16: Interface (treated as type)
export type NamedValue = {
    name: string;
    value: number;
};

export function testInterface(): NamedValue {
    return { name: "test", value: 42 };
}

// Test 17: String literals
export function testStringLiterals(): boolean {
    const status: "pending" | "active" | "done" = "active";
    return status === "active";
}

// Test 18: Number literals
export function testNumberLiterals(): boolean {
    const port: 8080 | 3000 | 443 = 8080;
    return port === 8080;
}

// Test 19: Optional properties
export function testOptionalProps(): boolean {
    const user: { name: string; age?: number } = { name: "Alice" };
    return user.name === "Alice" && user.age === undefined;
}

// Test 20: Union of primitive types
export function testUnionPrimitives(): boolean {
    const id: number | string = 123;
    const strId: number | string = "abc";
    return typeof id === "number" && typeof strId === "string";
}
