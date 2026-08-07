import {
  mustCall,
  mustNotMutateObjectDeep,
} from "../../../tests/node/test/common/index.mjs";
import { nextdir } from "../../../tests/node/test/common/fs.js";
import assert from "node:assert";
import { cp } from "node:fs";
import tmpdir from "../../../tests/node/test/common/tmpdir.js";
import fixtures from "../../../tests/node/test/common/fixtures.js";

tmpdir.refresh();
const src = fixtures.path("copy/kitchen-sink");
const dest = nextdir();
cp(
  src,
  dest,
  mustNotMutateObjectDeep({ recursive: true }),
  mustCall((error) => {
    assert.strictEqual(error, null);
    console.log("esm common cp setup passed");
  }),
);
