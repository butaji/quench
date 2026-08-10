import("prettier").then(async (prettier) => {
  const formatted = await prettier.format("const answer=42", {
    parser: "babel",
  });
  if (formatted !== "const answer = 42;\n") {
    throw new Error(`unexpected output: ${formatted}`);
  }
  console.log("npm prettier application passed");
});
