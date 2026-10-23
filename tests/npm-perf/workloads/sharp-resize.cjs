const sharp = require("sharp");

const rounds = Number(process.env.QUENCH_BENCH_ROUNDS || 5000);
const warmups = Number(process.env.QUENCH_BENCH_WARMUPS || 250);
const width = 256;
const height = 256;
const channels = 3;
const input = Buffer.allocUnsafe(width * height * channels);
for (let index = 0; index < input.length; index += 1) input[index] = (index * 29 + 17) & 0xff;

async function resize() {
  const output = await sharp(input, { raw: { width, height, channels } })
    .resize(96, 96, { fit: "cover" })
    .webp({ quality: 80, effort: 4 })
    .toBuffer();
  if (output.length < 16 || output[0] !== 0x52 || output[1] !== 0x49) {
    throw new Error("sharp WebP output mismatch");
  }
  return output;
}

async function main() {
  for (let index = 0; index < warmups; index += 1) await resize();
  let checksum = 0;
  let bytes = 0;
  const start = Date.now();
  for (let index = 0; index < rounds; index += 1) {
    const output = await resize();
    bytes += output.length;
    checksum = (checksum + output[index % output.length]) >>> 0;
  }
  const elapsedMs = Math.max(1, Date.now() - start);
  console.log(JSON.stringify({
    workload: "sharp-resize",
    package: `sharp@${sharp.versions.sharp}`,
    rounds,
    warmups,
    cases: ["raw-rgb-input", "resize-256-to-96", "webp-encode"],
    bytes,
    checksum,
    elapsedMs,
    operationsPerSecond: rounds * 1000 / elapsedMs,
  }));
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
