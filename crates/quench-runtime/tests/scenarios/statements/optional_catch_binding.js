// Test optional-catch-binding: try/catch without catch parameter
// ECMA-262 sec-try-statement
// Feature: optional-catch-binding

var caught = false;
try {
    throw 42;
} catch {
    caught = true;
}
caught;
