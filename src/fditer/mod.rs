#[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd"))]
mod dirfd;

pub fn iter_fds(mut minfd: libc::c_int, possible: bool) -> FdIter {
    if minfd < 0 {
        minfd = 0;
    }

    FdIter {
        curfd: minfd,
        possible,
        maxfd: -1,
        #[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd"))]
        dirfd_iter: dirfd::DirFdIter::open(minfd),
    }
}

pub struct FdIter {
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd"))]
    dirfd_iter: Option<dirfd::DirFdIter>,
    curfd: libc::c_int,
    possible: bool,
    maxfd: libc::c_int,
}

impl FdIter {
    fn get_maxfd_direct() -> libc::c_int {
        #[cfg(target_os = "netbsd")]
        {
            // NetBSD allows us to get the maximum open file descriptor

            let maxfd = unsafe { libc::fcntl(0, libc::F_MAXFD) };
            if maxfd >= 0 {
                return maxfd;
            }
        }

        #[cfg(target_os = "freebsd")]
        {
            // On FreeBSD, we can get the *number* of open file descriptors. From that,
            // we can use an is_fd_valid() loop to get the maximum open file descriptor.

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
                && nfds >= 0
            {
                if let Some(maxfd) = Self::nfds_to_maxfd(nfds) {
                    return maxfd;
                }
            }
        }

        #[cfg(unix)]
        let fdlimit = unsafe { libc::sysconf(libc::_SC_OPEN_MAX) };
        #[cfg(windows)]
        let fdlimit = unsafe { crate::externs::getmaxstdio() };

        // Clamp it at 65536 because that's a LOT of file descriptors
        // Also don't trust values below 1024 (512 on Windows)

        #[cfg(unix)]
        const LOWER_FDLIMIT: libc::c_long = 1024;
        #[cfg(windows)]
        const LOWER_FDLIMIT: libc::c_int = 512;

        if fdlimit <= 65536 && fdlimit >= LOWER_FDLIMIT {
            return fdlimit as libc::c_int - 1;
        }

        65536
    }

    #[cfg(target_os = "freebsd")]
    fn nfds_to_maxfd(nfds: libc::c_int) -> Option<libc::c_int> {
        // Given the number of open file descriptors, return the largest
        // open file descriptor (or None if it can't be reasonably determined).

        if nfds == 0 {
            // No open file descriptors -- nothing to do!
            return Some(-1);
        }

        if nfds >= 100 {
            // We're probably better off just iterating through
            return None;
        }

        let mut nfds_found = 0;

        // We know the number of open file descriptors; let's use that to
        // try to find the largest open file descriptor.

        for fd in 0..(nfds * 3) {
            if crate::util::is_fd_valid(fd) {
                // Valid file descriptor
                nfds_found += 1;

                if nfds_found >= nfds {
                    // We've found all the open file descriptors.
                    // We now know that the current `fd` is the largest open
                    // file descriptor
                    return Some(fd);
                }
            }
        }

        // We haven't found all of the open file descriptors yet, but
        // it seems like we *should* have.
        //
        // This usually means one of two things:
        //
        // 1. The process opened a large number of file descriptors, then
        //    closed many of them. However, it left several of the high-numbered
        //    file descriptors open. (For example, consider the case where the
        //    open file descriptors are 0, 1, 2, 50, and 100. nfds=5, but the
        //    highest open file descriptor is actually 100!)
        // 2. The 'nfds' method is vulnerable to a race condition: if a
        //    file descriptor is closed after the number of open file descriptors
        //    has been obtained, but before the fcntl() loop reaches that file
        //    descriptor, then the loop will never find all of the open file
        //    descriptors because it will be stuck at n_fds_found = nfds-1.
        //    If this happens, without this check the loop would essentially become
        //    an infinite loop.
        //    (For example, consider the case where the open file descriptors are
        //    0, 1, 2, and 3. If file descriptor 3 is closed before the fd=3
        //    iteration, then we will be stuck at n_fds_found=3 and will never
        //    be able to find the 4th file descriptor.)
        //
        // Error on the side of caution (case 2 is dangerous) and let the caller
        // select another method.

        None
    }

    fn get_maxfd(&mut self) -> libc::c_int {
        if self.maxfd < 0 {
            self.maxfd = Self::get_maxfd_direct();
        }

        self.maxfd
    }
}

impl Iterator for FdIter {
    type Item = libc::c_int;

    fn next(&mut self) -> Option<Self::Item> {
        #[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd"))]
        if let Some(dfd_iter) = self.dirfd_iter.as_mut() {
            // Try iterating using the directory file descriptor we opened

            match dfd_iter.next() {
                Ok(Some(fd)) => {
                    debug_assert!(fd >= self.curfd);

                    // We set self.curfd so that if something goes wrong we can switch to the maxfd
                    // loop without repeating file descriptors
                    self.curfd = fd;

                    return Some(fd);
                }

                Ok(None) => return None,

                Err(_) => {
                    // Something went wrong. Close the directory file descriptor and reset it
                    // so we don't try to use it again.
                    drop(self.dirfd_iter.take());
                }
            }
        }

        let maxfd = self.get_maxfd();

        while self.curfd <= maxfd {
            // Get the current file descriptor
            let fd = self.curfd;

            // Increment it for next time
            self.curfd += 1;

            // If we weren't given the "possible" flag, we have to check that it's a valid
            // file descriptor first.
            if self.possible || crate::util::is_fd_valid(fd) {
                return Some(fd);
            }
        }

        // Exhausted the range
        None
    }
}
