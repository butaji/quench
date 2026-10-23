const sharp = require("sharp");

const rounds = Number(process.env.QUENCH_BENCH_ROUNDS || 1000);
const warmups = Number(process.env.QUENCH_BENCH_WARMUPS || 100);
const concurrency = Number(process.env.QUENCH_BENCH_CONCURRENCY || 4);
const width = 256;
const height = 256;
const channels = 3;
const input = Buffer.allocUnsafe(width * height * channels);
for (let index = 0; index < input.length; index += 1) input[index] = (index * 29 + 17) & 0xff;

async function resize(seed) {
  const output = await sharp(input, { raw: { width, height, channels } })
    .resize(96, 96, { fit: "cover" })
    .webp({ quality: 80, effort: 4 })
    .toBuffer();
  if (output.length < 16 || output[0] !== 0x52 || output[1] !== 0x49) {
    throw new Error("sharp WebP output mismatch");
  }
  return output;
}

async function batch(seed) {
  return Promise.all(Array.from({ length: concurrency }, (_, index) => resize(seed + index)));
}

async function main() {
  for (let index = 0; index < warmups; index += 1) await batch(index * concurrency);
  let checksum = 0;
  let bytes = 0;
  const start = Date.now();
  for (let index = 0; index < rounds; index += 1) {
    const outputs = await batch(index * concurrency);
    for (let offset = 0; offset < outputs.length; offset += 1) {
      const output = outputs[offset];
      bytes += output.length;
      checksum = (checksum + output[(index + offset) % output.length]) >>> 0;
    }
  }
  const elapsedMs = Math.max(1, Date.now() - start);
  const operations = rounds * concurrency;
  console.log(JSON.stringify({
    workload: "sharp-resize-concurrent",
    package: `sharp@${sharp.versions.sharp}`,
    rounds,
    concurrency,
    warmups,
    cases: ["raw-rgb-input", "resize-256-to-96", "webp-encode", "parallel-toBuffer"],
    operations,
    bytes,
    checksum,
    elapsedMs,
    operationsPerSecond: operations * 1000 / elapsedMs,
  }));
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
