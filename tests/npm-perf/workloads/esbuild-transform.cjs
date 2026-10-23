const esbuild = require("esbuild");

const rounds = Number(process.env.QUENCH_BENCH_ROUNDS || 5000);
const warmups = Number(process.env.QUENCH_BENCH_WARMUPS || 250);
const source = `
export interface Event { id: number; type: "created" | "updated"; payload: Record<string, unknown> }
export const summarize = (events: Event[]) => events.reduce((counts, event) => {
  counts[event.type] = (counts[event.type] ?? 0) + 1;
  return counts;
}, { created: 0, updated: 0 });
export const render = (event: Event) => <article data-id={event.id}>
  <strong>{event.type}</strong><pre>{JSON.stringify(event.payload)}</pre>
</article>;
`;

function checksum(text, seed) {
  let value = seed >>> 0;
  for (let index = 0; index < text.length; index += 1) {
    value = Math.imul(value ^ text.charCodeAt(index), 16777619) >>> 0;
  }
  return value;
}

function transform() {
  const result = esbuild.transformSync(source, {
    loader: "tsx",
    target: "es2022",
    jsx: "automatic",
    minify: true,
  });
  if (!result.code.includes("summarize") || !result.code.includes("jsx")) {
    throw new Error("esbuild TSX transform output mismatch");
  }
  return result.code;
}

function main() {
  for (let index = 0; index < warmups; index += 1) transform();
  let hash = 2166136261;
  let bytes = 0;
  const start = Date.now();
  for (let index = 0; index < rounds; index += 1) {
    const code = transform();
    bytes += Buffer.byteLength(code);
    hash = checksum(code, hash);
  }
  const elapsedMs = Math.max(1, Date.now() - start);
  console.log(JSON.stringify({
    workload: "esbuild-transform",
    package: `esbuild@${esbuild.version}`,
    rounds,
    warmups,
    cases: ["sync-tsx-transform", "jsx-lowering", "minification"],
    bytes,
    checksum: hash,
    elapsedMs,
    operationsPerSecond: rounds * 1000 / elapsedMs,
  }));
}

try {
  main();
} catch (error) {
  console.error(error);
  process.exitCode = 1;
}
