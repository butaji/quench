const event = new CustomEvent("data", { detail: 42 });
if (event.detail !== 42) {
  throw new Error("CustomEvent detail was not preserved");
}
try {
  event.detail = 99;
} catch (_) {}
if (event.detail !== 42) throw new Error("CustomEvent detail was mutable");

try {
  new CustomEvent(Symbol("invalid"));
  throw new Error("CustomEvent accepted a symbol type");
} catch (error) {
  if (!(error instanceof TypeError)) throw error;
}
