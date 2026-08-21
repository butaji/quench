module.exports = {
  start: function () {
    return { close: function () {}, prompt: function () {}, context: {} };
  },
  REPLServer: function () {
    return module.exports.start();
  }
};
