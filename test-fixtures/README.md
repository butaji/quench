# Test Fixtures for Runts TypeScript Runtime

This directory contains TypeScript test fixtures to validate the `runts` TypeScript-to-Rust compiler and runtime.

## Directory Structure

```
test-fixtures/
├── 00-literals/           # Primitive types
│   ├── string-literals.ts
│   └── number-literals.ts
├── 01-expressions/         # Expression types
│   ├── array-literals.ts
│   ├── object-literals.ts
│   ├── binary-operators.ts
│   ├── function-expressions.ts
│   └── template-literals.ts
├── 02-statements/         # Statement types
│   ├── control-flow.ts
│   ├── loops.ts
│   └── try-catch.ts
├── 03-destructuring/      # Destructuring patterns
│   ├── array-destructuring.ts
│   └── object-destructuring.ts
├── 04-optional/           # Optional chaining & nullish
│   └── nullish-coalescing.ts
├── 05-classes/            # Class syntax
├── 06-async/              # Async/await patterns
└── 07-jsx/                # JSX components
```

## Supported Features (HIR Coverage)

### ✅ Literals
- String literals (`"hello"`, `'world'`, `` `template` ``)
- Number literals (`42`, `3.14`, `0xFF`, `0b1010`)
- Boolean literals (`true`, `false`)
- Null and Undefined (`null`, `undefined`)
- Template literals with expressions (`` `Hello ${name}!` ``)

### ✅ Expressions
- Array literals (`[1, 2, 3]`)
- Object literals (`{ a: 1, b: 2 }`)
- Binary operators (`+`, `-`, `*`, `/`, `%`, `**`)
- Comparison operators (`==`, `===`, `<`, `>`, `<=`, `>=`)
- Logical operators (`&&`, `||`, `!`)
- Bitwise operators (`&`, `|`, `^`, `<<`, `>>`, `>>>`)
- Ternary conditional (`condition ? true : false`)
- Function expressions and arrow functions
- Spread operator (`...array`, `...object`)

### ✅ Statements
- Variable declarations (`const`, `let`, `var`)
- If/else statements
- Switch statements
- For, while, do-while loops
- For-of and for-in loops
- Try/catch/finally
- Return statements
- Break and continue

### ✅ Destructuring
- Array destructuring (`const [a, b] = [1, 2]`)
- Object destructuring (`const { x, y } = point`)
- Rest patterns (`const [first, ...rest] = arr`)
- Default values in destructuring

### ✅ Optional Features
- Optional chaining (`obj?.prop?.nested`)
- Nullish coalescing (`value ?? default`)
- Optional parameters and properties

### 🔄 In Progress
- Async/await functions
- Classes and inheritance
- Generators and iterators
- JSX components

### ❌ Not Supported
- Complex type system features
- Decorators
- Namespace modules
- Certain ESNext features

## Running Tests

```bash
# Test all fixtures with runts
runts dev test-fixtures/

# Or compile and run
runts build test-fixtures/
cargo run --example test-fixtures
```

## Test Fixture Format

Each test file should:
1. Export functions or constants that can be called/accessed
2. Include both input and expected output patterns
3. Focus on a single language feature per file
4. Avoid external dependencies

## Adding New Test Fixtures

1. Create a new `.ts` file in the appropriate directory
2. Name it descriptively (e.g., `nullish-coalescing.ts`)
3. Include comments explaining the feature
4. Test both positive and edge cases
5. Avoid TypeScript-specific syntax that won't work at runtime
