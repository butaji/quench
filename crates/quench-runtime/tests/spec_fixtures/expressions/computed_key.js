// Computed property keys
const key = "dynamicKey";
const obj = { [key]: "value", [1 + 1]: "two" };
obj.dynamicKey === "value" && obj["2"] === "two";
