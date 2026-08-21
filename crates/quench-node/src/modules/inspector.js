(function () {
  function Session() { this.connected = false; }
  Session.prototype.connect = function () { this.connected = true; };
  Session.prototype.connectToMainThread = function () { this.connected = true; };
  Session.prototype.disconnect = function () { this.connected = false; };
  Session.prototype.post = function (method, params, callback) {
    if (!this.connected) {
      var error = new Error('Session is not connected');
      if (typeof callback === 'function') callback(error);
      return;
    }
    if (typeof callback === 'function') callback(null, { method: method, params: params || {} });
  };
  module.exports = {
    Session: Session,
    open: function () { var session = new Session(); session.connect(); return session; },
    close: function () {},
    url: function () { return undefined; },
    waitForDebugger: function () {},
    console: {}
  };
}());