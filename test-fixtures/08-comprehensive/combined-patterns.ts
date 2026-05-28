// ============================================================================
// COMPREHENSIVE - Combined Language Features
// ============================================================================

// ============================================================================
// 1. COMBINING LITERALS AND EXPRESSIONS
// ============================================================================

const template = `User: ${user.name}, Age: ${user.age}`;
const arr = [1, 2, ...rest];
const obj = { ...base, ...overrides };

// ============================================================================
// 2. DESTRUCTURING WITH FUNCTIONS
// ============================================================================

function processUser({ name, age, address: { city, zip } }: {
  name: string;
  age: number;
  address: { city: string; zip: string };
}): string {
  return `${name} lives in ${city}`;
}

const result = processUser({
  name: "Alice",
  age: 30,
  address: { city: "NYC", zip: "10001" }
});

// ============================================================================
// 3. ARROW FUNCTIONS AS CALLBACKS
// ============================================================================

const numbers = [1, 2, 3, 4, 5];
const sum = numbers
  .filter(n => n % 2 === 0)
  .map(n => n * n)
  .reduce((a, b) => a + b, 0);

const people = [
  { name: "Alice", age: 30 },
  { name: "Bob", age: 25 },
  { name: "Charlie", age: 35 }
];

const names = people
  .filter(p => p.age >= 30)
  .map(p => p.name)
  .sort();

// ============================================================================
// 4. OPTIONAL CHAINING IN OBJECTS
// ============================================================================

const company = {
  name: "Acme",
  departments: [
    {
      name: "Engineering",
      lead: { name: "Alice" }
    }
  ]
};

const leadName = company.departments?.[0]?.lead?.name ?? "Unknown";

// ============================================================================
// 5. NULLISH COALESCING
// ============================================================================

const config = {
  port: null,
  host: undefined,
  timeout: 0
};

const port = config.port ?? 8080;
const host = config.host ?? "localhost";
const timeout = config.timeout ?? 5000;

// ============================================================================
// 6. CLOSURES
// ============================================================================

function createAdder(base: number) {
  return (n: number) => base + n;
}

const add5 = createAdder(5);
const add10 = createAdder(10);

// ============================================================================
// 7. RECURSIVE FUNCTIONS
// ============================================================================

const fibonacci = (n: number): number => {
  if (n <= 1) return n;
  return fibonacci(n - 1) + fibonacci(n - 2);
};

const factorial = (n: number): number => {
  return n <= 1 ? 1 : n * factorial(n - 1);
};

// ============================================================================
// 8. IIFE (Immediately Invoked Function Expression)
// ============================================================================

const computed = (function() {
  const base = 10;
  return base * 2 + 5;
})();

// ============================================================================
// 9. TERNARY OPERATOR
// ============================================================================

const grade = (score: number): string => {
  return score >= 90 ? "A" :
         score >= 80 ? "B" :
         score >= 70 ? "C" :
         score >= 60 ? "D" : "F";
};

// ============================================================================
// 10. FOR...OF WITH DESTRUCTURING
// ============================================================================

const pairs = [
  ["a", 1],
  ["b", 2],
  ["c", 3]
];

for (const [key, value] of pairs) {
  console.log(`${key}: ${value}`);
}

// ============================================================================
// 11. SWITCH WITH MULTIPLE CASES
// ============================================================================

const getCategory = (status: number): string => {
  switch (status) {
    case 200:
    case 201:
    case 204:
      return "Success";
    case 400:
    case 401:
    case 403:
      return "Client Error";
    case 500:
    case 502:
    case 503:
      return "Server Error";
    default:
      return "Unknown";
  }
};

// ============================================================================
// 12. TRY-CATCH WITH ASYNC
// ============================================================================

async function safeAsync<T>(fn: () => Promise<T>): Promise<{ data?: T; error?: string }> {
  try {
    const data = await fn();
    return { data };
  } catch (e) {
    return { error: String(e) };
  }
}
