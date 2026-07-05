// Delete operator
const obj = { a: 1, b: 2 };
const deleted = delete obj.a;
deleted === true && obj.a === undefined;
