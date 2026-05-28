// ============================================================================
// OPTIONAL CHAINING & NULLISH COALESCING
// ============================================================================

interface User {
  name: string;
  address?: {
    city: string;
    zip?: string;
  };
  friends?: User[];
}

const user: User = {
  name: "Alice",
  address: {
    city: "NYC"
  }
};

// Optional chaining - property access
const city = user?.address?.city;
const zip = user?.address?.zip;  // undefined (not error)

// Optional chaining - method calls
const maybeFn: (() => void) | undefined = undefined;
maybeFn?.();  // safe call, won't throw

// Optional chaining - bracket notation
const key = "address";
const addr = user?.[key];

// Optional chaining - with array
const arr: number[] | undefined = undefined;
const first = arr?.[0];

// Nullish coalescing
const nullVal: string | null = null;
const undefinedVal: string | undefined = undefined;
const actualVal = "hello";

const result1 = nullVal ?? "default";     // "default"
const result2 = undefinedVal ?? "default"; // "default"
const result3 = actualVal ?? "default";    // "hello"

// Combining optional chaining and nullish coalescing
const userCity = user?.address?.city ?? "Unknown";
const userZip = user?.address?.zip ?? "N/A";

// Comparison with || (important difference)
const falsy = 0;
const emptyStr = "";
const falseVal = false;

const orResult1 = falsy || "default";      // "default" (0 is falsy)
const nullishResult1 = falsy ?? "default"; // 0 (only null/undefined triggers)

const orResult2 = emptyStr || "default";   // "default"
const nullishResult2 = emptyStr ?? "default"; // "" (empty string is NOT nullish)

const orResult3 = falseVal || "default";   // "default"
const nullishResult3 = falseVal ?? "default"; // false (false is NOT nullish)

// Chaining
const nested = {
  level1: {
    level2: {
      value: 42
    }
  }
};

const deepValue = nested?.level1?.level2?.value ?? 0;
const missingValue = nested?.nonexistent?.value ?? "N/A";

// With array methods
const maybeArr: number[] | undefined = [1, 2, 3];
const sum = maybeArr?.reduce((a, b) => a + b, 0) ?? 0;
