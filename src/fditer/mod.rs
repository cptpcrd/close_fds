#[cfg(any(
    target_os = "linux",
    target_os = "macos",
    target_os = "freebsd",
    target_os = "solaris",
    target_os = "illumos",
))]
mod dirfd;

#[allow(unused_variables)]
pub fn iter_fds(mut minfd: libc::c_int, possible: bool, skip_nfds: bool) -> FdIter {
    if minfd < 0 {
        minfd = 0;
    }

    FdIter {
        curfd: minfd,
        possible,
        maxfd: -1,
        #[cfg(any(target_os = "freebsd", target_os = "openbsd"))]
        skip_nfds,
        #[cfg(any(
            target_os = "linux",
            target_os = "macos",
            target_os = "freebsd",
            target_os = "solaris",
            target_os = "illumos",
        ))]
        dirfd_iter: dirfd::DirFdIter::open(minfd),
    }
}

pub struct FdIter {
    #[cfg(any(
        target_os = "linux",
        target_os = "macos",
        target_os = "freebsd",
        target_os = "solaris",
        target_os = "illumos",
    ))]
    dirfd_iter: Option<dirfd::DirFdIter>,
    curfd: libc::c_int,
    possible: bool,
    maxfd: libc::c_int,
    /// If this is true, it essentially means "don't try the 'nfds' methods of finding the maximum
    /// open file descriptor."
    /// `close_open_fds()` passes this as true on some systems becaus the system has a working
    /// closefrom() and at some point it can just close the rest of the file descriptors in one go.
    /// Additionally, the "nfds" method is not thread-safe.
    #[cfg(any(target_os = "freebsd", target_os = "openbsd"))]
    skip_nfds: bool,
}

impl FdIter {
    fn get_maxfd_direct(&self) -> libc::c_int {
        // Note: This function must NEVER return a negative value. NEVER.

        #[cfg(target_os = "netbsd")]
        unsafe {
            // NetBSD allows us to get the maximum open file descriptor

            *libc::__errno() = 0;
            let maxfd = libc::fcntl(0, libc::F_MAXFD);

            if maxfd >= 0 {
                return maxfd;
            } else if *libc::__errno() == 0 {
                // fcntl(F_MAXFD) actually succeeded and returned -1, which means that no file
                // descriptors are open.
                // We can't return -1 because that's the special value used to signify that maxfd is
                // unknown, but returning 0 will get us close.
                return 0;
            }
        }

        #[cfg(target_os = "freebsd")]
        if !self.skip_nfds {
            // On FreeBSD, we can get the *number* of open file descriptors. From that, we can use
            // an is_fd_valid() loop to get the maximum open file descriptor.

            let mib = [
                libc::CTL_KERN,
                libc::KERN_PROC,
                crate::externs::KERN_PROC_NFDS,
                0,
            ];
            let mut nfds: libc::c_int = 0;
            let mut oldlen = core::mem::size_of::<libc::c_int>();

            if unsafe {
                libc::sysctl(
                    mib.as_ptr(),
                    mib.len() as libc::c_uint,
                    &mut nfds as *mut libc::c_int as *mut libc::c_void,
                    &mut oldlen,
                    core::ptr::null(),
                    0,
                )
            } == 0
            {
                if let Some(maxfd) = Self::nfds_to_maxfd(nfds) {
                    return maxfd;
                }
            }
        }

        #[cfg(target_os = "openbsd")]
        if !self.skip_nfds {
            // On OpenBSD, we have a similar situation as with FreeBSD -- we can get the *number*
            // of open file descriptors, and perhaps work from that to get the maximum open file
            // descriptor.

            if let Some(maxfd) = Self::nfds_to_maxfd(unsafe { crate::externs::getdtablecount() }) {
                return maxfd;
            }
        }

        let fdlimit = unsafe { libc::sysconf(libc::_SC_OPEN_MAX) };

        // Clamp it at 65536 because that's a LOT of file descriptors
        // Also don't trust values below 1024

        fdlimit.max(1024).min(65536) as libc::c_int - 1
    }

    #[cfg(any(target_os = "freebsd", target_os = "openbsd"))]
    #[inline]
    fn nfds_to_maxfd(nfds: libc::c_int) -> Option<libc::c_int> {
        // Given the number of open file descriptors, return the largest open file descriptor (or
        // None if it can't be reasonably determined).

        if nfds == 0 {
            // No open file descriptors -- nothing to do!
            // We can't return -1 because that's the special value used to signify that maxfd is
            // unknown, but returning 0 will get us close.
            return Some(0);
        } else if nfds < 0 {
            // Probably failure of the underlying function
            return None;
        } else if nfds >= 100 {
            // We're probably better off just iterating through
            return None;
        }

        let mut nfds_found = 0;

        // We know the number of open file descriptors; let's use that to try to find the largest
        // open file descriptor.

        for fd in 0..(nfds * 2) {
            if crate::util::is_fd_valid(fd) {
                // Valid file descriptor
                nfds_found += 1;

                if nfds_found >= nfds {
                    // We've found all the open file descriptors.
                    // We now know that the current `fd` is the largest open file descriptor
                    return Some(fd);
                }
            }
        }

        // We haven't found all of the open file descriptors yet, but it seems like we *should*
        // have.
        //
        // This usually means one of two things:
        //
        // 1. The process opened a large number of file descriptors, then closed many of them.
        //    However, it left several of the high-numbered file descriptors open. (For example,
        //    consider the case where the open file descriptors are 0, 1, 2, 50, and 100. nfds=5,
        //    but the highest open file descriptor is actually 100!)
        // 2. The 'nfds' method is vulnerable to a race condition: if a file descriptor is closed
        //    after the number of open file descriptors has been obtained, but before the fcntl()
        //    loop reaches that file descriptor, then the loop will never find all of the open file
        //    descriptors because it will be stuck at n_fds_found = nfds-1.
        //    If this happens, without this check the loop would essentially become an infinite
        //    loop.
        //    (For example, consider the case where the open file descriptors are 0, 1, 2, and 3. If
        //    file descriptor 3 is closed before the fd=3 iteration, then we will be stuck at
        //    n_fds_found=3 and will never be able to find the 4th file descriptor.)
        //
        // Error on the side of caution (case 2 is dangerous) and let the caller select another
        // method.

        None
    }

