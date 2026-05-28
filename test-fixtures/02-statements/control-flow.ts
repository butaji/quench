// ============================================================================
// CONTROL FLOW - If, Switch, Ternary
// ============================================================================

// If statement
const ifResult = (x: number): string => {
  if (x > 0) {
    return "positive";
  } else if (x < 0) {
    return "negative";
  } else {
    return "zero";
  }
};

// Switch statement
const switchResult = (day: number): string => {
  switch (day) {
    case 0: return "Sunday";
    case 1: return "Monday";
    case 2: return "Tuesday";
    case 3: return "Wednesday";
    case 4: return "Thursday";
    case 5: return "Friday";
    case 6: return "Saturday";
    default: return "Invalid day";
  }
};

// Switch with fall-through
const getSound = (animal: string): string => {
  switch (animal) {
    case "dog":
    case "cat":
      return "mammal sound";
    case "bird":
      return "chirp";
    default:
      return "unknown";
  }
};

// Ternary operator
const ternary = (x: number) => x > 0 ? "positive" : "not positive";

// Nested ternary
const grade = (score: number): string => {
  return score >= 90 ? "A" :
         score >= 80 ? "B" :
         score >= 70 ? "C" :
         score >= 60 ? "D" : "F";
};

// Logical operators for conditionals
const and = (a: boolean, b: boolean) => a && b;
const or = (a: boolean, b: boolean) => a || b;
const not = (a: boolean) => !a;

// Nullish coalescing
const nullish = (x: string | null | undefined): string => {
  return x ?? "default";
};

// Optional chaining with nullish
interface User {
  address?: {
    city?: string;
  };
}
const getCity = (user: User): string => {
  return user?.address?.city ?? "Unknown";
};
