// ============================================================================
// EXCEPTION HANDLING - try, catch, finally, throw
// ============================================================================

// Basic try-catch
const safeDivide = (a: number, b: number): number | string => {
  try {
    if (b === 0) throw new Error("Division by zero");
    return a / b;
  } catch (e) {
    return "Error occurred";
  }
};

// Try-catch with error variable
const withErrorVar = (fn: () => number): number => {
  try {
    return fn();
  } catch (error) {
    console.error("Error:", error);
    return -1;
  }
};

// Try-catch-finally
const withFinally = (fn: () => void): boolean => {
  let executed = false;
  try {
    fn();
  } catch (e) {
    return false;
  } finally {
    executed = true;
  }
  return executed;
};

// Finally always executes
let finallyRan = false;
try {
  throw new Error("Test error");
} catch (e) {
  // handle error
} finally {
  finallyRan = true;
}

// Nested try-catch
const nestedTry = (): string => {
  try {
    try {
      throw new Error("Inner error");
    } catch (inner) {
      throw new Error("Outer error");
    }
  } catch (outer) {
    return "Caught by outer";
  }
};

// Throw various types
const throwNumber = () => { throw 42; };
const throwString = () => { throw "error message"; };
const throwObject = () => { throw { code: 404, message: "Not found" }; };
const throwNull = () => { throw null; };

// Re-throwing
const rethrow = (): void => {
  try {
    riskyOperation();
  } catch (e) {
    console.error("Logging error:", e);
    throw e; // re-throw
  }
};

// Custom error class
class CustomError extends Error {
  constructor(public code: number, message: string) {
    super(message);
    this.name = "CustomError";
  }
}

const throwCustom = () => {
  throw new CustomError(500, "Server error");
};

// Catch with type guard
const handleError = (e: unknown): string => {
  if (e instanceof Error) {
    return e.message;
  }
  return String(e);
};
