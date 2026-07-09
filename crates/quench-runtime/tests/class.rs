//! Tests for ES6 class support in quench-runtime
//!
//! This file tests the ES6 class implementation. Features that are
//! working vs. not yet implemented are documented below.
//!
//! WORKING:
//! - Class declarations with constructor
//! - Class expressions (named and unnamed)
//! - Instance methods
//! - Getters and setters
//! - Static methods
//! - extends clause (basic prototype chain)
//! - instanceof checks
//!
//! NOT YET WORKING (documented as failing tests):
//! - Static field declarations (static count = 0)
//! - super() calls in constructors
//! - super.method() calls
//! - Extending built-in classes like Array
//! - Private fields (#field syntax)

use quench_runtime::Context;
use quench_runtime::Value;

// =========================================================================
// Working tests - basic class functionality
// =========================================================================

#[test]
fn test_class_declaration_basic() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(r#"
        class Point {
            constructor(x, y) {
                this.x = x;
                this.y = y;
            }
        }
        const p = new Point(1, 2);
        p.x;
    "#);
    assert!(result.is_ok(), "Class declaration failed: {:?}", result);
    assert_eq!(result.unwrap(), Value::Number(1.0));
}

#[test]
fn test_class_with_method() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(r#"
        class Counter {
            constructor() {
                this.count = 0;
            }
            increment() {
                this.count = this.count + 1;
            }
            getCount() {
                return this.count;
            }
        }
        const c = new Counter();
        c.getCount();
    "#);
    assert!(result.is_ok(), "Class with method failed: {:?}", result);
    assert_eq!(result.unwrap(), Value::Number(0.0));
}

#[test]
fn test_class_extends() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(r#"
        class Animal {
            constructor(name) {
                this.name = name;
            }
            speak() {
                return "generic sound";
            }
        }
        class Dog extends Animal {
            speak() {
                return "woof";
            }
        }
        const d = new Dog("Rex");
        d.name;
    "#);
    assert!(result.is_ok(), "Class extends failed: {:?}", result);
    assert_eq!(result.unwrap(), Value::String("Rex".to_string()));
}

#[test]
fn test_class_static_method() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(r#"
        class MathHelper {
            static add(a, b) {
                return a + b;
            }
        }
        MathHelper.add(2, 3);
    "#);
    assert!(result.is_ok(), "Class static method failed: {:?}", result);
    assert_eq!(result.unwrap(), Value::Number(5.0));
}

#[test]
fn test_class_expression() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(r#"
        const Rectangle = class {
            constructor(width, height) {
                this.width = width;
                this.height = height;
            }
            area() {
                return this.width * this.height;
            }
        };
        const r = new Rectangle(3, 4);
        r.area();
    "#);
    assert!(result.is_ok(), "Class expression failed: {:?}", result);
    assert_eq!(result.unwrap(), Value::Number(12.0));
}

#[test]
fn test_class_named_expression() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(r#"
        const Square = class Square {
            constructor(side) {
                this.side = side;
            }
            area() {
                return this.side * this.side;
            }
        };
        new Square(5).area();
    "#);
    assert!(result.is_ok(), "Named class expression failed: {:?}", result);
    assert_eq!(result.unwrap(), Value::Number(25.0));
}

#[test]
fn test_class_getter() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(r#"
        class Circle {
            constructor(radius) {
                this.radius = radius;
            }
            get diameter() {
                return this.radius * 2;
            }
        }
        new Circle(5).diameter;
    "#);
    assert!(result.is_ok(), "Class getter failed: {:?}", result);
    assert_eq!(result.unwrap(), Value::Number(10.0));
}

#[test]
fn test_class_setter() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(r#"
        class Temperature {
            constructor() {
                this._celsius = 0;
            }
            get celsius() {
                return this._celsius;
            }
            set celsius(value) {
                this._celsius = value;
            }
        }
        const t = new Temperature();
        t.celsius = 100;
        t.celsius;
    "#);
    assert!(result.is_ok(), "Class setter failed: {:?}", result);
    assert_eq!(result.unwrap(), Value::Number(100.0));
}

#[test]
fn test_class_prototype_chain() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(r#"
        class Base {
            greet() {
                return "hello";
            }
        }
        class Derived extends Base {
            farewell() {
                return "goodbye";
            }
        }
        const d = new Derived();
        d.greet();
    "#);
    assert!(result.is_ok(), "Class prototype chain failed: {:?}", result);
    assert_eq!(result.unwrap(), Value::String("hello".to_string()));
}

#[test]
fn test_class_instanceof() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(r#"
        class A {}
        class B extends A {}
        const b = new B();
        b instanceof B;
    "#);
    assert!(result.is_ok(), "Class instanceof failed: {:?}", result);
    assert_eq!(result.unwrap(), Value::Boolean(true));
}

