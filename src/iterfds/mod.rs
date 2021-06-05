mod fditer;
pub use fditer::FdIter;

#[cfg(any(
    target_os = "linux",
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "solaris",
    target_os = "illumos",
))]
mod dirfd;

/// A "builder" to construct an [`FdIter`] with custom parameters.
///
/// # Warnings
///
/// **TL;DR**: Don't use `FdIter`/`FdIterBuilder` in multithreaded programs unless you know what
/// you're doing, and avoid opening/closing file descriptors while consuming an `FdIter`.
///
/// 1. File descriptors that are opened *during* iteration may or may not be included in the results
///    (exact behavior is platform-specific and depends on several factors).
///
/// 2. **IMPORTANT**: On some platforms, if other threads open file descriptors at very specific
///    times during a call to `FdIter::next()`, that may result in other file descriptors being
///    skipped. Use with caution. (If this is a problem for you, set `.threadsafe(true)`, which
///    avoids this issue).
///
/// 3. *Closing* file descriptors during iteration (in the same thread or in another thread) will
///    not affect the iterator's ability to list other open file descriptors (if it does, that is a
///    bug). However, in most cases you should use
///    [`CloseFdsBuilder`](./struct.CloseFdsBuilder.html) to do this.
///
/// 4. Some of the file descriptors yielded by this iterator may be in active use by other sections
///    of code. Be very careful about which operations you perform on them.
///
///    If your program is multi-threaded, this is especially true, since a file descriptor returned
///    by this iterator may have been closed by the time your code tries to do something with it.
#[derive(Clone, Debug)]
pub struct FdIterBuilder {
    possible: bool,
    #[cfg(any(target_os = "freebsd", target_os = "openbsd"))]
    skip_nfds: bool,
    #[cfg(any(
        target_os = "linux",
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "solaris",
        target_os = "illumos",
    ))]
    dirfd: bool,
}

impl FdIterBuilder {
    /// Create a new builder.
    ///
    /// `minfd` specifies the number of the file descriptor at which iteration will begin.
    #[inline]
    pub fn new() -> Self {
        Self {
            possible: false,
            #[cfg(any(target_os = "freebsd", target_os = "openbsd"))]
            skip_nfds: false,
            #[cfg(any(
                target_os = "linux",
                target_os = "macos",
                target_os = "ios",
                target_os = "freebsd",
                target_os = "netbsd",
                target_os = "solaris",
                target_os = "illumos",
            ))]
            dirfd: true,
        }
    }

    /// Set whether the returned `FdIter` is allowed to yield invalid file descriptors for
    /// efficiency (default is `false`).
    ///
    /// If this flag is set, the caller is responsible for checking if the returned file descriptors
    /// are valid.
    ///
    /// # Proper usage
    ///
    /// You should only use this flag if you immediately perform an operation on each file
    /// descriptor that implicitly checks if the file descriptor is valid.
    #[inline]
    pub fn possible(&mut self, possible: bool) -> &mut Self {
        self.possible = possible;
        self
    }

    /// Set whether the returned `FdIter` needs to behave reliably in multithreaded programs
    /// (default is `false`).
    ///
    /// If other threads open file descriptors at specific times, an `FdIter` may skip over other
    /// file descriptors. Setting `.threadsafe(true)` prevents this, but may come at the cost of
    /// significantly increased performance on some platforms (because the code which may behave
    /// strangely in the presence of threads provides a potential performance improvement).
    ///
    /// Currently, setting this flag will only affect performance on 1) OpenBSD and 2) FreeBSD
    /// without an `fdescfs` mounted on `/dev/fd`.
    #[allow(unused_variables)]
    #[inline]
    pub fn threadsafe(&mut self, threadsafe: bool) -> &mut Self {
        #[cfg(any(target_os = "freebsd", target_os = "openbsd"))]
        {
            self.skip_nfds = threadsafe;
        }
        self
    }

    /// Set whether returned `FdIter` is allowed to look at special files for speedups (default is
    /// `true`).
    ///
    /// On some systems, `/dev/fd` and/or `/proc/self/fd` provide an accurate view of the file
    /// descriptors that the current process has open; if this flag is set to `true` then those
    /// may be examined as an optimization.
    ///
    /// It may be desirable to set this to `false` e.g. if `chroot()`ing into an environment where
    /// untrusted code may be able to replace `/proc` or `/dev`. However, on some platforms (such
    /// as Linux<5.9 and macOS) setting this to `false` may significantly decrease performance.
    #[allow(unused_variables)]
    #[inline]
    pub fn allow_filesystem(&mut self, fs: bool) -> &mut Self {
        #[cfg(any(
            target_os = "linux",
            target_os = "macos",
            target_os = "ios",
            target_os = "freebsd",
            target_os = "netbsd",
            target_os = "solaris",
            target_os = "illumos",
        ))]
        {
            self.dirfd = fs;
        }
        self
    }

    /// Create an `FdIter` that iterates over the open file descriptors starting at `minfd`.
    pub fn iter_from(&self, mut minfd: libc::c_int) -> FdIter {
        if minfd < 0 {
            minfd = 0;
        }

        FdIter {
            curfd: minfd,
            possible: self.possible,
            maxfd: None,
            #[cfg(any(target_os = "freebsd", target_os = "openbsd"))]
            skip_nfds: self.skip_nfds,
            #[cfg(any(
                target_os = "linux",
                target_os = "macos",
                target_os = "ios",
                target_os = "freebsd",
                target_os = "netbsd",
                target_os = "solaris",
                target_os = "illumos",
            ))]
            dirfd_iter: if self.dirfd {
                dirfd::DirFdIter::open(minfd)
            } else {
                None
            },
        }
    }
}

