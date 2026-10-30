//! Actual nonblocking UDP descriptors and atomic datagram request ownership.
use std::{
    collections::VecDeque,
    cell::{Ref, RefCell},
    rc::Rc,
    io,
    net::{SocketAddr, UdpSocket},
};
pub(crate) const IPV6_ONLY: u32 = 1;
pub(crate) const REUSE_ADDR: u32 = super::util_system_error_shared_vm::UV_UDP_REUSEADDR as u32;
pub(crate) const REUSE_PORT: u32 = 64; // libuv UV_UDP_REUSEPORT, deps/uv/include/uv.h.
pub(crate) const DATAGRAM_CAPACITY: usize = u16::MAX as usize;
pub(crate) struct Pending<T> {
    pub(crate) token: T,
    pub(crate) bytes: Vec<u8>,
    pub(crate) destination: Option<SocketAddr>,
}
// Multiple inactive libuv wrappers may adopt one real descriptor. Keep one
// Rust ownership authority; closing either wrapper closes the OS descriptor.
// The other wrapper retains its observed fd number, matching libuv's slot.
pub(crate) struct Descriptor {
    socket: RefCell<Option<UdpSocket>>,
    fd: i32,
}
impl Descriptor {
    fn new(socket: UdpSocket) -> Rc<Self> {
        #[cfg(unix)] let fd = { use std::os::fd::AsRawFd; socket.as_raw_fd() };
        #[cfg(not(unix))] let fd = -libc::EBADF;
        Rc::new(Self { socket: RefCell::new(Some(socket)), fd })
    }
}
pub(crate) struct Endpoint<T> {
    socket: Option<Rc<Descriptor>>,
    pub(crate) receiving: bool,
    pub(crate) referenced: bool,
    pub(crate) pending: VecDeque<Pending<T>>,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SendMode {
    Try,
    Queue,
}
pub(crate) enum Submission<T> {
    Synchronous { token: T, sent: usize },
    Asynchronous,
}
impl<T> Endpoint<T> {
    pub(crate) fn new() -> Self {
        Self {
            socket: None,
            receiving: false,
            referenced: true,
            pending: VecDeque::new(),
        }
    }
    fn socket(&self) -> io::Result<Ref<'_, UdpSocket>> {
        let owner = self.socket.as_ref().ok_or_else(|| io::Error::from_raw_os_error(libc::EBADF))?;
        Ref::filter_map(owner.socket.borrow(), |socket| socket.as_ref())
            .map_err(|_| io::Error::from_raw_os_error(libc::EBADF))
    }
    fn ensure_socket(&mut self, ipv6: bool) -> io::Result<Ref<'_, UdpSocket>> {
        if self.socket.is_some() {
            if self.socket()?.local_addr()?.is_ipv6() != ipv6 {
                return Err(io::Error::from_raw_os_error(libc::EINVAL));
            }
        } else {
            let socket = socket2::Socket::new(
                if ipv6 { socket2::Domain::IPV6 } else { socket2::Domain::IPV4 },
                socket2::Type::DGRAM, Some(socket2::Protocol::UDP),
            )?;
            socket.set_nonblocking(true)?;
            self.socket = Some(Descriptor::new(socket.into()));
        }
        self.socket()
    }
    pub(crate) fn descriptor(&self) -> Option<Rc<Descriptor>> { self.socket.as_ref().filter(|owner| owner.socket.borrow().is_some()).cloned() }
    pub(crate) fn adopt(&mut self, owner: Rc<Descriptor>) -> io::Result<()> {
        if self.socket.is_some() { return Err(io::Error::from_raw_os_error(libc::EBUSY)); }
        {
            let socket=owner.socket.borrow();let socket=socket.as_ref().ok_or_else(||io::Error::from_raw_os_error(libc::EBADF))?;
            socket.set_nonblocking(true)?;socket2::SockRef::from(socket).set_reuse_address(true)?;
        }
        self.socket=Some(owner);Ok(())
    }
    #[cfg(unix)]
    pub(crate) fn open(&mut self, fd:i32)->io::Result<()> {
        use std::os::fd::{BorrowedFd,FromRawFd};
        if self.socket.is_some(){return Err(io::Error::from_raw_os_error(libc::EBUSY));}
        if unsafe{libc::fcntl(fd,libc::F_GETFD)}<0{return Err(io::Error::last_os_error());}
        if super::fd_handle_type::guess(fd)!=super::fd_handle_type::Kind::Udp{return Err(io::Error::from_raw_os_error(libc::EINVAL));}
        // Only borrow before configuration succeeds. Failed adoption never closes
        // an external descriptor that the caller still owns.
        let borrowed=unsafe{BorrowedFd::borrow_raw(fd)};
        let socket=socket2::SockRef::from(&borrowed);
        socket.set_nonblocking(true)?;socket.set_reuse_address(true)?;
        self.socket=Some(Descriptor::new(unsafe{UdpSocket::from_raw_fd(fd)}));Ok(())
    }
    #[cfg(not(unix))]
    pub(crate) fn open(&mut self,_:i32)->io::Result<()>{Err(io::Error::from_raw_os_error(libc::ENOTSUP))}
    pub(crate) fn bind(&mut self, address: SocketAddr, flags: u32) -> io::Result<()> {
        if flags & !(IPV6_ONLY | REUSE_ADDR | REUSE_PORT) != 0 {
            return Err(io::Error::from_raw_os_error(libc::EINVAL));
        }
        let socket = self.ensure_socket(address.is_ipv6())?;
        let socket = socket2::SockRef::from(&*socket);
        if address.is_ipv6() {
            socket.set_only_v6(flags & IPV6_ONLY != 0)?;
        }
        #[cfg(unix)]
        socket.set_reuse_address(flags & REUSE_ADDR != 0)?;
        if flags & REUSE_PORT != 0 {
            #[cfg(any(
                target_os = "linux",
                target_os = "android",
                target_os = "freebsd",
                target_os = "dragonfly",
                target_os = "aix"
            ))]
            socket.set_reuse_port(true)?;
            #[cfg(not(any(
                target_os = "linux",
                target_os = "android",
                target_os = "freebsd",
                target_os = "dragonfly",
                target_os = "aix"
            )))]
            return Err(io::Error::from_raw_os_error(libc::ENOTSUP));
        }
        socket.bind(&address.into())
    }
    pub(crate) fn address(&self, peer: bool) -> io::Result<SocketAddr> {
        let socket = self.socket()?;
        if peer {
            socket.peer_addr()
        } else {
            socket.local_addr()
        }
    }
    pub(crate) fn connect(&mut self, address: SocketAddr) -> io::Result<()> {
        self.ensure_socket(address.is_ipv6())?.connect(address)
    }
    pub(crate) fn recv_start(&mut self) -> io::Result<()> {
        let ipv6 = self.address(false).is_ok_and(|a| a.is_ipv6());
        let socket = self.ensure_socket(ipv6)?;
        if socket.local_addr()?.port() == 0 {
            let address = if ipv6 {
                SocketAddr::from(([0u16; 8], 0))
            } else {
                SocketAddr::from(([0u8; 4], 0))
            };
            socket2::SockRef::from(&*socket).bind(&address.into())?;
        }
        drop(socket);
        self.receiving = true;
        Ok(())
    }
    pub(crate) fn recv_stop(&mut self) {
        self.receiving = false;
    }
    fn send_bytes(&self, bytes: &[u8], destination: Option<SocketAddr>) -> io::Result<usize> {
        let socket = self.socket()?;
        match destination {
            Some(address) => socket.send_to(bytes, address),
            None => socket.send(bytes),
        }
    }
    pub(crate) fn submit(
        &mut self,
        token: T,
        bytes: Vec<u8>,
        destination: Option<SocketAddr>,
        mode: SendMode,
    ) -> Result<Submission<T>, (T, io::Error)> {
        let ipv6 = destination.is_some_and(|a| a.is_ipv6())
            || destination.is_none()
                && self.address(false).is_ok_and(|a| a.is_ipv6());
        if let Err(error) = self.ensure_socket(ipv6) {
            return Err((token, error));
        }
        if mode == SendMode::Try && self.pending.is_empty() {
            match self.send_bytes(&bytes, destination) {
                Ok(sent) => return Ok(Submission::Synchronous { token, sent }),
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ) => {}
                Err(error) => return Err((token, error)),
            }
        }
        self.pending.push_back(Pending {
            token,
            bytes,
            destination,
        });
        Ok(Submission::Asynchronous)
    }
    pub(crate) fn poll_send(&mut self) -> Option<(T, usize, io::Result<usize>)> {
        let pending = self.pending.front()?;
        let result = self.send_bytes(&pending.bytes, pending.destination);
        if matches!(&result,Err(error)if matches!(error.kind(),io::ErrorKind::WouldBlock|io::ErrorKind::Interrupted))
        {
            return None;
        }
        let pending = self
            .pending
            .pop_front()
            .expect("observed native UDP request");
        Some((pending.token, pending.bytes.len(), result))
    }
    pub(crate) fn receive(&self) -> io::Result<Option<(Vec<u8>, SocketAddr)>> {
        if !self.receiving {
            return Ok(None);
        }
        let mut bytes = vec![0; DATAGRAM_CAPACITY];
        let socket = self.socket()?;
        match socket.recv_from(&mut bytes) {
            Ok((length, address)) => {
                bytes.truncate(length);
                Ok(Some((bytes, address)))
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) =>
            {
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }
    pub(crate) fn queue_size(&self) -> usize {
        self.pending.iter().map(|request| request.bytes.len()).sum()
    }
    pub(crate) fn queue_count(&self) -> usize {
        self.pending.len()
    }
    pub(crate) fn has_work(&self) -> bool {
        self.receiving && self.referenced || !self.pending.is_empty()
    }
    #[cfg(unix)]
    pub(crate) fn fd(&self) -> i32 {
        self.socket.as_ref().map_or(-libc::EBADF, |owner| owner.fd)
    }
    #[cfg(not(unix))]
    pub(crate) fn fd(&self) -> i32 {
        -libc::EBADF
    }
}

#[derive(Clone, Copy)]
pub(crate) enum SocketOption { Broadcast, Ttl, MulticastTtl, MulticastLoop }
impl<T> Endpoint<T> {
    pub(crate) fn set_option(&self, kind: SocketOption, value: i32) -> io::Result<()> {
        if matches!(kind,SocketOption::Ttl) && !(1..=u8::MAX as i32).contains(&value)
            || matches!(kind,SocketOption::MulticastTtl|SocketOption::MulticastLoop) && !(0..=u8::MAX as i32).contains(&value) {
            return Err(io::Error::from_raw_os_error(libc::EINVAL));
        }
        let socket=self.socket()?;
        let ipv6=socket.local_addr()?.is_ipv6();let socket=socket2::SockRef::from(&*socket);
        match kind {
            SocketOption::Broadcast=>socket.set_broadcast(value!=0),
            SocketOption::Ttl=>if ipv6 {socket.set_unicast_hops_v6(value as u32)} else {socket.set_ttl_v4(value as u32)},
            SocketOption::MulticastTtl=>if ipv6 {socket.set_multicast_hops_v6(value as u32)} else {socket.set_multicast_ttl_v4(value as u32)},
            SocketOption::MulticastLoop=>if ipv6 {socket.set_multicast_loop_v6(value!=0)} else {socket.set_multicast_loop_v4(value!=0)},
        }
    }
    pub(crate) fn buffer_size(&self,size:u32,recv:bool)->io::Result<usize>{
        let socket=self.socket()?;
        let socket=socket2::SockRef::from(&*socket);
        if size!=0 {if recv {socket.set_recv_buffer_size(size as usize)?}else{socket.set_send_buffer_size(size as usize)?}}
        if recv {socket.recv_buffer_size()}else{socket.send_buffer_size()}
    }
    pub(crate) fn disconnect(&self)->io::Result<()>{
        let socket=self.socket()?;
        #[cfg(unix)] {
            use std::os::fd::AsRawFd;
            #[cfg(target_os = "macos")]
            let address=libc::sockaddr{sa_len:std::mem::size_of::<libc::sockaddr>() as u8,sa_family:libc::AF_UNSPEC as libc::sa_family_t,sa_data:[0;14]};
            #[cfg(not(target_os = "macos"))]
            let address=libc::sockaddr{sa_family:libc::AF_UNSPEC as libc::sa_family_t,sa_data:[0;14]};
            let result=unsafe{libc::connect(socket.as_raw_fd(),&address,std::mem::size_of_val(&address) as libc::socklen_t)};
            if result==0 {Ok(())} else {Err(io::Error::last_os_error())}
        }
        #[cfg(not(unix))] {let _=socket;Err(io::Error::from_raw_os_error(libc::ENOTSUP))}
    }
}

