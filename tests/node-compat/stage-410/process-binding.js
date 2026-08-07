let error;
try {
  process.binding("test");
} catch (caught) {
  error = caught;
}
if (!error || !/No such module: test/.test(error.message)) {
  throw new Error("process.binding must reject unknown modules");
}

console.log("process binding passed");