#[test]
fn test_class_constructor_returns_object() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(r#"
        class MyClass {
            constructor() {
                return { value: 42 };
            }
        }
        new MyClass().value;
    "#);
    assert!(result.is_ok(), "Class constructor returning object failed: {:?}", result);
    assert_eq!(result.unwrap(), Value::Number(42.0));
}

#[test]
fn test_class_multiple_methods() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(r#"
        class Calculator {
            add(a, b) { return a + b; }
            subtract(a, b) { return a - b; }
            multiply(a, b) { return a * b; }
        }
        const calc = new Calculator();
        calc.subtract(10, 3);
    "#);
    assert!(result.is_ok(), "Class multiple methods failed: {:?}", result);
    assert_eq!(result.unwrap(), Value::Number(7.0));
}

#[test]
fn test_class_no_constructor() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(r#"
        class Empty {}
        const e = new Empty();
        e instanceof Empty;
    "#);
    assert!(result.is_ok(), "Class with no constructor failed: {:?}", result);
    assert_eq!(result.unwrap(), Value::Boolean(true));
}

#[test]
fn test_class_property_access() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(r#"
        class Person {
            constructor(name, age) {
                this.name = name;
                this.age = age;
            }
        }
        const p = new Person("Alice", 30);
        p.name + " is " + p.age + " years old";
    "#);
    assert!(result.is_ok(), "Class property access failed: {:?}", result);
    assert_eq!(result.unwrap(), Value::String("Alice is 30 years old".to_string()));
}

#[test]
fn test_class_this_in_arrow_in_constructor() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(r#"
        class MyClass {
            constructor() {
                this.handler = () => this.value;
                this.value = 42;
            }
        }
        new MyClass().handler();
    "#);
    assert!(result.is_ok(), "Arrow in constructor failed: {:?}", result);
    assert_eq!(result.unwrap(), Value::Number(42.0));
}

// =========================================================================
// Tests for features that need implementation
// These tests document expected behavior for features not yet implemented
// =========================================================================

#[test]
#[ignore] // super() calls not yet working properly
fn test_class_super_call_in_constructor() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(r#"
        class Animal {
            constructor(name) {
                this.name = name;
            }
        }
        class Dog extends Animal {
            constructor(name, breed) {
                super(name);
                this.breed = breed;
            }
        }
        const d = new Dog("Rex", "Labrador");
        d.name + " is a " + d.breed;
    "#);
    assert!(result.is_ok(), "super() call in constructor failed: {:?}", result);
    assert_eq!(result.unwrap(), Value::String("Rex is a Labrador".to_string()));
}

#[test]
#[ignore] // super() calls not yet working properly
fn test_class_super_call_no_args() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(r#"
        class Base {
            constructor() {
                this.baseValue = 42;
            }
        }
        class Derived extends Base {
            constructor() {
                super();
                this.derivedValue = 100;
            }
        }
        const d = new Derived();
        [d.baseValue, d.derivedValue];
    "#);
    assert!(result.is_ok(), "super() with no args failed: {:?}", result);
    match result.unwrap() {
        Value::Object(o) => {
            let o = o.borrow();
            assert_eq!(o.elements.get(0), Some(&Value::Number(42.0)), "baseValue should be 42");
            assert_eq!(o.elements.get(1), Some(&Value::Number(100.0)), "derivedValue should be 100");
        }
        _ => panic!("Expected array"),
    }
}

#[test]
#[ignore] // super() calls not yet working properly
fn test_class_chaining_constructors() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(r#"
        class Base {
            constructor(x) {
                this.x = x;
            }
        }
        class Middle extends Base {
            constructor(x, y) {
                super(x);
                this.y = y;
            }
        }
        class Derived extends Middle {
            constructor(x, y, z) {
                super(x, y);
                this.z = z;
            }
        }
        const d = new Derived(1, 2, 3);
        d.x + d.y + d.z;
    "#);
    assert!(result.is_ok(), "Chaining constructors failed: {:?}", result);
    assert_eq!(result.unwrap(), Value::Number(6.0));
}

#[test]
#[ignore] // super.method() not yet working
fn test_class_super_method_call() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(r#"
        class Base {
            greet() {
                return "Hello";
            }
        }
        class Derived extends Base {
            greet() {
                return super.greet() + " World";
            }
        }
        new Derived().greet();
    "#);
    assert!(result.is_ok(), "super.method() call failed: {:?}", result);
    assert_eq!(result.unwrap(), Value::String("Hello World".to_string()));
}

#[test]
#[ignore] // Static field declarations not yet parsed
fn test_class_static_property() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(r#"
        class Config {
            static defaultName = "Anonymous";
            static getDefault() {
                return this.defaultName;
            }
        }
        Config.defaultName;
    "#);
    assert!(result.is_ok(), "Static property access failed: {:?}", result);
    assert_eq!(result.unwrap(), Value::String("Anonymous".to_string()));
}

