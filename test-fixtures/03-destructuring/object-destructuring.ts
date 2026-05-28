// ============================================================================
// DESTRUCTURING - Objects
// ============================================================================

// Basic object destructuring
const { name, age } = { name: "Alice", age: 30 };

// Renaming variables
const { name: userName, age: userAge } = { name: "Bob", age: 25 };

// Default values
const { a = 1, b = 2 } = { a: 10 };
const { x = "default" } = {};

// Nested destructuring
const {
  address: { city, zip },
  email
}: {
  address: { city: string; zip: string };
  email: string;
} = {
  address: { city: "NYC", zip: "10001" },
  email: "test@example.com"
};

// Rest pattern
const { first, second, ...remaining } = { first: 1, second: 2, third: 3, fourth: 4 };

// In function parameters
const greet = ({ name, greeting = "Hello" }: { name: string; greeting?: string }) => 
  `${greeting}, ${name}!`;

// In for-of loop
const people = [
  { name: "Alice", age: 30 },
  { name: "Bob", age: 25 }
];
for (const { name, age } of people) {
  console.log(`${name} is ${age}`);
}

// In function return
function getCoordinates(): { x: number; y: number; z: number } {
  return { x: 1, y: 2, z: 3 };
}
const { x, y, z } = getCoordinates();

// Combined with arrays
const [{ a }, { b }] = [{ a: 1 }, { b: 2 }];
