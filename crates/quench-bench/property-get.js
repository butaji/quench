const receiver = { first: 1, second: 2, hot: 3, fourth: 4 };
let checksum = 0;

new BenchmarkSuite("PropertyGet", 1, [
  new Benchmark("PropertyGet", function () {
    for (let index = 0; index < 100000; index++) checksum += receiver.hot;
  })
]);
