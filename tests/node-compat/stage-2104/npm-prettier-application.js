import("prettier").then(async (prettier) => {
  if (typeof prettier.format !== "function") throw new Error("format missing");
  if (typeof prettier.check !== "function") throw new Error("check missing");
  if (typeof prettier.default?.format !== "function") {
    throw new Error("default format missing");
  }
  if (typeof prettier.version !== "string") throw new Error("version missing");
  console.log("npm prettier application passed");
});
