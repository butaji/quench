//! Child output reads follow guest demand; inherited pipes keep one native owner.
use std::{
    fs::File,
    os::fd::{AsRawFd, OwnedFd},
};
const OUTPUT_CHUNK_BYTES: usize = 16 * 1024;

#[derive(Clone, Copy)]
enum Usage {
    Guest,
    Inherited,
}
#[derive(Clone, Copy)]
enum Demand {
    Idle,
    Requested,
}
enum Phase {
    Open { file: File, demand: Demand },
    Closed,
}
pub(crate) struct Output {
    phase: Phase,
    usage: Usage,
}
pub(crate) enum Progress {
    Idle,
    Data(Vec<u8>),
    End,
}
impl Output {
    pub(crate) fn new(pipe: Option<OwnedFd>) -> Self {
        Self {
            phase: pipe.map_or(Phase::Closed, |fd| Phase::Open {
                file: fd.into(),
                demand: Demand::Idle,
            }),
            usage: Usage::Guest,
        }
    }
    pub(crate) fn descriptor(&self) -> Option<i32> {
        match &self.phase {
            Phase::Open { file, .. } => Some(file.as_raw_fd()),
            Phase::Closed => None,
        }
    }
    pub(crate) fn stop(&mut self) {
        if let Phase::Open { demand, .. } = &mut self.phase {
            *demand = Demand::Idle;
        }
    }
    pub(crate) fn inherit(&mut self) {
        self.stop();
        self.usage = Usage::Inherited;
    }
    pub(crate) fn inherited(&self) -> bool {
        matches!(self.usage, Usage::Inherited)
    }
    pub(crate) fn is_closed(&self) -> bool {
        matches!(self.phase, Phase::Closed)
    }
    pub(crate) fn is_referenced(&self) -> bool {
        match &self.phase {
            Phase::Closed => false,
            Phase::Open { demand, .. } => !self.inherited() || matches!(demand, Demand::Requested),
        }
    }
    pub(crate) fn request(&mut self) {
        if let Phase::Open { demand, .. } = &mut self.phase {
            *demand = Demand::Requested;
        }
    }
    pub(crate) fn close(&mut self) {
        self.phase = Phase::Closed;
    }
    pub(crate) fn progress(&mut self) -> Progress {
        let Phase::Open {
            file,
            demand: Demand::Requested,
        } = &self.phase
        else {
            return Progress::Idle;
        };
        let fd = file.as_raw_fd();
        let mut bytes = [0; OUTPUT_CHUNK_BYTES];
        loop {
            let received = unsafe {
                libc::recv(
                    fd,
                    bytes.as_mut_ptr().cast(),
                    bytes.len(),
                    libc::MSG_DONTWAIT,
                )
            };
            let read = if received < 0 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(received as usize)
            };
            match read {
                Ok(0) => {
                    self.close();
                    return Progress::End;
                }
                Ok(length) => {
                    self.stop();
                    return Progress::Data(bytes[..length].to_vec());
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    return Progress::Idle;
                }
                Err(_) => {
                    self.close();
                    return Progress::End;
                }
            }
        }
    }
}
