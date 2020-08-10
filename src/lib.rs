#![no_std]

mod externs;

/// Iterate over all open file descriptors for the current process.
#[inline]
pub fn iter_open_fds(minfd: libc::c_int) -> FdIter {
    iter_fds(minfd, false)
}

/// Identical to ``iter_open_fds()``, but may -- for efficiency -- yield invalid
/// file descriptors.
///
/// With this function, the caller is responsible for checking if the file
/// descriptors are valid.
#[inline]
pub fn iter_possible_fds(minfd: libc::c_int) -> FdIter {
    iter_fds(minfd, true)
}

/// Close all open file descriptors starting at `minfd`, except for the file
/// descriptors in `keep_fds`.
///
/// # Safety
///
/// Some objects, such as `std::fs::File`, may open file descriptors and then assume
/// that they will remain open. This function, by closing those file descriptors,
/// violates those assumptions.
///
/// This function is safe to use if it can be verified that this is not a concern.
/// (For example, it *should* be safe at startup or just before an `exec()`.)
pub unsafe fn close_open_fds(mut minfd: libc::c_int, keep_fds: &[libc::c_int]) {
    if minfd < 0 {
        minfd = 0;
    }

    let max_keep_fd = keep_fds.iter().cloned().max().unwrap_or(-1);

    #[cfg(any(
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd",
        target_os = "dragonfly"
    ))]
    if max_keep_fd < minfd {
        // On the BSDs, all the file descriptors in keep_fds are less than
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
        } else if !keep_fds.contains(&fd) {
            libc::close(fd);
        }
    }
}

fn iter_fds(mut minfd: libc::c_int, possible: bool) -> FdIter {
    if minfd < 0 {
        minfd = 0;
    }

    #[cfg(target_os = "linux")]
    let dirfd = unsafe {
        // Try /proc/self/fd on Linux

        libc::open(
            "/proc/self/fd\0".as_ptr() as *const libc::c_char,
            libc::O_RDONLY | libc::O_DIRECTORY,
        )
    };

    #[cfg(target_os = "freebsd")]
    let dirfd = {
        // On FreeBSD platforms, /dev/fd is usually a static directory with only entries
        // for 0, 1, and 2. This is obviously incorrect.
        // However, it can also be a fdescfs filesystem, in which case it's correct.
        // So we only trust /dev/fd if it's on a different device than /dev.

        let mut dev_stat = unsafe { core::mem::zeroed() };
        let mut devfd_stat = unsafe { core::mem::zeroed() };

        unsafe {
            if libc::stat("/dev\0".as_ptr() as *const libc::c_char, &mut dev_stat) == 0
                && libc::stat("/dev/fd\0".as_ptr() as *const libc::c_char, &mut devfd_stat) == 0
                && dev_stat.st_dev != devfd_stat.st_dev
            {
                // /dev and /dev/fd are on different devices; /dev/fd is probably an fdescfs

                libc::open(
                    "/dev/fd\0".as_ptr() as *const libc::c_char,
                    libc::O_RDONLY | libc::O_DIRECTORY,
                )
            } else {
                // /dev/fd is probably a static directory
                -1
            }
        }
    };

    #[cfg(target_os = "macos")]
    let dirfd = unsafe {
        // On macOS, /dev/fd is correct

        libc::open(
            "/dev/fd\0".as_ptr() as *const libc::c_char,
            libc::O_RDONLY | libc::O_DIRECTORY,
        )
    };

    FdIter {
        minfd,
        curfd: -1,
        possible,
        maxfd: -1,
        #[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd"))]
        dirfd,
        #[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd"))]
        dirents: [0; core::mem::size_of::<RawDirent>()],
        #[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd"))]
        dirent_nbytes: 0,
        #[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd"))]
        dirent_offset: 0,
    }
}

#[cfg(target_os = "linux")]
type RawDirent = libc::dirent64;
#[cfg(target_os = "linux")]
unsafe fn getdents(fd: libc::c_int, buf: &mut [u8]) -> isize {
    libc::syscall(
        libc::SYS_getdents64,
        fd as libc::c_uint,
        buf.as_mut_ptr(),
        buf.len(),
    ) as isize
}

#[cfg(target_os = "freebsd")]
#[repr(C)]
struct RawDirent {
    pub d_fileno: libc::ino_t,
    pub d_off: libc::off_t,
    pub d_reclen: u16,
    pub d_type: u8,
    d_pad0: u8,
    pub d_namlen: u16,
    d_pad1: u16,
    pub d_name: [libc::c_char; 256],
}
#[cfg(target_os = "freebsd")]
unsafe fn getdents(fd: libc::c_int, buf: &mut [u8]) -> isize {
    externs::getdirentries(
        fd,
        buf.as_mut_ptr() as *mut libc::c_char,
        buf.len(),
        core::ptr::null_mut(),
    ) as isize
}

