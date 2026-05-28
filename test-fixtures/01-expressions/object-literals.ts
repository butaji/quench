// ============================================================================
// OBJECT EXPRESSIONS
// ============================================================================

// Empty object
const empty = {};

// With properties
const person = {
  name: "Alice",
  age: 30,
  active: true
};

// Shorthand property names
const x = 1, y = 2;
const point = { x, y };

// Computed property names
const key = "dynamic";
const objWithComputed = {
  [key]: "value"
};

// Method shorthand
const calculator = {
  value: 0,
  add(n) { return this.value + n; },
  subtract(n) { return this.value - n; }
};

// Getter and setter
const proto = {
  _name: "default",
  get name() { return this._name; },
  set name(v) { this._name = v; }
};

// Spread operator
const base = { a: 1, b: 2 };
const extended = { ...base, c: 3 };
const override = { ...base, a: 10 };

// Nested objects
const nested = {
  outer: {
    inner: {
      value: 42
    }
  }
};

// Accessing properties
const name = person.name;
const age = person["age"];

// Property access with variables
const propName = "name";
const propValue = person[propName];

// Destructuring
const { name: n, age: a } = person;
const { name: firstName, ...others } = person;

// Spread in destructuring
const { x1, ...remaining } = { x1: 1, y: 2, z: 3 };

// Object methods
const keys = Object.keys(person);
const values = Object.values(person);
const entries = Object.entries(person);

// Object.assign
const target = { a: 1 };
const source = { b: 2 };
const assigned = Object.assign(target, source);
