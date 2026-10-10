const pattern = /(?:)/gu;
Object.defineProperty(pattern, "unicode", { value: false });
console.log("before replace");
console.log(JSON.stringify("A😀B".replace(pattern, "-")));