#[cfg(target_os = "macos")]
type RawDirent = libc::dirent;
#[cfg(target_os = "macos")]
unsafe fn getdents(fd: libc::c_int, buf: &mut [u8]) -> isize {
    let mut offset: libc::off_t = 0;

    // 344 is getdents64()
    libc::syscall(344, fd, buf.as_mut_ptr(), buf.len(), &mut offset) as isize
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd"))]
fn parse_int_bytes<I: Iterator<Item = u8>>(it: I) -> Option<libc::c_int> {
    let mut num: libc::c_int = 0;
    let mut seen_any = false;

    for ch in it {
        if ch >= b'0' && ch <= b'9' {
            num = num.checked_mul(10)?.checked_add((ch - b'0') as libc::c_int)?;
            seen_any = true;
        } else {
            return None;
        }
    }

    if seen_any {
        Some(num)
    } else {
        None
    }
}

pub struct FdIter {
    minfd: libc::c_int,
    curfd: libc::c_int,
    possible: bool,
    maxfd: libc::c_int,
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd"))]
    dirfd: libc::c_int,
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd"))]
    dirents: [u8; core::mem::size_of::<RawDirent>()],
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd"))]
    dirent_nbytes: usize,
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd"))]
    dirent_offset: usize,
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
            // we can use an fcntl() loop to get the maximum open file descriptor.

            let mib = [libc::CTL_KERN, libc::KERN_PROC, externs::KERN_PROC_NFDS, 0];
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

        let fdlimit = unsafe { libc::sysconf(libc::_SC_OPEN_MAX) };
        if fdlimit > 0 && fdlimit <= 65536 {
            return fdlimit as libc::c_int - 1;
        }

        65536
    }

    #[cfg(target_os = "freebsd")]
    fn nfds_to_maxfd(nfds: libc::c_int) -> Option<libc::c_int> {
        if nfds == 0 {
            // No open file descriptors -- nothing to do!
            return Some(0);
        }

        if nfds >= 100 {
            // We're probably better off just iterating through
            return None;
        }

        let mut nfds_found = 0;

        // We know the number of open file descriptors; let's use that to
        // try to find the largest open file descriptor.

        for fd in 0..(nfds * 3) {
            if unsafe { libc::fcntl(fd, libc::F_GETFD) } >= 0 {
                nfds_found += 1;

                if nfds_found >= nfds {
                    // We found the largest open file descriptor
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

    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd"))]
    fn next_dirfd(&mut self) -> Result<Option<libc::c_int>, ()> {
        loop {
            if self.dirent_offset >= self.dirent_nbytes {
                let nbytes = unsafe { getdents(self.dirfd, &mut self.dirents) };

                match nbytes.cmp(&0) {
                    // >= 0 -> Found at least one entry
                    core::cmp::Ordering::Greater => {
                        self.dirent_nbytes = nbytes as usize;
                        self.dirent_offset = 0;
                    }
                    // 0 -> EOF
                    core::cmp::Ordering::Equal => return Ok(None),
                    // < 0 -> Error
                    _ => return Err(()),
                }
            }

            // Note: We're assuming the OS will return the file descriptors in ascending order.
            // This's probably the case, considering that the kernel probably stores them in that
            // order.

            #[allow(clippy::cast_ptr_alignment)] // We trust the kernel not to make us segfault
            let entry =
                unsafe { &*(self.dirents.as_ptr().add(self.dirent_offset) as *const RawDirent) };
            self.dirent_offset += entry.d_reclen as usize;

            if let Some(fd) = parse_int_bytes(
                entry
                    .d_name
                    .iter()
                    .take_while(|c| **c != 0)
                    .map(|c| *c as u8),
            ) {
                if fd >= self.minfd && fd != self.dirfd {
                    self.curfd = fd;
                    return Ok(Some(fd));
                }
            }
        }
    }
}

impl Iterator for FdIter {
    type Item = libc::c_int;

    fn next(&mut self) -> Option<Self::Item> {
        #[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd"))]
        if self.dirfd >= 0 {
            if let Ok(res) = self.next_dirfd() {
                return res;
            } else {
                unsafe {
                    libc::close(self.dirfd);
                }
                self.dirfd = -1;
            }
        }

        if self.curfd < 0 {
            self.curfd = self.minfd;
        }

        let maxfd = self.get_maxfd();

        while self.curfd <= maxfd {
            let fd = self.curfd;

            self.curfd += 1;

            if self.possible || unsafe { libc::fcntl(fd, libc::F_GETFD) } >= 0 {
                return Some(fd);
            }
        }

        None
    }
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd"))]
impl Drop for FdIter {
    fn drop(&mut self) {
        if self.dirfd >= 0 {
            unsafe {
                libc::close(self.dirfd);
            }
        }
    }
}
