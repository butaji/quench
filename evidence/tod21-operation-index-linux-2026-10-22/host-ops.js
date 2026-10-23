const out = [];
const fs = require("fs"), path = require("path"), os = require("os"), crypto = require("crypto"), zlib = require("zlib");
const { execFileSync } = require("child_process");
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "q-"));
fs.writeFileSync(path.join(tmp, "a.txt"), "hello");
out.push(fs.readFileSync(path.join(tmp, "a.txt"), "utf8"), fs.statSync(tmp).isDirectory(), fs.readdirSync(tmp).join());
out.push(crypto.createHash("sha256").update("x").digest("hex").slice(0, 16), crypto.randomBytes(4).length);
out.push(zlib.gunzipSync(zlib.gzipSync("abc")).toString());
out.push(new URL("https://a.b/c?d=1").searchParams.get("d"), typeof process.hrtime.bigint(), process.cwd() === process.cwd());
out.push(typeof process.memoryUsage().rss, process.uptime() >= 0, typeof process.getuid(), process.umask() >= 0);
out.push(execFileSync("echo", ["child"]).toString().trim());
const { EventEmitter } = require("events"); const e = new EventEmitter(); e.on("x", v => out.push("ev" + v)); e.emit("x", 1);
const { Readable } = require("stream"); Readable.from(["s1", "s2"]).on("data", d => out.push(String(d))).on("end", () => {
  setTimeout(() => {
    const http = require("http");
    const srv = http.createServer((q, r) => r.end("ok")).listen(0, () => {
      http.get({ port: srv.address().port }, res => { let b = ""; res.on("data", c => b += c); res.on("end", () => {
        out.push("http:" + b); srv.close();
        const { Worker } = require("worker_threads");
        const w = new Worker("require('worker_threads').parentPort.postMessage(7)", { eval: true });
        w.on("message", m => { out.push("worker:" + m); w.terminate().then(() => { fs.rmSync(tmp, { recursive: true }); console.log(JSON.stringify(out)); }); });
      }); });
    });
  }, 5);
});
