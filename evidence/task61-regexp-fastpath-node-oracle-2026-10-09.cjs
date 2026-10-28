const results = [];

function record(name, operation) {
  try {
    results.push({ name, value: operation() });
  } catch (error) {
    results.push({ name, error: error.name });
  }
  console.error(name);
}

record("global-zero-capture", () => "The quick brown fox".replace(/o/g, "0"));
record("replacement-tokens", () => "abc".replace(/b/g, "$$-$&-$`-$'-$1"));
record("no-match-last-index", () => {
  const pattern = /z/g;
  pattern.lastIndex = 2;
  const value = "abc".replace(pattern, "x");
  return { value, lastIndex: pattern.lastIndex };
});
record("empty-global-unicode", () => {
  const pattern = /(?:)/gu;
  pattern.lastIndex = 2;
  const value = "A😀B".replace(pattern, "-");
  return { value, lastIndex: pattern.lastIndex };
});
record("empty-global-sticky-unicode", () => {
  const pattern = /(?:)/guy;
  const value = "A😀B".replace(pattern, "-");
  return { value, lastIndex: pattern.lastIndex };
});
record("capture-fallback", () => "abcabc".replace(/(b)/g, "<$1:$&>"));
record("callback-fallback", () => {
  let calls = 0;
  const value = "foo".replace(/o/g, (match, offset, input) => {
    calls++;
    return `${match}${offset}:${input.length}`;
  });
  return { value, calls };
});
record("own-exec-fallback", () => {
  const pattern = /o/g;
  pattern.exec = () => null;
  return "foo".replace(pattern, "x");
});
record("own-flags-fallback", () => {
  const pattern = /o/g;
  Object.defineProperty(pattern, "flags", { value: "" });
  return "foo".replace(pattern, "x");
});
record("own-global-fallback", () => {
  const pattern = /o/g;
  Object.defineProperty(pattern, "global", { value: false });
  const value = "foo".replace(pattern, "x");
  return { value, lastIndex: pattern.lastIndex };
});
record("own-unicode-fallback", () => {
  const pattern = /(?:)/gu;
  Object.defineProperty(pattern, "unicode", { value: false });
  return "AB".replace(pattern, "-");
});
record("nonwritable-last-index", () => {
  const pattern = /o/g;
  Object.defineProperty(pattern, "lastIndex", { writable: false });
  return "foo".replace(pattern, "x");
});

console.log(JSON.stringify(results));
