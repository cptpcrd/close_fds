//! # Why is this crate useful?
//!
//! By default, any file descriptors opened by a process are inherited by any of that process's
//! children. This can cause major bugs and security issues, but this behavior can't be changed
//! without massively breaking backwards compatibility.
//!
//! Rust hides most of these problems by setting the close-on-exec flag on all file descriptors
//! opened by the standard library (which causes them to *not* be inherited by child processes).
//! However, in some scenarios it may be necessary to have a way to close all open file
//! descriptors.
//!
//! ## Scenarios where this is helpful
//!
//! - Writing set-UID programs (which may not be able to fully trust their environment; for
//!   example, `sudo` closes all open file descriptors when it starts as a security measure)
//! - Spawning processes while interacting with FFI code that *doesn't* set the close-on-exec flag
//!   on file descriptors it opens
//! - On some platforms (notably, macOS/iOS), Rust isn't always able to set the close-on-exec flag
//!   *atomically*, which creates race conditions if one thread is e.g. opening sockets while
//!   another thread is spawning processes. This crate may be useful in helping to avoid those
//!   race conditions.
//!
//! # Example usage
//!
//! Here is a short program that uses `close_fds` to close all file descriptors (except the ones
//! in a specified list) in a child process before launching it:
//!
//! ```
//! use std::process::Command;
//! use std::os::unix::prelude::*;
//!
//! // Add any file descriptors that should stay open here
//! let mut keep_fds = [];
//! // ALWAYS sort the slice! It will give significant performance improvements.
//! keep_fds.sort_unstable();
//!
//! let mut cmd = Command::new("true");
//!
//! // ...
//! // Set up `cmd` here
//! // ...
//!
//! unsafe {
//!     cmd.pre_exec(move || {
//!         // On macOS/iOS, just set them as close-on-exec (some sources indicate closing them
//!         // directly may cause problems)
//!         #[cfg(any(target_os = "macos", target_os = "ios"))]
//!         close_fds::set_fds_cloexec(3, &keep_fds);
//!         #[cfg(not(any(target_os = "macos", target_os = "ios")))]
//!         close_fds::close_open_fds(3, &keep_fds);
//!
//!         Ok(())
//!     });
//! }
//!
//! // Launch the child process
//!
//! cmd.status().unwrap();
//! ```
//!
//! # Async-signal-safety
//!
//! ## Background
//!
//! An async-signal-safe function is one that is safe to call from a signal handler (many functions
//! in the system libc are not!). In a multi-threaded program, when running in the child after a
//! `fork()` (such as in a closure registered with
//! `std::os::unix::process::CommandExt::pre_exec()`, it is only safe to call async-signal-safe
//! functions. See `signal-safety(7)` and `fork(2)` for more information.
//!
//! ## Async-signal-safety in this crate
//!
//! **TL;DR**: The functions in this crate are async-signal-safe on Linux, macOS/iOS, the BSDs, and
//! Solaris/Illumos. They *should* also be async-signal-safe on other \*nix-like OSes.
//!
//! Since the functions in this crate are most useful in the child process after a `fork()`, this
//! crate tries to make all of them async-signal-safe. However, many of the optimizations that this
//! crate performs in order to be efficient would not be possible by simply calling functions that
//! POSIX requires to be async-signal-safe.
//!
//! As a result, this crate assumes that the following functions are async-signal-safe (in addition
//! to the ones required by POSIX):
//!
//! - `closefrom()` on the BSDs
//! - The `close_range()` syscall on Linux and FreeBSD
//! - `sysctl()` on FreeBSD
//! - `getdtablecount()` on OpenBSD
//! - `getdirentries()`/`getdents()` (whichever is available) on Linux, NetBSD, FreeBSD, macOS/iOS,
//!   and Solaris/Illumos
//! - `sysconf(_SC_OPEN_MAX)` on all OSes
//!
//! All of these except for `sysconf()` are implemented as system calls (or thin wrappers around
//! other system calls) on whichever OS(es) they are present on. As a result, they should be
//! async-signal-safe, even though they are not explicitly documented as such.
//!
//! `sysconf()` is not guaranteed to be async-signal-safe. However, on Linux, macOS/iOS, the BSDs,
//! and Solaris/Illumos, `sysconf(_SC_OPEN_MAX)` is implemented in terms of
//! `getrlimit(RLIMIT_NOFILE)`. On those platforms, `getrlimit()` is a system call, so
//! `sysconf(_SC_OPEN_MAX)` (and thus, the functions in this crate) should be async-signal-safe.

#![no_std]

mod cloexec;
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
/// Note, however, that this behavior comes at the cost of significantly decreased performance on
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
/// ```no_run
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
/// the cost of decreased performance on some platforms).
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
    cloexec::set_fds_cloexec_generic(minfd, keep_fds, false)
}

/// Equivalent to `set_fds_cloexec()`, but behaves more reliably in multithreaded programs (at the
/// cost of decreased performance on some platforms).
///
/// See [`iter_open_fds_threadsafe()`] for more details on what this means.
///
/// [`iter_open_fds_threadsafe()`]: ./fn.iter_open_fds_threadsafe.html
#[inline]
pub fn set_fds_cloexec_threadsafe(minfd: libc::c_int, keep_fds: &[libc::c_int]) {
    cloexec::set_fds_cloexec_generic(minfd, keep_fds, true)
}
