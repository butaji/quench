globalThis.__quenchTcpBinding = () => {
  const TCP = class TCP {
    constructor() {
      this.fd = 0;
    }
    setNoDelay() {}
    listen() {}
    close() {}
  };
  return { TCP, TCPWrap: TCP, constants: { SOCKET: 1 } };
};
