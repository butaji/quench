//! The host's read-turn queue and half-close boundary are invisible to guest suites.

use super::*;
use std::{
    io::Write,
    net::TcpListener,
    time::{Duration, Instant},
};

const TEST_IO_TIMEOUT: Duration = Duration::from_secs(5);

fn connected_socket() -> (Rc<RefCell<NetSocket>>, TcpStream) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let peer = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    peer.set_write_timeout(Some(TEST_IO_TIMEOUT)).unwrap();
    peer.set_read_timeout(Some(TEST_IO_TIMEOUT)).unwrap();
    let (stream, _) = listener.accept().unwrap();
    stream.set_nonblocking(true).unwrap();
    let socket = NetSocket {
        id: 0,
        process_scope: 0,
        owner_worker: None,
        stream: Some(stream),
        js: Value::Undefined,
        state: SocketState::Open,
        refed: true,
        server_id: None,
        write_buf: Vec::new(),
        write_offset: 0,
        read_buf: Vec::new(),
        bytes_read: 0,
        bytes_written: 0,
        read_eof: false,
        close_emitted: false,
        close_deferred: false,
        write_shutdown_pending: false,
        finish_emitted: false,
        connect_announced: true,
        peer: None,
        local: None,
        encoding: None,
        decode_buf: Vec::new(),
    };
    (Rc::new(RefCell::new(socket)), peer)
}

#[test]
fn queued_read_turns_preserve_identity_byte_order_and_pending_eof_writes() {
    let (socket, mut peer) = connected_socket();
    let mut queued = Vec::new();
    assert!(!read_available(
        &socket,
        &mut socket.borrow_mut(),
        &mut queued
    ));
    assert!(queued.is_empty());
    assert_eq!(socket.borrow().state, SocketState::Open);
    let payload = [
        vec![b'A'; READ_CHUNK],
        vec![b'B'; READ_CHUNK],
        vec![b'C'; READ_CHUNK],
    ]
    .concat();
    peer.write_all(&payload).unwrap();
    peer.shutdown(Shutdown::Write).unwrap();
    let deadline = Instant::now() + TEST_IO_TIMEOUT;
    let mut received = Vec::new();
    loop {
        assert!(
            Instant::now() < deadline,
            "socket read-turn deadline expired"
        );
        let eof = read_available(&socket, &mut socket.borrow_mut(), &mut queued);
        // Each turn yields to observers after one bounded kernel chunk.
        assert!(queued.len() <= 1);
        for (owner, bytes) in queued.drain(..) {
            assert!(Rc::ptr_eq(&owner, &socket));
            assert!(!bytes.is_empty() && bytes.len() <= READ_CHUNK);
            received.extend(bytes);
        }
        assert_eq!(socket.borrow().bytes_read, received.len() as u64);
        if eof {
            break;
        }
        std::thread::yield_now();
    }
    assert_eq!(received, payload);
    {
        let mut socket = socket.borrow_mut();
        assert_eq!(socket.state, SocketState::Closing);
        assert!(socket.read_eof);
        assert!(!socket.write_shutdown_pending);
        // End observers retain the live write side until the pump handles FIN.
        socket.stream.as_mut().unwrap().write_all(b"ACK").unwrap();
    }
    let mut reply = [0; b"ACK".len()];
    peer.read_exact(&mut reply).unwrap();
    assert_eq!(&reply, b"ACK");
    assert!(!read_available(
        &socket,
        &mut socket.borrow_mut(),
        &mut queued
    ));
    assert!(queued.is_empty());
}
