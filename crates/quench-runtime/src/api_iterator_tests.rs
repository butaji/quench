use super::{Engine, Runtime};
use crate::Host;
use std::{cell::RefCell, rc::Rc};

#[derive(Clone, Default)]
struct Capture(Rc<RefCell<Vec<String>>>);

impl Host for Capture {
    fn write_line(&mut self, text: &str) {
        self.0.borrow_mut().push(text.into());
    }

    fn clock_millis(&mut self) -> f64 {
        0.0
    }
}

#[test]
fn iterator_map_proxy_accesses_only_the_iterator_protocol_properties() {
    let source = r#"
            const handlerProxy = log => new Proxy({}, {
              get: (target, key, receiver) => (...args) => {
                const target = args[0];
                const item = Reflect[key](...args);
                log.push(`${key}: ${args.filter(x => typeof x != 'object').map(x => x.toString())}`);
                switch (typeof item) {
                  case 'function': return item.bind(new Proxy(target, handlerProxy(log)));
                  case 'object': return new Proxy(item, handlerProxy(log));
                  default: return item;
                }
              },
            });
            const log = [];
            const iterator = Object.setPrototypeOf({
              next: function() {
                if (this.value < 3) return { done: false, value: this.value++ };
                return { done: true, value: undefined };
              },
              value: 0,
            }, Iterator.prototype);
            const iteratorProxy = new Proxy(iterator, handlerProxy(log));
            const mappedProxy = iteratorProxy.map(x => x);
            for (const item of mappedProxy) {}
            for (const line of log) print(line);
            "#;

    let expected = [
        "get: map",
        "get: next",
        "get: value",
        "get: value",
        "getOwnPropertyDescriptor: value",
        "has: enumerable",
        "get: enumerable",
        "has: configurable",
        "get: configurable",
        "has: value",
        "get: value",
        "has: writable",
        "get: writable",
        "has: get",
        "has: set",
        "defineProperty: value",
        "set: value,1",
        "get: value",
        "get: value",
        "getOwnPropertyDescriptor: value",
        "has: enumerable",
        "get: enumerable",
        "has: configurable",
        "get: configurable",
        "has: value",
        "get: value",
        "has: writable",
        "get: writable",
        "has: get",
        "has: set",
        "defineProperty: value",
        "set: value,2",
        "get: value",
        "get: value",
        "getOwnPropertyDescriptor: value",
        "has: enumerable",
        "get: enumerable",
        "has: configurable",
        "get: configurable",
        "has: value",
        "get: value",
        "has: writable",
        "get: writable",
        "has: get",
        "has: set",
        "defineProperty: value",
        "set: value,3",
        "get: value",
    ];
    for (mode, compile) in [
        ("specialized", Engine::specialize as fn(&str, &str) -> _),
        ("unspecialized", Engine::specialize_unspecialized),
    ] {
        let host = Capture::default();
        let view = host.clone();
        let mut runtime = Runtime::new(host);
        let program = compile(source, "iterator-map-proxy.js").unwrap();
        if let Err(error) = runtime.execute(&program) {
            panic!("{mode}: {}", runtime.format_error(&program, &error));
        }
        assert_eq!(view.0.borrow().as_slice(), expected, "{mode}");
    }
}
