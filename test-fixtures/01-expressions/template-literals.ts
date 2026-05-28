// ============================================================================
// TEMPLATE LITERALS
// ============================================================================

const name = "World";
const age = 30;
const user = { first: "John", last: "Doe" };

// Basic template literal
const greeting = `Hello, ${name}!`;

// Expression interpolation
const add = `1 + 2 = ${1 + 2}`;
const funcResult = `Result: ${Math.sqrt(16)}`;
const conditional = `Age is ${age > 18 ? "adult" : "minor"}`;

// Multi-line template
const multiline = `
  This is
  a multi-line
  template string
`;

// Nested templates
const nested = `Result: ${`inner ${name}`}`;

// With functions
const sayHello = (n: string) => `Hello, ${n}!`;
const fnResult = `${sayHello("World")}`;

// With array methods
const items = ["apple", "banana", "cherry"];
const list = `Fruits: ${items.map(i => `- ${i}`).join("\n")}`;

// Tagged template literal
function tag(strings: TemplateStringsArray, ...values: any[]) {
  let result = "";
  for (let i = 0; i < strings.length; i++) {
    result += strings[i];
    if (i < values.length) {
      result += String(values[i]);
    }
  }
  return result;
}

const tagged = tag`Hello ${name}, you are ${age} years old.`;

// HTML escaping with tagged template
const html = (strings: TemplateStringsArray, ...values: string[]): string => {
  return strings.reduce((acc, str, i) => {
    const value = values[i] ? values[i].replace(/</g, "&lt;") : "";
    return acc + str + value;
  }, "");
};

const safeHtml = html`<div>${user.first}</div>`;

// Escape sequences in templates
const escaped = `\\ \` \$ \n \t`;
