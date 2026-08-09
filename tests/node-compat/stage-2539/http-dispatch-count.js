const http = require("http");
let count = 0;
const server = http.createServer((req, res) => {
  count++;
  res.end("ok");
});
server.listen(0, () => {
  const req = http.get({ port: server.address().port, path: "/" }, (res) => {
    res.resume();
    res.on("end", () => {
      setTimeout(() => {
        if (count !== 1) throw new Error(`count=${count}`);
        server.close();
      }, 5);
    });
  });
  req.on("error", (error) => {
    throw error;
  });
});
