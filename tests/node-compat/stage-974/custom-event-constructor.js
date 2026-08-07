const event = new CustomEvent("data", {
  detail: { value: 42 },
  cancelable: true,
});
if (event.detail.value !== 42) throw new Error("CustomEvent detail is missing");
if (!event.cancelable || event.defaultPrevented) {
  throw new Error("CustomEvent flags are incorrect");
}
event.preventDefault();
if (!event.defaultPrevented) throw new Error("CustomEvent did not cancel");

try {
  new CustomEvent();
  throw new Error("CustomEvent accepted no type");
} catch (error) {
  if (!(error instanceof TypeError)) throw error;
}
