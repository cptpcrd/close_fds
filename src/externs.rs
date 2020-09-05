#[cfg(target_os = "freebsd")]
pub const KERN_PROC_NFDS: libc::c_int = 43;

#[cfg(target_os = "freebsd")]
extern "C" {
    pub fn closefrom(lowfd: libc::c_int);

    pub fn getdirentries(
        fd: libc::c_int,
        buf: *mut libc::c_char,
        nbytes: libc::size_t,
        basep: *mut libc::off_t,
    ) -> libc::ssize_t;
}

#[cfg(any(target_os = "openbsd", target_os = "netbsd", target_os = "dragonfly"))]
extern "C" {
    pub fn closefrom(fd: libc::c_int) -> libc::c_int;
}