    #[inline]
    fn get_maxfd(&mut self) -> libc::c_int {
        if self.maxfd < 0 {
            self.maxfd = self.get_maxfd_direct();

            // Why is this here? Well, if get_maxfd_direct() ever returns a value < 0, future calls
            // to get_maxfd() would cause self.maxfd to be recomputed.
            //
            // This would violate the guarantee that FdIter is fused, since that guarantee is based
            // on the assumption that self.maxfd will NEVER CHANGE once it has been determined. It
            // would also cause incorrect behavior from methods such as size_hint() and count(),
            // which make similar assumptions about self.maxfd.
            debug_assert!(self.maxfd >= 0);
        }

        self.maxfd
    }
}

impl Iterator for FdIter {
    type Item = libc::c_int;

    fn next(&mut self) -> Option<Self::Item> {
        #[cfg(any(
            target_os = "linux",
            target_os = "macos",
            target_os = "freebsd",
            target_os = "solaris",
            target_os = "illumos",
        ))]
        if let Some(dfd_iter) = self.dirfd_iter.as_mut() {
            // Try iterating using the directory file descriptor we opened

            match dfd_iter.next() {
                Ok(Some(fd)) => {
                    debug_assert!(fd >= self.curfd);

                    // We set self.curfd so that if something goes wrong we can switch to the maxfd
                    // loop without repeating file descriptors
                    self.curfd = fd + 1;

                    return Some(fd);
                }

                Ok(None) => return None,

                // Something went wrong. Close the directory file descriptor and fall back on a
                // maxfd loop
                Err(_) => self.dirfd_iter = None,
            }
        }

        let maxfd = self.get_maxfd();

        while self.curfd <= maxfd {
            // Get the current file descriptor
            let fd = self.curfd;

            // Increment it for next time
            self.curfd += 1;

            // If we weren't given the "possible" flag, we have to check that it's a valid file
            // descriptor first.
            if self.possible || crate::util::is_fd_valid(fd) {
                return Some(fd);
            }
        }

        // Exhausted the range
        None
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        #[cfg(any(
            target_os = "linux",
            target_os = "macos",
            target_os = "freebsd",
            target_os = "solaris",
            target_os = "illumos",
        ))]
        if let Some(dfd_iter) = self.dirfd_iter.as_ref() {
            // Delegate to the directory file descriptor
            return dfd_iter.size_hint();
        }

        if self.maxfd >= 0 {
            // maxfd is set; we can give an upper bound by comparing to curfd
            let diff = (self.maxfd as usize + 1).saturating_sub(self.curfd as usize);

            // If we were given the "possible" flag, then this is also the lower limit.
            (if self.possible { diff } else { 0 }, Some(diff))
        } else {
            // Unknown
            (0, Some(libc::c_int::MAX as usize))
        }
    }

    #[inline]
    fn count(mut self) -> usize {
        #[cfg(any(
            target_os = "linux",
            target_os = "macos",
            target_os = "freebsd",
            target_os = "solaris",
            target_os = "illumos",
        ))]
        let know_exact = self.dirfd_iter.is_none() && self.possible;

        #[cfg(not(any(
            target_os = "linux",
            target_os = "macos",
            target_os = "freebsd",
            target_os = "solaris",
            target_os = "illumos",
        )))]
        let know_exact = self.possible;

        if know_exact {
            // We know the exact count
            (self.get_maxfd() as usize + 1).saturating_sub(self.curfd as usize)
        } else {
            let mut count = 0;
            while self.next().is_some() {
                count += 1;
            }
            count
        }
    }

    #[inline]
    fn min(mut self) -> Option<Self::Item> {
        self.next()
    }

    #[inline]
    fn max(self) -> Option<Self::Item> {
        self.last()
    }
}

impl core::iter::FusedIterator for FdIter {}

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
        test_size_hint_generic(iter_fds(0, false, false));
        test_size_hint_generic(iter_fds(0, false, true));

        let fds = open_files();
        test_size_hint_generic(iter_fds(0, false, false));
        test_size_hint_generic(iter_fds(0, false, true));
        unsafe {
            close_files(&fds);
        }
    }

    #[test]
    fn test_size_hint_possible() {
        test_size_hint_generic(iter_fds(0, true, false));
        test_size_hint_generic(iter_fds(0, true, true));

        let fds = open_files();
        test_size_hint_generic(iter_fds(0, true, false));
        test_size_hint_generic(iter_fds(0, true, true));
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
        test_fused_generic(iter_fds(0, false, false));
        test_fused_generic(iter_fds(0, false, true));

        let fds = open_files();
        test_fused_generic(iter_fds(0, false, false));
        test_fused_generic(iter_fds(0, false, true));
        unsafe {
            close_files(&fds);
        }
    }

    #[test]
    fn test_fused_possible() {
        test_fused_generic(iter_fds(0, true, false));
        test_fused_generic(iter_fds(0, true, true));

        let fds = open_files();
        test_fused_generic(iter_fds(0, true, false));
        test_fused_generic(iter_fds(0, true, true));
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
