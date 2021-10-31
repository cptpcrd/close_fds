use crate::FdIterBuilder;

mod cloexec;
mod close;

/// A "builder" for either closing all open file descriptors or setting them as close-on-exec.
#[derive(Clone, Debug)]
pub struct CloseFdsBuilder<'a> {
    keep_fds: KeepFds<'a>,
    it: FdIterBuilder,
}

impl<'a> CloseFdsBuilder<'a> {
    /// Create a new builder.
    #[inline]
    pub fn new() -> Self {
        Self {
            keep_fds: KeepFds::empty(),
            it: FdIterBuilder::new(),
        }
    }

    /// Leave the file descriptors listed in `keep_fds` alone; i.e. do not close them or set the
    /// close-on-exec flag on them.
    ///
    /// Calling this method multiple times will *replace* the list of file descriptors to be left
    /// alone, not extend it. Additionally, it's not recommended to call this method multiple
    /// times, since it pre-scans the list to collect information for later use.
    ///
    /// # Efficiency
    ///
    /// It is **highly** recommended to sort the `keep_fds` slice first (see also
    /// [`Self::keep_fds_sorted()`]). This will give you significant performance improvements
    /// (especially on Linux 5.9+ and FreeBSD 12.2+).
    ///
    /// `close_fds` can't just copy the slice and sort it for you because allocating memory is not
    /// async-signal-safe (see ["Async-signal-safety"](./index.html#async-signal-safety)).
    #[inline]
    pub fn keep_fds(&mut self, keep_fds: &'a [libc::c_int]) -> &mut Self {
        self.keep_fds = KeepFds::new(keep_fds);
        self
    }

    /// Identical to [`Self::keep_fds()`], but assumes that the given list of file descriptors is
    /// sorted.
    ///
    /// # Safety
    ///
    /// `keep_fds` must be sorted in ascending order.
    #[inline]
    pub unsafe fn keep_fds_sorted(&mut self, keep_fds: &'a [libc::c_int]) -> &mut Self {
        self.keep_fds = KeepFds::new_sorted(keep_fds);
        self
    }

    /// Set whether [`Self::cloexecfrom()`] needs to behave reliably in multithreaded programs
    /// (default is `false`).
    ///
    /// See [`FdIterBuilder::threadsafe()`](./struct.FdIterBuilder.html#method.threadsafe) for more
    /// information.
    ///
    /// Note that this only applies when using [`Self::cloexecfrom()`]. [`Self::closefrom()`]
    /// is unsafe in the presence of threads anyway, so it is not required to honor this.
    #[inline]
    pub fn threadsafe(&mut self, threadsafe: bool) -> &mut Self {
        self.it.threadsafe(threadsafe);
        self
    }

    /// Set whether this crate is allowed to look at special files for speedups when closing the
    /// specified file descriptors (default is `true`).
    ///
    /// See
    /// [`FdIterBuilder::allow_filesystem()`](./struct.FdIterBuilder.html#method.allow_filesystem)
    /// for more information.
    #[inline]
    pub fn allow_filesystem(&mut self, fs: bool) -> &mut Self {
        self.it.allow_filesystem(fs);
        self
    }

    /// Identical to [`Self::closefrom()`], but sets the `FD_CLOEXEC` flag on the file descriptors
    /// instead of closing them.
    ///
    /// On some platforms (most notably, some of the BSDs), this is significantly less efficient than
    /// [`Self::closefrom()`], and use of that function should be preferred when possible.
    pub fn cloexecfrom(&self, minfd: libc::c_int) {
        cloexec::set_fds_cloexec(
            core::cmp::max(minfd, 0),
            self.keep_fds.clone(),
            self.it.clone(),
        );
    }

