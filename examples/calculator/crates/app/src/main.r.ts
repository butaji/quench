// main.r.ts - Calculator Demo
// Demonstrates: generics, pattern matching, tagged unions, Option patterns

// Expression types for a simple calculator
export type Expr =
    | { tag: "Number"; value: number }
    | { tag: "Add"; left: Expr; right: Expr }
    | { tag: "Sub"; left: Expr; right: Expr }
    | { tag: "Mul"; left: Expr; right: Expr }
    | { tag: "Div"; left: Expr; right: Expr }
    | { tag: "Neg"; expr: Expr }
    | { tag: "Var"; name: string };

export type Result = 
    | { ok: true; value: number }
    | { ok: false; error: string };

// Parse a number literal
export function parseNumber(s: string): number | null {
    const n = parseFloat(s);
    if (n === n) { // NaN check
        return n;
    }
    return null;
}

// Evaluate an expression
export function evaluate(expr: Expr, vars: Record<string, number>): Result {
    switch (expr.tag) {
        case "Number":
            return { ok: true, value: expr.value };
        case "Var":
            const val = vars[expr.name];
            if (val !== undefined) {
                return { ok: true, value: val };
            }
            return { ok: false, error: "Unknown variable: " + expr.name };
        case "Neg":
            const inner = evaluate(expr.expr, vars);
            if (!inner.ok) {
                return inner;
            }
            return { ok: true, value: -inner.value };
        case "Add":
            return evalBinary(expr.left, expr.right, vars, (a, b) => a + b);
        case "Sub":
            return evalBinary(expr.left, expr.right, vars, (a, b) => a - b);
        case "Mul":
            return evalBinary(expr.left, expr.right, vars, (a, b) => a * b);
        case "Div":
            return evalBinary(expr.left, expr.right, vars, (a, b) => {
                if (b === 0) {
                    return { ok: false, error: "Division by zero" };
                }
                return { ok: true, value: a / b };
            });
    }
}

// Helper for binary operations
function evalBinary(
    left: Expr,
    right: Expr,
    vars: Record<string, number>,
    op: (a: number, b: number) => Result
): Result {
    const leftResult = evaluate(left, vars);
    if (!leftResult.ok) {
        return leftResult;
    }
    const rightResult = evaluate(right, vars);
    if (!rightResult.ok) {
        return rightResult;
    }
    return op(leftResult.value, rightResult.value);
}

// Pretty print an expression
export function toString(expr: Expr): string {
    switch (expr.tag) {
        case "Number":
            return String(expr.value);
        case "Var":
            return expr.name;
        case "Neg":
            return "-" + toString(expr.expr);
        case "Add":
            return "(" + toString(expr.left) + " + " + toString(expr.right) + ")";
        case "Sub":
            return "(" + toString(expr.left) + " - " + toString(expr.right) + ")";
        case "Mul":
            return "(" + toString(expr.left) + " * " + toString(expr.right) + ")";
        case "Div":
            return "(" + toString(expr.left) + " / " + toString(expr.right) + ")";
    }
}

// Simplify an expression (constant folding)
export function simplify(expr: Expr): Expr {
    switch (expr.tag) {
        case "Number":
        case "Var":
            return expr;
        case "Neg":
            const negInner = simplify(expr.expr);
            if (negInner.tag === "Number") {
                return { tag: "Number", value: -negInner.value };
            }
            return { tag: "Neg", expr: negInner };
        case "Add":
        case "Sub":
        case "Mul":
        case "Div":
            const left = simplify(expr.left);
            const right = simplify(expr.right);
            
            if (left.tag === "Number" && right.tag === "Number") {
                const result = evaluate({ tag: expr.tag, left, right }, {});
                if (result.ok) {
                    return { tag: "Number", value: result.value };
                }
            }
            
            if (expr.tag === "Add" && left.tag === "Number" && left.value === 0) {
                return right;
            }
            if (expr.tag === "Add" && right.tag === "Number" && right.value === 0) {
                return left;
            }
            if (expr.tag === "Mul" && left.tag === "Number" && left.value === 1) {
                return right;
            }
            if (expr.tag === "Mul" && right.tag === "Number" && right.value === 1) {
                return left;
            }
            if (expr.tag === "Mul" && (left.tag === "Number" && left.value === 0 || 
                                       right.tag === "Number" && right.value === 0)) {
                return { tag: "Number", value: 0 };
            }
            
            return { tag: expr.tag, left, right };
    }
}

// Generic stack operations
export type Stack<T> = {
    items: T[];
};

export function push<T>(stack: Stack<T>, item: T): Stack<T> {
    return { items: stack.items.concat([item]) };
}

export function pop<T>(stack: Stack<T>): { stack: Stack<T>; item: T | null } {
    if (stack.items.length === 0) {
        return { stack, item: null };
    }
    const newItems = stack.items.slice(0, -1);
    const item = stack.items[stack.items.length - 1];
    return { stack: { items: newItems }, item };
}

export function peek<T>(stack: Stack<T>): T | null {
    if (stack.items.length === 0) {
        return null;
    }
    return stack.items[stack.items.length - 1];
}