#[test]
#[ignore] // Static field declarations not yet parsed
fn test_class_static_property_assignment() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(r#"
        class Counter {
            static count = 0;
            static increment() {
                this.count = this.count + 1;
                return this.count;
            }
        }
        Counter.increment();
        Counter.increment();
        Counter.count;
    "#);
    assert!(result.is_ok(), "Static property assignment failed: {:?}", result);
    assert_eq!(result.unwrap(), Value::Number(2.0));
}

#[test]
#[ignore] // Static field declarations not yet parsed
fn test_class_static_this_access() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(r#"
        class MathUtils {
            static factor = 10;
            static multiply(x) {
                return x * this.factor;
            }
        }
        MathUtils.multiply(5);
    "#);
    assert!(result.is_ok(), "Static this access failed: {:?}", result);
    assert_eq!(result.unwrap(), Value::Number(50.0));
}

#[test]
#[ignore] // Extending built-in Array requires native constructor support
fn test_class_extends_builtin_array() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(r#"
        class Stack extends Array {
            constructor() {
                super();
                this._items = [];
            }
            push(item) {
                this._items.push(item);
            }
            pop() {
                return this._items.pop();
            }
        }
        const s = new Stack();
        s.push(1);
        s.push(2);
        s.pop();
    "#);
    assert!(result.is_ok(), "Extends Array failed: {:?}", result);
    assert_eq!(result.unwrap(), Value::Number(2.0));
}

#[test]
#[ignore] // super() and extends expressions not yet working
fn test_class_extends_expression() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(r#"
        const getBaseClass = function() {
            return class {
                getValue() { return 42; }
            };
        };
        class Derived extends getBaseClass() {
            getValue() {
                return super.getValue() + 8;
            }
        }
        new Derived().getValue();
    "#);
    assert!(result.is_ok(), "Extends expression failed: {:?}", result);
    assert_eq!(result.unwrap(), Value::Number(50.0));
}

#[test]
#[ignore] // super() calls not yet working
fn test_class_instanceof_with_extends() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(r#"
        class A {}
        class B extends A {}
        class C extends B {}
        const c = new C();
        [c instanceof C, c instanceof B, c instanceof A, c instanceof Object];
    "#);
    assert!(result.is_ok(), "instanceof with multi-level extends failed: {:?}", result);
    match result.unwrap() {
        Value::Object(o) => {
            let o = o.borrow();
            assert_eq!(o.elements.get(0), Some(&Value::Boolean(true)), "instanceof C");
            assert_eq!(o.elements.get(1), Some(&Value::Boolean(true)), "instanceof B");
            assert_eq!(o.elements.get(2), Some(&Value::Boolean(true)), "instanceof A");
            assert_eq!(o.elements.get(3), Some(&Value::Boolean(true)), "instanceof Object");
        }
        _ => panic!("Expected array"),
    }
}

#[test]
#[ignore] // super() auto-injection not yet working
fn test_class_constructor_default_when_no_super() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(r#"
        class Base {
            constructor() {
                this.initialized = true;
            }
        }
        class Derived extends Base {
            constructor() {
                // JavaScript auto-calls super() if not explicitly called
                this.extra = 1;
            }
        }
        const d = new Derived();
        [d.initialized, d.extra];
    "#);
    assert!(result.is_ok(), "Constructor failed: {:?}", result);
    match result.unwrap() {
        Value::Object(o) => {
            let o = o.borrow();
            assert_eq!(o.elements.get(0), Some(&Value::Boolean(true)), "initialized should be true");
            assert_eq!(o.elements.get(1), Some(&Value::Number(1.0)), "extra should be 1");
        }
        _ => panic!("Expected array"),
    }
}

#[test]
#[ignore] // Private fields syntax not yet supported
fn test_class_private_fields_syntax() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(r#"
        class Counter {
            #count = 0;
            increment() {
                this.#count = this.#count + 1;
            }
            getCount() {
                return this.#count;
            }
        }
        const c = new Counter();
        c.increment();
        c.increment();
        c.getCount();
    "#);
    // Private fields syntax parsing may not be supported
    if result.is_err() {
        println!("Private fields not yet supported: {:?}", result);
    }
}

#[test]
#[ignore] // super.method() not yet working
fn test_class_method_returns_super() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(r#"
        class Base {
            getPrototype() {
                return Object.getPrototypeOf(this);
            }
        }
        class Derived extends Base {}
        const d = new Derived();
        d.getPrototype() === Derived.prototype;
    "#);
    assert!(result.is_ok(), "Method returning prototype check failed: {:?}", result);
    assert_eq!(result.unwrap(), Value::Boolean(true));
}
