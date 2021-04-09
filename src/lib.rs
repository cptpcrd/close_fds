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
//!   on file descriptors it opens (the functionality offered by this crate is the ONLY way to
//!   safely do this)
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

mod closefds;
mod iterfds;
mod sys;
mod util;

pub use closefds::*;
pub use iterfds::*;
