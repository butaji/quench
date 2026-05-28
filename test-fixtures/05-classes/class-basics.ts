// ============================================================================
// CLASSES
// ============================================================================

// Basic class
class Point {
  x: number;
  y: number;

  constructor(x: number, y: number) {
    this.x = x;
    this.y = y;
  }

  distance(): number {
    return Math.sqrt(this.x ** 2 + this.y ** 2);
  }
}

const p = new Point(3, 4);
const dist = p.distance();

// Class with methods
class Counter {
  private count: number = 0;

  increment(): number {
    return ++this.count;
  }

  decrement(): number {
    return --this.count;
  }

  getCount(): number {
    return this.count;
  }

  reset(): void {
    this.count = 0;
  }
}

// Getter and setter
class Person {
  private _name: string = "";

  get name(): string {
    return this._name;
  }

  set name(value: string) {
    this._name = value.trim();
  }
}

const person = new Person();
person.name = "  Alice  ";
const name = person.name; // "Alice"

// Static methods and properties
class MathUtils {
  static PI = 3.14159;

  static add(a: number, b: number): number {
    return a + b;
  }

  static circleArea(radius: number): number {
    return MathUtils.PI * radius * radius;
  }
}

const pi = MathUtils.PI;
const area = MathUtils.circleArea(5);

// Class with inheritance
class Animal {
  constructor(public name: string) {}

  speak(): string {
    return `${this.name} makes a sound`;
  }
}

class Dog extends Animal {
  speak(): string {
    return `${this.name} barks`;
  }
}

class Cat extends Animal {
  speak(): string {
    return `${this.name} meows`;
  }
}

const dog = new Dog("Rex");
const cat = new Cat("Whiskers");

// Method chaining (fluent interface)
class Builder {
  private parts: string[] = [];

  addPart(part: string): this {
    this.parts.push(part);
    return this;
  }

  build(): string {
    return this.parts.join("-");
  }
}

const result = new Builder()
  .addPart("a")
  .addPart("b")
  .addPart("c")
  .build();
