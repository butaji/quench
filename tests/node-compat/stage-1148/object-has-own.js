const object = { value: 1 };
if (!Object.hasOwn(object, "value")) throw new Error("own property was missed");
if (Object.hasOwn(Object.create(object), "value")) {
  throw new Error("inherited property was reported as own");
}
