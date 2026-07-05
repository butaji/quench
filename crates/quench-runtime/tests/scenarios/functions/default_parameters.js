// Default parameters
// ECMA-262 sec-function-definitions-runtime-semantics

function greet(name, greeting) {
    greeting = greeting !== undefined ? greeting : "Hello";
    return greeting + " " + name;
}
greet("World");
