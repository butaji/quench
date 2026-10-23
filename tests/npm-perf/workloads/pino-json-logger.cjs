const pino = require("pino");

const rounds = Number(process.env.QUENCH_BENCH_ROUNDS || 5000);
const warmups = Number(process.env.QUENCH_BENCH_WARMUPS || 250);
let writes = 0;
let bytes = 0;
let checksum = 0;

const sink = {
  write(chunk) {
    if (writes === 0) {
      const record = typeof chunk === "string" ? chunk : chunk.toString("utf8");
      if (!record.includes("[Redacted]") || record.includes("secret-token") || record.includes("password-value")) {
        throw new Error(`Pino redaction mismatch: ${record}`);
      }
      if (!record.includes("\"method\":\"GET\"") || !record.includes("\"msg\":\"request completed\"")) {
        throw new Error(`Pino serializer/output mismatch: ${record}`);
      }
    }
    const length = typeof chunk === "string" ? Buffer.byteLength(chunk) : chunk.byteLength;
    const sample = length === 0 ? 0 : typeof chunk === "string"
      ? chunk.charCodeAt(writes % length)
      : chunk[writes % length];
    bytes += length;
    checksum = (checksum + sample) >>> 0;
    writes += 1;
    return true;
  },
};

const logger = pino({
  level: "info",
  timestamp: false,
  base: null,
  redact: {
    paths: ["user.password", "req.headers.authorization"],
    censor: "[Redacted]",
  },
  serializers: {
    req(request) {
      return {
        method: request.method,
        url: request.url,
        headers: request.headers,
      };
    },
  },
}, sink);

function logRequest(index) {
  logger.info({
    req: {
      method: "GET",
      url: `/records/${index & 63}`,
      headers: { authorization: "secret-token", "content-type": "application/json" },
    },
    user: { id: `user-${index & 31}`, password: "password-value" },
    elapsedMs: index % 9,
    itemCount: 3,
  }, "request completed");
}

for (let index = 0; index < warmups; index += 1) logRequest(index);
writes = 0;
bytes = 0;
checksum = 0;
const start = Date.now();
for (let index = 0; index < rounds; index += 1) logRequest(index);
const elapsedMs = Math.max(1, Date.now() - start);
if (writes !== rounds) throw new Error(`Pino sink received ${writes} records for ${rounds} operations`);
console.log(JSON.stringify({
  workload: "pino-json-logger",
  package: "pino@10.4.0",
  rounds,
  warmups,
  cases: ["structured-json", "request-serializer", "redaction"],
  bytes,
  checksum,
  elapsedMs,
  operationsPerSecond: rounds * 1000 / elapsedMs,
}));