impl<T> Drop for Endpoint<T> {
    fn drop(&mut self) {
        if let Some(owner)=self.socket.take() { owner.socket.borrow_mut().take(); }
    }
}

/// libuv uv_ip[46]_addr, including Unix interface-name scope resolution.
pub(crate) fn numeric_address(text:&str,port:u16,ipv6:bool)->io::Result<SocketAddr>{
    let invalid=||io::Error::from_raw_os_error(libc::EINVAL);
    let (literal,zone)=if ipv6{text.split_once('%').map_or((text,None),|(ip,zone)|(ip,Some(zone)))}else{(text,None)};
    let ip=literal.parse::<std::net::IpAddr>().map_err(|_|invalid())?;
    match ip{
        std::net::IpAddr::V4(ip)if !ipv6=>Ok(SocketAddr::from((ip,port))),
        std::net::IpAddr::V6(ip)if ipv6=>{
            let scope=if let Some(zone)=zone{
                #[cfg(unix)]{std::ffi::CString::new(zone).map_or(0,|name|unsafe{libc::if_nametoindex(name.as_ptr())})}
                #[cfg(not(unix))]{zone.parse::<u32>().unwrap_or(0)}
            }else{0};
            Ok(SocketAddr::V6(std::net::SocketAddrV6::new(ip,port,0,scope)))
        }
        _=>Err(invalid()),
    }
}
#[derive(Clone,Copy)]
pub(crate) enum Membership{Join,Leave}
impl<T> Endpoint<T>{
    fn deferred_bind(&mut self,ipv6:bool)->io::Result<()>{
        let unbound={self.ensure_socket(ipv6)?.local_addr()?.port()==0};
        if unbound{self.bind(if ipv6{SocketAddr::from(([0u16;8],0))}else{SocketAddr::from(([0u8;4],0))},REUSE_ADDR)?;}
        Ok(())
    }
    pub(crate) fn multicast_interface(&self,interface:Option<&str>)->io::Result<()>{
        let address=match interface{
            Some(text)=>numeric_address(text,0,false).or_else(|_|numeric_address(text,0,true))?,
            None=>if self.address(false).is_ok_and(|a|a.is_ipv6()){SocketAddr::from(([0u16;8],0))}else{SocketAddr::from(([0u8;4],0))},
        };
        let socket=self.socket()?;let socket=socket2::SockRef::from(&*socket);
        match address{SocketAddr::V4(address)=>socket.set_multicast_if_v4(address.ip()),SocketAddr::V6(address)=>socket.set_multicast_if_v6(address.scope_id())}
    }
    pub(crate) fn membership(&mut self,group:&str,interface:Option<&str>,source:Option<&str>,action:Membership)->io::Result<()>{
        let group=numeric_address(group,0,false).or_else(|_|numeric_address(group,0,true))?;
        let source=source.map(|text|numeric_address(text,0,group.is_ipv6())).transpose()?;
        self.deferred_bind(group.is_ipv6())?;
        let interface=interface.map(|text|numeric_address(text,0,group.is_ipv6())).transpose()?;
        let socket=self.socket()?;let socket=socket2::SockRef::from(&*socket);
        match group{
            SocketAddr::V4(group)=>{
                let interface=match interface{Some(SocketAddr::V4(a))=>*a.ip(),_=>std::net::Ipv4Addr::UNSPECIFIED};
                if let Some(SocketAddr::V4(source))=source{
                    match action{Membership::Join=>socket.join_ssm_v4(source.ip(),group.ip(),&interface),Membership::Leave=>socket.leave_ssm_v4(source.ip(),group.ip(),&interface)}
                }else{match action{Membership::Join=>socket.join_multicast_v4(group.ip(),&interface),Membership::Leave=>socket.leave_multicast_v4(group.ip(),&interface)}}
            }
            SocketAddr::V6(group)=>{
                let interface=match interface{Some(SocketAddr::V6(a))=>a.scope_id(),_=>0};
                if let Some(SocketAddr::V6(source))=source{
                    #[cfg(target_os="linux")]{
                        use std::os::fd::AsRawFd;
                        #[repr(C)]struct SourceGroup{interface:u32,group:libc::sockaddr_storage,source:libc::sockaddr_storage}
                        let address=|value:std::net::SocketAddrV6|{
                            let mut storage=unsafe{std::mem::zeroed::<libc::sockaddr_storage>()};
                            let address=libc::sockaddr_in6{sin6_family:libc::AF_INET6 as libc::sa_family_t,sin6_port:0,sin6_flowinfo:0,sin6_addr:libc::in6_addr{s6_addr:value.ip().octets()},sin6_scope_id:value.scope_id()};
                            unsafe{std::ptr::write((&mut storage as *mut libc::sockaddr_storage).cast::<libc::sockaddr_in6>(),address)};storage
                        };
                        let request=SourceGroup{interface,group:address(group),source:address(source)};
                        let option=match action{Membership::Join=>libc::MCAST_JOIN_SOURCE_GROUP,Membership::Leave=>libc::MCAST_LEAVE_SOURCE_GROUP};
                        let result=unsafe{libc::setsockopt(socket.as_raw_fd(),libc::IPPROTO_IPV6,option,(&request as *const SourceGroup).cast(),std::mem::size_of_val(&request) as libc::socklen_t)};
                        if result==0{Ok(())}else{Err(io::Error::last_os_error())}
                    }
                    #[cfg(not(target_os="linux"))]{let _=source;Err(io::Error::from_raw_os_error(libc::ENOTSUP))}
                }else{match action{Membership::Join=>socket.join_multicast_v6(group.ip(),interface),Membership::Leave=>socket.leave_multicast_v6(group.ip(),interface)}}
            }
        }
    }
}

