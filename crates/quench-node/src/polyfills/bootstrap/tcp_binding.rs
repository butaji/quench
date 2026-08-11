//! Polyfill: `tcp-binding`

pub const JS: &str = quench_js_check::checked_js!(r#"globalThis.__quenchTcpBinding = () => {
  const TCP = class TCP {
    constructor() {
      this.fd = 0;
    }
    setNoDelay() {}
    listen() {
      this.fd = 2;
      return 0;
    }
    close() {
      this.fd = -1;
    }
  };
  return { TCP, TCPWrap: TCP, constants: { SOCKET: 1 } };
};
"#);
