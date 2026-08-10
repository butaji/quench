__quenchNetSocket.prototype.setTypeOfService = function (value) {
  return __quenchSetTypeOfService(this, value);
};
__quenchNetSocket.prototype.getTypeOfService = function () {
  return this._typeOfService;
};
__quenchNetSocket.prototype.ref = function () {
  this._refed = true;
  this._timeoutTimer?.ref?.();
  return this;
};
__quenchNetSocket.prototype.unref = function () {
  this._refed = false;
  this._timeoutTimer?.unref?.();
  return this;
};
__quenchNetSocket.prototype.hasRef = function () {
  return this._refed;
};
__quenchNetSocket.prototype.cork = function () {
  this._corked++;
  return this;
};
__quenchNetSocket.prototype.uncork = function () {
  this._corked = Math.max(0, this._corked - 1);
  return this;
};
__quenchNetSocket.prototype.address = function () {
  if (this._nativeId) {
    return {
      address: this.localAddress || "127.0.0.1",
      family: "IPv4",
      port: this.localPort,
    };
  }
  return this.destroyed ? null : undefined;
};
