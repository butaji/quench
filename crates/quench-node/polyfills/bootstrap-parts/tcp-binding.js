globalThis.__quenchTcpBinding = () => {
  const TCP = class TCP {
    setNoDelay() {}
  };
  return { TCP, TCPWrap: TCP };
};
