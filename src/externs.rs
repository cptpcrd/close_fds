#[cfg(target_os = "freebsd")]
pub const KERN_PROC_NFDS: libc::c_int = 43;

#[cfg(target_os = "macos")]
pub const SYS_GETDIRENTRIES64: libc::c_int = 344;

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

#[cfg(target_os = "openbsd")]
extern "C" {
    pub fn getdtablecount() -> libc::c_int;
}

#[cfg(any(target_os = "openbsd", target_os = "netbsd", target_os = "dragonfly"))]
extern "C" {
    pub fn closefrom(fd: libc::c_int) -> libc::c_int;
}

#[cfg(any(target_os = "solaris", target_os = "illumos"))]
extern "C" {
    pub fn getdents(
        fildes: libc::c_int,
        buf: *mut libc::dirent,
        nbyte: libc::size_t,
    ) -> libc::c_int;
}
