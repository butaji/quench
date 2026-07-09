//! Tests for ES6 class support
//!
//! Note: These tests are ignored until class support is fully implemented.
//! The AST types exist but constructor support is not yet complete.

use quench_runtime::Context;
use quench_runtime::Value;

#[test]
#[ignore = "class support not yet implemented"]
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
#[ignore = "class support not yet implemented"]
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
#[ignore = "class support not yet implemented"]
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
#[ignore = "class support not yet implemented"]
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
#[ignore = "class support not yet implemented"]
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
#[ignore = "class support not yet implemented"]
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
#[ignore = "class support not yet implemented"]
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
#[ignore = "class support not yet implemented"]
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
#[ignore = "class support not yet implemented"]
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
#[ignore = "class support not yet implemented"]
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
#[ignore = "class support not yet implemented"]
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
#[ignore = "class support not yet implemented"]
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
#[ignore = "class support not yet implemented"]
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
#[ignore = "class support not yet implemented"]
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
