const fs = require("fs");

fs.opendir(process.cwd(), (error, dir) => {
  if (error) throw error;
  if (!(dir instanceof fs.Dir)) throw new Error("missing Dir");
  dir.read((readError, entry) => {
    if (readError) throw readError;
    if (entry === undefined) throw new Error("missing read result");
    dir.close((closeError) => {
      if (closeError) throw closeError;
      console.log("fs opendir callback lifecycle passed");
    });
  });
});
