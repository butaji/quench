const { internalBinding } = require("internal/test/binding");
internalBinding("os").getHomeDirectory = (context) => {
  context.syscall = "foo";
  context.code = "bar";
  context.message = "baz";
};
try {
  require("os").homedir();
  throw new Error("os.homedir did not report the system error");
} catch (error) {
  if (error.message !== "A system error occurred: foo returned bar (baz)") {
    throw error;
  }
}
