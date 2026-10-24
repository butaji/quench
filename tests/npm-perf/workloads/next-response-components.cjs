const rounds = Number(process.env.QUENCH_BENCH_ROUNDS || 5000);
const warmups = Number(process.env.QUENCH_BENCH_WARMUPS || 250);
const selected = process.env.QUENCH_RESPONSE_CASE || "json";
const payload = {
  route: 2,
  params: { id: "8421" },
  result: { status: "ready", records: 12 },
};
const serialized = JSON.stringify(payload);
const bytes = Buffer.byteLength(serialized);
const encoder = new TextEncoder();

function make() {
  switch (selected) {
    case "stringify":
      return JSON.stringify(payload);
    case "text-response":
      return new Response(serialized, { status: 200 });
    case "bytes-response":
      return new Response(encoder.encode(serialized), {
        status: 200,
        headers: { "content-type": "application/json" },
      });
    case "json":
      return Response.json(payload, { status: 200 });
    default:
      throw new Error(`unknown QUENCH_RESPONSE_CASE: ${selected}`);
  }
}

function verify(value) {
  if (selected === "stringify") {
    if (value !== serialized) throw new Error("JSON stringify output mismatch");
    return;
  }
  if (value.status !== 200 || !value.body) throw new Error("Response metadata mismatch");
  const contentType = value.headers.get("content-type");
  if (selected !== "text-response" && !contentType?.startsWith("application/json")) {
    throw new Error("JSON Response content type mismatch");
  }
}

async function main() {
  for (let index = 0; index < warmups; index += 1) verify(make());
  let last;
  const start = Date.now();
  for (let index = 0; index < rounds; index += 1) {
    last = make();
    verify(last);
  }
  const elapsedMs = Math.max(1, Date.now() - start);
  const body = selected === "stringify" ? last : await last.text();
  if (body !== serialized) throw new Error("Response body mismatch");
  console.log(JSON.stringify({
    workload: "next-response-components",
    package: "next@16.4.3",
    rounds,
    warmups,
    cases: [selected],
    bytes: bytes * rounds,
    elapsedMs,
    operationsPerSecond: rounds * 1000 / elapsedMs,
  }));
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
