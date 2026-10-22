/// Return whether this process currently has a terminal attached to `fd`.
pub(crate) fn is_terminal_fd(fd: i32) -> bool {
    #[cfg(unix)]
    {
        if fd < 0 {
            return false;
        }
        unsafe { libc_isatty(fd) }
    }
    #[cfg(not(unix))]
    {
        let _ = fd;
        false
    }
}

#[cfg(unix)]
unsafe fn libc_isatty(fd: i32) -> bool {
    extern "C" {
        fn isatty(fd: i32) -> i32;
    }
    unsafe { isatty(fd) != 0 }
}
