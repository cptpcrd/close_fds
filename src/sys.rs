#[cfg(target_os = "freebsd")]
pub const KERN_PROC_NFDS: libc::c_int = 43;

#[cfg(target_os = "macos")]
pub const SYS_GETDIRENTRIES64: libc::c_int = 344;

// This is the correct value for every architecture except alpha, which Rust doesn't support.
#[cfg(target_os = "linux")]
pub const SYS_CLOSE_RANGE: libc::c_long = 436;

#[cfg(target_os = "linux")]
pub const CLOSE_RANGE_CLOEXEC: libc::c_uint = 1 << 2;

#[cfg(target_os = "freebsd")]
pub const SYS_CLOSE_RANGE: libc::c_int = 575;

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

#[cfg(target_os = "freebsd")]
#[repr(C)]
pub struct dirent {
    pub d_fileno: libc::ino_t,
    pub d_off: libc::off_t,
    pub d_reclen: u16,
    pub d_type: u8,
    d_pad0: u8,
    pub d_namlen: u16,
    d_pad1: u16,
    pub d_name: [libc::c_char; 256],
}

#[cfg(target_os = "openbsd")]
extern "C" {
    pub fn getdtablecount() -> libc::c_int;
}

#[cfg(any(target_os = "openbsd", target_os = "netbsd", target_os = "dragonfly"))]
extern "C" {
    pub fn closefrom(fd: libc::c_int) -> libc::c_int;
}

#[cfg(target_os = "netbsd")]
extern "C" {
    pub fn getdents(
        fildes: libc::c_int,
        buf: *mut libc::c_char,
        nbyte: libc::size_t,
    ) -> libc::c_int;
}

#[cfg(any(target_os = "solaris", target_os = "illumos"))]
extern "C" {
    pub fn getdents(
        fildes: libc::c_int,
        buf: *mut libc::dirent,
        nbyte: libc::size_t,
    ) -> libc::c_int;
}
