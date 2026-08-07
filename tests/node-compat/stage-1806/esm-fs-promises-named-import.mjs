import { cp, mkdir, writeFile } from "node:fs/promises";

if (
  typeof cp !== "function" || typeof mkdir !== "function" ||
  typeof writeFile !== "function"
) {
  throw new Error("fs/promises named exports missing");
}
