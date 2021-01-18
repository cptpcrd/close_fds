#![no_std]

mod close;
mod fditer;
mod sys;
mod util;

pub use close::close_open_fds;
pub use fditer::FdIter;

/// Iterate over all open file descriptors for the current process, starting at `minfd`. The file
/// descriptors are guaranteed to be returned in ascending order.
///
/// # Warnings
///
/// **TL;DR**: Don't use this function in multithreaded programs unless you know what you're doing,
/// and avoid opening/closing file descriptors while consuming this iterator.
///
/// 1. File descriptors that are opened *during* iteration may or may not be included in the results
///    (exact behavior is platform-specific and depends on several factors).
///
/// 2. **IMPORTANT**: On some platforms, if other threads open file descriptors at very specific
///    times during a call to `FdIter::next()`, that may result in other file descriptors being
///    skipped. Use with caution. (If this is a problem for you, use
///    [`iter_open_fds_threadsafe()`], which avoids this issue).
///
/// 3. *Closing* file descriptors during iteration (in the same thread or in another thread) will
///    not affect the iterator's ability to list other open file descriptors (if it does, that is a
///    bug). However, in most cases you should use [`close_open_fds()`] to do this.
///
/// 4. Some of the file descriptors yielded by this iterator may be in active use by other sections
///    of code. Be very careful about which operations you perform on them.
///
///    If your program is multi-threaded, this is especially true, since a file descriptor returned
///    by this iterator may have been closed by the time your code tries to do something with it.
///
/// [`close_open_fds()`]: ./fn.close_open_fds.html
/// [`iter_open_fds_threadsafe()`]: ./fn.iter_open_fds_threadsafe.html
#[inline]
pub fn iter_open_fds(minfd: libc::c_int) -> FdIter {
    fditer::iter_fds(minfd, false, false)
}

/// Equivalent to `iter_open_fds()`, but behaves more reliably in multithreaded programs (at the
/// cost of decreased performance on some platforms).
///
/// Specifically, if other threads open file descriptors at specific times, [`iter_open_fds()`]
/// may skip over other file descriptors. This function avoids those issues.
///
/// Note, however, that this behavior comes at the cost of significantly increased performance on
/// certain platforms (currently, this is limited to 1) OpenBSD and 2) FreeBSD without an `fdescfs`
/// mounted on `/dev/fd`). This is because the non-thread-safe code provides a potential performance
/// improvement on those platforms.
///
/// [`iter_open_fds()`]: ./fn.iter_open_fds.html
#[inline]
pub fn iter_open_fds_threadsafe(minfd: libc::c_int) -> FdIter {
    fditer::iter_fds(minfd, false, true)
}

/// Identical to `iter_open_fds()`, but may -- for efficiency -- yield invalid file descriptors.
///
/// With this function, the caller is responsible for checking if the file descriptors are valid.
///
/// # Proper usage
///
/// You should only use this function instead of `iter_open_fds()` if you immediately perform an
/// operation that implicitly checks if the file descriptor is valid. For example:
///
/// ```
/// use std::os::unix::io::FromRawFd;
///
/// for fd in close_fds::iter_possible_fds(0) {
///     let file = unsafe { std::fs::File::from_raw_fd(fd) };
///     let _meta = match file.metadata() {
///         Ok(m) => m,
///         Err(e) if e.raw_os_error() == Some(libc::EBADF) => {
///             std::mem::forget(file);  // Don't try to close() it
///             continue;
///         }
///         Err(e) => panic!(e),
///     };
///
///     // ...
/// }
/// ```
///
/// Note that this example is NOT intended to imply that you *should* be calling `metadata()` (or
/// any other methods) on random file descriptors. Most of the warnings about [`iter_open_fds()`]
/// apply to this function as well.
///
/// [`iter_open_fds()`]: ./fn.iter_open_fds.html
#[inline]
pub fn iter_possible_fds(minfd: libc::c_int) -> FdIter {
    fditer::iter_fds(minfd, true, false)
}

/// Equivalent to `iter_possible_fds()`, but behaves more reliably in multithreaded programs (at
/// the cost of increased performance on some platforms).
///
/// See [`iter_open_fds_threadsafe()`] for more details on what this means.
///
/// [`iter_open_fds_threadsafe()`]: ./fn.iter_open_fds_threadsafe.html
#[inline]
pub fn iter_possible_fds_threadsafe(minfd: libc::c_int) -> FdIter {
    fditer::iter_fds(minfd, true, true)
}

/// Identical to `close_open_fds()`, but sets the `FD_CLOEXEC` flag on the file descriptors instead
/// of closing them.
///
/// On some platforms (most notably, some of the BSDs), this is significantly less efficient than
/// `close_open_fds()`, and use of that function should be preferred when possible.
#[inline]
pub fn set_fds_cloexec(minfd: libc::c_int, keep_fds: &[libc::c_int]) {
    set_fds_cloexec_generic(minfd, keep_fds, false)
}

/// Equivalent to `set_fds_cloexec()`, but behaves more reliably in multithreaded programs (at the
/// cost of increased performance on some platforms).
///
/// See [`iter_open_fds_threadsafe()`] for more details on what this means.
///
/// [`iter_open_fds_threadsafe()`]: ./fn.iter_open_fds_threadsafe.html
#[inline]
pub fn set_fds_cloexec_threadsafe(minfd: libc::c_int, keep_fds: &[libc::c_int]) {
    set_fds_cloexec_generic(minfd, keep_fds, true)
}

fn set_fds_cloexec_generic(minfd: libc::c_int, mut keep_fds: &[libc::c_int], thread_safe: bool) {
    let (max_keep_fd, fds_sorted) = util::inspect_keep_fds(keep_fds);

    for fd in fditer::iter_fds(minfd, true, thread_safe) {
        if fd > max_keep_fd || !util::check_should_keep(&mut keep_fds, fd, fds_sorted) {
            // It's not in keep_fds

            let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };

            if flags >= 0 && (flags & libc::FD_CLOEXEC) != libc::FD_CLOEXEC {
                // fcntl(F_GETFD) succeeded, and it did *not* return the FD_CLOEXEC flag
                unsafe {
                    libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC);
                }
            }
        }
    }
}