impl Default for FdIterBuilder {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

/// Iterate over all open file descriptors for the current process, starting at `minfd`. The file
/// descriptors are guaranteed to be returned in ascending order.
///
/// This is equivalent to `FdIterBuilder::new().iter_from(minfd)`.
///
/// See the warnings for [`FdIterBuilder`].
#[inline]
pub fn iter_open_fds(minfd: libc::c_int) -> FdIter {
    FdIterBuilder::new().iter_from(minfd)
}

/// Equivalent to [`iter_open_fds()`], but behaves more reliably in multithreaded programs (at the
/// cost of decreased performance on some platforms).
///
/// This is equivalent to `FdIterBuilder::new().threadsafe(true).iter_from(minfd)`.
///
/// See [`FdIterBuilder::threadsafe()`] for more information.
#[inline]
pub fn iter_open_fds_threadsafe(minfd: libc::c_int) -> FdIter {
    FdIterBuilder::new().threadsafe(true).iter_from(minfd)
}

/// Identical to `iter_open_fds()`, but may -- for efficiency -- yield invalid file descriptors.
///
/// This is equivalent to `FdIterBuilder::new().possible(true).iter_from(minfd)`.
///
/// See [`FdIterBuilder::possible()`] for more information.
#[inline]
pub fn iter_possible_fds(minfd: libc::c_int) -> FdIter {
    FdIterBuilder::new().possible(true).iter_from(minfd)
}

/// Identical to `iter_open_fds_threadsafe()`, but may -- for efficiency -- yield invalid file
/// descriptors.
///
/// This is equivalent to `FdIterBuilder::new().possible(true).threadafe(true).iter_from(minfd)`.
///
/// See [`FdIterBuilder::possible()`] and [`FdIterBuilder::threadsafe()`] for more information.
#[inline]
pub fn iter_possible_fds_threadsafe(minfd: libc::c_int) -> FdIter {
    FdIterBuilder::new()
        .possible(true)
        .threadsafe(true)
        .iter_from(minfd)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_files() -> [libc::c_int; 10] {
        let mut fds = [-1; 10];
        for cur_fd in fds.iter_mut() {
            *cur_fd = unsafe { libc::open("/\0".as_ptr() as *const libc::c_char, libc::O_RDONLY) };
            assert!(*cur_fd >= 0);
        }
        fds
    }

    unsafe fn close_files(fds: &[libc::c_int]) {
        for &fd in fds {
            libc::close(fd);
        }
    }

    #[test]
    fn test_size_hint_open() {
        test_size_hint_generic(FdIterBuilder::new().threadsafe(false).iter_from(0));
        test_size_hint_generic(FdIterBuilder::new().threadsafe(true).iter_from(0));

        let fds = open_files();
        test_size_hint_generic(FdIterBuilder::new().threadsafe(false).iter_from(0));
        test_size_hint_generic(FdIterBuilder::new().threadsafe(true).iter_from(0));
        unsafe {
            close_files(&fds);
        }
    }

    #[test]
    fn test_size_hint_possible() {
        test_size_hint_generic(
            FdIterBuilder::new()
                .possible(true)
                .threadsafe(false)
                .iter_from(0),
        );
        test_size_hint_generic(
            FdIterBuilder::new()
                .possible(true)
                .threadsafe(true)
                .iter_from(0),
        );

        let fds = open_files();
        test_size_hint_generic(
            FdIterBuilder::new()
                .possible(true)
                .threadsafe(false)
                .iter_from(0),
        );
        test_size_hint_generic(
            FdIterBuilder::new()
                .possible(true)
                .threadsafe(true)
                .iter_from(0),
        );
        unsafe {
            close_files(&fds);
        }
    }

    fn test_size_hint_generic(mut fditer: FdIter) {
        let (mut init_low, mut init_high) = fditer.size_hint();
        if let Some(init_high) = init_high {
            // Sanity check
            assert!(init_high >= init_low);
        }

        let mut i = 0;
        while let Some(_fd) = fditer.next() {
            let (cur_low, cur_high) = fditer.size_hint();

            // Adjust them so they're comparable to init_low and init_high
            let adj_low = cur_low + i + 1;
            let adj_high = if let Some(cur_high) = cur_high {
                // Sanity check
                assert!(cur_high >= cur_low);

                Some(cur_high + i + 1)
            } else {
                None
            };

            // Now we adjust init_low and init_high to be the most restrictive limits that we've
            // received so far.
            if adj_low > init_low {
                init_low = adj_low;
            }

            if let Some(adj_high) = adj_high {
                if let Some(ihigh) = init_high {
                    if adj_high < ihigh {
                        init_high = Some(adj_high);
                    }
                } else {
                    init_high = Some(adj_high);
                }
            }

            i += 1;
        }

        // At the end, the lower boundary should be 0. The upper boundary can be anything.
        let (final_low, _) = fditer.size_hint();
        assert_eq!(final_low, 0);

        // Now make sure that the actual count falls within the boundaries we were given
        assert!(i >= init_low);
        if let Some(init_high) = init_high {
            assert!(i <= init_high);
        }
    }

    #[test]
    fn test_fused_open() {
        test_fused_generic(FdIterBuilder::new().threadsafe(false).iter_from(0));
        test_fused_generic(FdIterBuilder::new().threadsafe(true).iter_from(0));

        let fds = open_files();
        test_fused_generic(FdIterBuilder::new().threadsafe(false).iter_from(0));
        test_fused_generic(FdIterBuilder::new().threadsafe(true).iter_from(0));
        unsafe {
            close_files(&fds);
        }
    }

    #[test]
    fn test_fused_possible() {
        test_fused_generic(
            FdIterBuilder::new()
                .possible(true)
                .threadsafe(false)
                .iter_from(0),
        );
        test_fused_generic(
            FdIterBuilder::new()
                .possible(true)
                .threadsafe(true)
                .iter_from(0),
        );

        let fds = open_files();
        test_fused_generic(
            FdIterBuilder::new()
                .possible(true)
                .threadsafe(false)
                .iter_from(0),
        );
        test_fused_generic(
            FdIterBuilder::new()
                .possible(true)
                .threadsafe(true)
                .iter_from(0),
        );

        unsafe {
            close_files(&fds);
        }
    }

    fn test_fused_generic(mut fditer: FdIter) {
        // Exhaust the iterator
        fditer.by_ref().count();
        assert_eq!(fditer.next(), None);
    }
}