/// Node's numeric address projection retains the actual IPv6 interface scope.
pub(crate) fn display_ip(address: SocketAddr) -> String {
    let ip=address.ip().to_string();
    let SocketAddr::V6(address)=address else{return ip};
    let index=address.scope_id();if index==0{return ip;}
    #[cfg(unix)]let scope={
        let mut name=[0 as libc::c_char;libc::IF_NAMESIZE];
        let result=unsafe{libc::if_indextoname(index,name.as_mut_ptr())};
        if result.is_null(){index.to_string()}else{unsafe{std::ffi::CStr::from_ptr(result)}.to_string_lossy().into_owned()}
    };
    #[cfg(not(unix))]let scope=index.to_string();
    format!("{ip}%{scope}")
}

#[cfg(unix)]
impl<T> Endpoint<T> {
    /// Pin an admitted SCM_RIGHTS send independently of subsequent wrapper
    /// close. This duplicates a real kernel descriptor solely for transfer.
    pub(crate) fn duplicate_descriptor(&self) -> io::Result<std::os::fd::OwnedFd> {
        use std::os::fd::AsFd;
        self.socket()?.as_fd().try_clone_to_owned()
    }
    /// A received SCM_RIGHTS descriptor is already owned by this process;
    /// every unsuccessful adoption drops it exactly once.
    pub(crate) fn adopt_owned_descriptor(&mut self, fd: std::os::fd::OwnedFd) -> io::Result<()> {
        use std::os::fd::AsRawFd;
        if self.socket.is_some() { return Err(io::Error::from_raw_os_error(libc::EBUSY)); }
        if super::fd_handle_type::guess(fd.as_raw_fd()) != super::fd_handle_type::Kind::Udp {
            return Err(io::Error::from_raw_os_error(libc::EINVAL));
        }
        let socket = UdpSocket::from(fd);
        socket.set_nonblocking(true)?;
        socket2::SockRef::from(&socket).set_reuse_address(true)?;
        self.socket = Some(Descriptor::new(socket));
        Ok(())
    }
}
