#![no_std]

mod close;
mod externs;
mod fditer;
mod util;

pub use close::close_open_fds;
pub use fditer::FdIter;

/// Iterate over all open file descriptors for the current process, starting
/// at `minfd`.
#[inline]
pub fn iter_open_fds(minfd: libc::c_int) -> FdIter {
    fditer::iter_fds(minfd, false)
}

/// Identical to `iter_open_fds()`, but may -- for efficiency -- yield invalid
/// file descriptors.
///
/// With this function, the caller is responsible for checking if the file
/// descriptors are valid.
///
/// # Proper usage
///
/// You should only use this function instead of `iter_open_fds()` if you
/// immediately perform an operation that implicitly checks if the file descriptor
/// is valid. For example:
///
/// ```
/// #[cfg(unix)]
/// {
///     use std::os::unix::io::FromRawFd;
///
///     for fd in close_fds::iter_possible_fds(0) {
///         let file = unsafe { std::fs::File::from_raw_fd(fd) };
///         let _meta = match file.metadata() {
///             Ok(m) => m,
///             Err(e) if e.raw_os_error() == Some(libc::EBADF) => {
///                 std::mem::forget(file);  // Don't try to close() it
///                 continue;
///             }
///             Err(e) => panic!(e),
///         };
///
///         // ...
///     }
/// }
/// ```
///
/// Note that this example is NOT intended to imply that you *should* be calling
/// `metadata()` (or any other methods) on random file descriptors.
#[inline]
pub fn iter_possible_fds(minfd: libc::c_int) -> FdIter {
    fditer::iter_fds(minfd, true)
}

/// Identical to `close_open_fds()`, but sets the `FD_CLOEXEC` flag on the file descriptors instead
/// of closing them. (Unix-only)
///
/// On some platforms (most notably, some of the BSDs), this is significantly less efficient than
/// `close_open_fds()`, and use of that function should be preferred when possible.
#[cfg(unix)]
pub fn set_fds_cloexec(minfd: libc::c_int, mut keep_fds: &[libc::c_int]) {
    let (max_keep_fd, fds_sorted) = util::inspect_keep_fds(keep_fds);

    for fd in iter_possible_fds(minfd) {
        if fd > max_keep_fd || !util::check_should_keep(&mut keep_fds, fd, fds_sorted) {
            // It's not in keep_fds
            unsafe {
                set_cloexec(fd);
            }
        }
    }
}

#[cfg(unix)]
unsafe fn set_cloexec(fd: libc::c_int) {
    let flags = libc::fcntl(fd, libc::F_GETFD);
    if flags >= 0 && (flags & libc::FD_CLOEXEC) != libc::FD_CLOEXEC {
        // fcntl(F_GETFD) succeeded, and it did *not* return the FD_CLOEXEC flag
        libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC);
    }
}