    /// Close all of the file descriptors starting at `minfd` and not excluded by
    /// [`Self::keep_fds()`].
    ///
    /// # Safety
    ///
    /// This function is NOT safe to use if other threads are interacting with files, networking,
    /// or anything else that could possibly involve file descriptors in any way, shape, or form.
    /// (Note: On some systems, file descriptor use may be more common than you think! For example,
    /// on Linux with some versions of musl libc, `std::fs::canonicalize()` will open a file
    /// descriptor to the given path.)
    ///
    /// In addition, some objects, such as `std::fs::File`, may open file descriptors and then
    /// assume that they will remain open. This function, by closing those file descriptors,
    /// violates those assumptions.
    ///
    /// This function is safe to use if it can be verified that these are not concerns. For
    /// example, it *should* be safe at startup or just before an `exec()`. At all other times,
    /// exercise extreme caution when using this function, as it may lead to race conditions and/or
    /// security issues.
    ///
    /// (Note: The above warnings, by definition, make it unsafe to call this function concurrently
    /// from multiple threads. As a result, this function may perform other non-thread-safe
    /// operations.)
    pub unsafe fn closefrom(&self, minfd: libc::c_int) {
        close::close_fds(
            core::cmp::max(minfd, 0),
            self.keep_fds.clone(),
            self.it.clone(),
        );
    }
}

impl<'a> Default for CloseFdsBuilder<'a> {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug)]
pub(crate) struct KeepFds<'a> {
    fds: &'a [libc::c_int],
    max: libc::c_int,
    sorted: bool,
}

impl<'a> KeepFds<'a> {
    #[inline]
    pub fn empty() -> Self {
        Self {
            fds: &[],
            max: -1,
            sorted: true,
        }
    }

    #[inline]
    pub fn new(fds: &'a [libc::c_int]) -> Self {
        let (max, sorted) = crate::util::inspect_keep_fds(fds);
        Self { fds, max, sorted }
    }

    #[inline]
    pub unsafe fn new_sorted(fds: &'a [libc::c_int]) -> Self {
        Self {
            fds,
            max: fds.last().copied().unwrap_or(-1),
            sorted: true,
        }
    }
}

/// Identical to [`close_open_fds()`], but sets the `FD_CLOEXEC` flag on the file descriptors instead
/// of closing them.
///
/// This is equivalent to `CloseFdsBuilder::new().keep_fds(keep_fds).cloexecfrom(minfd)`.
///
/// See [`CloseFdsBuilder::cloexecfrom()`] for more information.
#[inline]
pub fn set_fds_cloexec(minfd: libc::c_int, keep_fds: &[libc::c_int]) {
    CloseFdsBuilder::new().keep_fds(keep_fds).cloexecfrom(minfd)
}

/// Equivalent to `set_fds_cloexec()`, but behaves more reliably in multithreaded programs (at the
/// cost of decreased performance on some platforms).
///
/// This is equivalent to
/// `CloseFdsBuilder::new().keep_fds(keep_fds).threadsafe(true).cloexecfrom(minfd)`.
///
/// See [`CloseFdsBuilder::cloexecfrom()`] and [`FdIterBuilder::threadsafe()`] for more information.
#[inline]
pub fn set_fds_cloexec_threadsafe(minfd: libc::c_int, keep_fds: &[libc::c_int]) {
    CloseFdsBuilder::new()
        .keep_fds(keep_fds)
        .threadsafe(true)
        .cloexecfrom(minfd)
}

/// Close all open file descriptors starting at `minfd`, except for the file descriptors in
/// `keep_fds`.
///
/// This is equivalent to `CloseFdsBuilder::new().keep_fds(keep_fds).closefrom(minfd)`.
///
/// See [`CloseFdsBuilder::closefrom()`] for more information.
///
/// # Safety
///
/// See [`CloseFdsBuilder::closefrom()`].
pub unsafe fn close_open_fds(minfd: libc::c_int, keep_fds: &[libc::c_int]) {
    CloseFdsBuilder::new().keep_fds(keep_fds).closefrom(minfd)
}

#[inline]
pub(crate) fn probe() {
    close::probe();
    cloexec::probe();
}
