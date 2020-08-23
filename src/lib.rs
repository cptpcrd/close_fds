#![no_std]

mod externs;
mod fditer;
mod util;

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd"))]
mod dirfd;

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

/// Close all open file descriptors starting at `minfd`, except for the file
/// descriptors in `keep_fds`.
///
/// # Safety
///
/// This function is NOT safe to use if other threads are interacting with files,
/// networking, or anything else that could possibly involve file descriptors in
/// any way, shape, or form.
///
/// In addition, some objects, such as `std::fs::File`, may open file descriptors
/// and then assume that they will remain open. This function, by closing those
/// file descriptors, violates those assumptions.
///
/// This function is safe to use if it can be verified that these are not concerns.
/// For example, it *should* be safe at startup or just before an `exec()`. At all
/// other times, exercise extreme caution when using this function, as it may lead
/// to race conditions and/or security issues.
///
/// # Efficiency
///
/// ## Efficiency of using `keep_fds`
///
/// **TL;DR**: If you're going to be passing more than a few file descriptors in
/// `keep_fds`, sort the slice first for best performance.
///
/// On some systems, the `keep_fds` list may see massive numbers of lookups,
/// especially if it contains high-numbered file descriptors.
///
/// If `keep_fds` is sorted, since `iter_open_fds()` goes in ascending order it is easy
/// to check for the presence of a given file descriptor in `keep_fds`. However,
/// because `close_fds` is a `#![no_std]` crate, it can't allocate memory for a *copy*
/// of `keep_fds` that it can sort.
///
/// As a result, this function first checks if `keep_fds` is sorted. If it is, the more
/// efficient method can be employed. If not, it falls back on `.contains()`. which
/// can be very slow.
///
/// # Windows support
///
/// On Windows, this only deals with file descriptors, NOT file handles.
pub unsafe fn close_open_fds(mut minfd: libc::c_int, mut keep_fds: &[libc::c_int]) {
    if minfd < 0 {
        minfd = 0;
    }

    let (max_keep_fd, fds_sorted) = util::inspect_keep_fds(keep_fds);

    #[cfg(any(
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd",
        target_os = "dragonfly"
    ))]
    if max_keep_fd < minfd {
        // On the BSDs, if all the file descriptors in keep_fds are less than
        // minfd (or if keep_fds is empty), we can just call closefrom()
        externs::closefrom(minfd);
        return;
    }

    let mut fditer = iter_possible_fds(minfd);

    // We have to use a while loop so we can drop() the iterator in the
    // closefrom() case
    #[allow(clippy::while_let_on_iterator)]
    while let Some(fd) = fditer.next() {
        if fd > max_keep_fd {
            // If fd > max_keep_fd, we know that none of the file descriptors we encounter from
            // here onward can be in keep_fds.

            // On the BSDs we can use closefrom() to close the rest
            #[cfg(any(
                target_os = "freebsd",
                target_os = "netbsd",
                target_os = "openbsd",
                target_os = "dragonfly"
            ))]
            {
                // Close the directory file descriptor (if one is being used) first
                drop(fditer);
                externs::closefrom(fd);
                return;
            }

            // On other systems, this just allows us to skip the contains() check
            #[cfg(not(any(
                target_os = "freebsd",
                target_os = "netbsd",
                target_os = "openbsd",
                target_os = "dragonfly"
            )))]
            {
                libc::close(fd);
            }
        } else if !util::check_should_keep(&mut keep_fds, fd, fds_sorted) {
            // Close it if it's not in keep_fds
            libc::close(fd);
        }
    }
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
    #[cfg(any(
        target_os = "linux",
        target_os = "macos",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly"
    ))]
    libc::ioctl(fd, libc::FIOCLEX);

    #[cfg(not(any(
        target_os = "linux",
        target_os = "macos",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly"
    )))]
    {
        let flags = libc::fcntl(fd, libc::F_GETFD);
        if flags >= 0 && (flags & libc::FD_CLOEXEC) != libc::FD_CLOEXEC {
            // fcntl(F_GETFD) succeeded, and it did *not* return the FD_CLOEXEC flag
            libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC);
        }
    }
}
