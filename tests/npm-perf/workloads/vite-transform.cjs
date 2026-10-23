const { transformWithOxc } = require("vite");

const rounds = Number(process.env.QUENCH_BENCH_ROUNDS || 5000);
const warmups = Number(process.env.QUENCH_BENCH_WARMUPS || 250);
const source = `
export interface Product { id: string; title: string; price: number }
export type ProductView = Pick<Product, "id" | "title"> & { label: string };
export function renderProducts(items: Product[], currency = "USD"): ProductView[] {
  return items.filter((item) => item.price >= 0).map((item) => ({
    id: item.id,
    title: item.title,
    label: new Intl.NumberFormat("en-US", { style: "currency", currency }).format(item.price),
  }));
}
export const ProductList = ({ items }: { items: Product[] }) => <ul>
  {items.map((item) => <li key={item.id}>{item.title}: {item.price}</li>)}
</ul>;
`;

function checksum(text, seed) {
  let value = seed >>> 0;
  for (let index = 0; index < text.length; index += 1) {
    value = Math.imul(value ^ text.charCodeAt(index), 16777619) >>> 0;
  }
  return value;
}

async function transform() {
  const result = await transformWithOxc(source, "src/ProductList.tsx", {
    lang: "tsx",
    target: "es2022",
    sourcemap: false,
  });
  if (!result.code.includes("renderProducts") || !result.code.includes("jsx")) {
    throw new Error("Vite TSX transform output mismatch");
  }
  return result.code;
}

async function main() {
  for (let index = 0; index < warmups; index += 1) await transform();
  let hash = 2166136261;
  let bytes = 0;
  const start = Date.now();
  for (let index = 0; index < rounds; index += 1) {
    const code = await transform();
    bytes += Buffer.byteLength(code);
    hash = checksum(code, hash);
  }
  const elapsedMs = Math.max(1, Date.now() - start);
  console.log(JSON.stringify({
    workload: "vite-transform",
    package: "vite@8.3.5",
    rounds,
    warmups,
    cases: ["warm-tsx-transform", "typescript-erasure", "jsx-automatic"],
    bytes,
    checksum: hash,
    elapsedMs,
    operationsPerSecond: rounds * 1000 / elapsedMs,
  }));
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
