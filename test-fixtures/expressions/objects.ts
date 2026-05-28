// ===========================================
// TypeScript Object & Property Access Tests
// ===========================================

// Object creation
const person = {
  name: "Alice",
  age: 30,
  greet() {
    return `Hello, I'm ${this.name}`;
  }
};

// Property access
const name: string = person.name;
const age: number = person["age"];

// Nested objects
const company = {
  name: "TechCorp",
  address: {
    street: "123 Main St",
    city: "San Francisco"
  }
};

const street: string = company.address.street;
const city: string = company["address"]["city"];

// Object with computed properties
const key = "dynamicKey";
const objWithComputed = {
  [key]: "computed value"
};

// Object spread
const base = { a: 1, b: 2 };
const extended = { ...base, c: 3 };

// Object destructuring
const { name: n, age: a } = person;

// Function to validate
export function validateObjects(): boolean {
  return person.name === "Alice"
    && person.age === 30
    && company.address.city === "San Francisco"
    && extended.a === 1
    && extended.c === 3;
}
