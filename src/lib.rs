#![no_std]

mod externs;

/// Iterate over all open file descriptors for the current process, starting
/// at `minfd`.
#[inline]
pub fn iter_open_fds(minfd: libc::c_int) -> FdIter {
    iter_fds(minfd, false)
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
/// Note that this example is NOT intended to imply that you *should* be calling
/// `metadata()` (or any other methods) on random file descriptors.
#[inline]
pub fn iter_possible_fds(minfd: libc::c_int) -> FdIter {
    iter_fds(minfd, true)
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

    let (max_keep_fd, fds_sorted) = inspect_keep_fds(keep_fds);

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
        } else if !check_should_keep(&mut keep_fds, fd, fds_sorted) {
            // Close it if it's not in keep_fds
            libc::close(fd);
        }
    }
}

/// Identical to `close_open_fds()`, but sets the `FD_CLOEXEC` flag on the file descriptors instead
/// of closing them. (Unix-only)
///
/// On some platforms (most notably, Linux without `/proc` mounted and some of the BSDs), this is
/// significantly less efficient than `close_open_fds()`, and use of that function should be
/// preferred when possible.
#[cfg(unix)]
pub fn set_fds_cloexec(minfd: libc::c_int, mut keep_fds: &[libc::c_int]) {
    let (max_keep_fd, fds_sorted) = inspect_keep_fds(keep_fds);

    for fd in iter_possible_fds(minfd) {
        if fd > max_keep_fd || !check_should_keep(&mut keep_fds, fd, fds_sorted) {
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

fn inspect_keep_fds(keep_fds: &[libc::c_int]) -> (libc::c_int, bool) {
    // Get the maximum file descriptor from the list, and also check if it's
    // sorted.

    let mut max_keep_fd = -1;
    let mut last_fd = -1;
    let mut fds_sorted = true;
    for fd in keep_fds.iter().cloned() {
        // Check for a new maximum file descriptor
        if fd > max_keep_fd {
            max_keep_fd = fd;
        }

        if last_fd > fd {
            // Out of order
            fds_sorted = false;
        }
        last_fd = fd;
    }

    (max_keep_fd, fds_sorted)
}

fn check_should_keep(keep_fds: &mut &[libc::c_int], fd: libc::c_int, fds_sorted: bool) -> bool {
    if fds_sorted {
        // If the file descriptor list is sorted, we can do a more efficient
        // lookup

        // Skip over any elements less than the current file descriptor.
        // For example if keep_fds is [0, 1, 4, 5] and fd is either 3 or 4,
        // we can skip over 0 and 1 -- those cases have been covered already.
        if let Some(index) = keep_fds.iter().position(|&x| x >= fd) {
            *keep_fds = &(*keep_fds)[index..];
        }

        // Is the file descriptor we're searching for present?
        keep_fds.first() == Some(&fd)
    } else {
        // Otherwise, we have to fall back on contains()
        keep_fds.contains(&fd)
    }
}

#[cfg(target_os = "linux")]
fn is_wsl_1() -> bool {
    // It seems that on WSL 1 the kernel "release name" ends with "-Microsoft", and on WSL 2 the
    // release name ends with "-microsoft-standard". So we look for "Microsoft" at the end.

    let mut uname: libc::utsname = unsafe { core::mem::zeroed() };

    if unsafe { libc::uname(&mut uname) } == 0 {
        // Look for the string "Microsoft" at the end
        // Because we don't have libstd, and because libc::c_char might be either
        // an i8 or a u8, this is actually kind of hard.

        if let Some(nul_index) = uname.release.iter().position(|c| *c == 0) {
            if let Some(m_index) = uname
                .release
                .iter()
                .take(nul_index)
                .position(|c| *c as u8 == b'M')
            {
                if nul_index - m_index == b"Microsoft".len()
                    && uname.release[m_index..nul_index]
                        .iter()
                        .zip(b"Microsoft".iter())
                        .all(|(&a, &b)| a as u8 == b)
                {
                    return true;
                }
            }
        }
    }

    false
}

fn iter_fds(mut minfd: libc::c_int, possible: bool) -> FdIter {
    if minfd < 0 {
        minfd = 0;
    }

    #[cfg(target_os = "linux")]
    let dirfd = unsafe {
        // Try /proc/self/fd on Linux

        if is_wsl_1() {
            // On WSL 1, getdents64() doesn't always return the entries in order.
            // It also seems to skip some file descriptors.
            // We can't trust it.

            -1
        } else {
            libc::open(
                "/proc/self/fd\0".as_ptr() as *const libc::c_char,
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
            )
        }
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
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
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
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
        )
    };

    FdIter {
        curfd: minfd,
        possible,
        maxfd: -1,
        #[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd"))]
        dirfd_iter: if dirfd >= 0 {
            Some(DirFdIter {
                minfd,
                dirfd,
                dirents: [0; core::mem::size_of::<RawDirent>()],
                dirent_nbytes: 0,
                dirent_offset: 0,
            })
        } else {
            None
        },
    }
}

#[cfg(unix)]
#[inline]
fn is_fd_valid(fd: libc::c_int) -> bool {
    unsafe { libc::fcntl(fd, libc::F_GETFD) >= 0 }
}

#[cfg(windows)]
#[inline]
fn is_fd_valid(fd: libc::c_int) -> bool {
    unsafe { libc::get_osfhandle(fd) >= 0 }
}

#[cfg(target_os = "linux")]
type RawDirent = libc::dirent64;
#[cfg(target_os = "linux")]
#[inline]
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
#[inline]
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
#[inline]
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
            num = num
                .checked_mul(10)?
                .checked_add((ch - b'0') as libc::c_int)?;
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

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd"))]
struct DirFdIter {
    minfd: libc::c_int,
    dirfd: libc::c_int,
    dirents: [u8; core::mem::size_of::<RawDirent>()],
    dirent_nbytes: usize,
    dirent_offset: usize,
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd"))]
impl DirFdIter {
    fn next(&mut self) -> Result<Option<libc::c_int>, ()> {
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

            // Adjust the offset for next time
            self.dirent_offset += entry.d_reclen as usize;

            // Try to parse the file descriptor as an integer
            if let Some(fd) = parse_int_bytes(
                entry
                    .d_name
                    .iter()
                    .take_while(|c| **c != 0)
                    .map(|c| *c as u8),
            ) {
                // Only return it if 1) it's in the correct range and 2) it's not
                // the directory file descriptor we're using

                if fd >= self.minfd && fd != self.dirfd {
                    return Ok(Some(fd));
                }
            }
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd"))]
impl Drop for DirFdIter {
    fn drop(&mut self) {
        // Close the directory file descriptor
        unsafe {
            libc::close(self.dirfd);
        }
    }
}

pub struct FdIter {
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd"))]
    dirfd_iter: Option<DirFdIter>,
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

        #[cfg(unix)]
        let fdlimit = unsafe { libc::sysconf(libc::_SC_OPEN_MAX) };
        #[cfg(windows)]
        let fdlimit = unsafe { externs::getmaxstdio() };

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
            if is_fd_valid(fd) {
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
            if self.possible || is_fd_valid(fd) {
                return Some(fd);
            }
        }

        // Exhausted the range
        None
    }
}

#[cfg(test)]
mod tests {
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd"))]
    use super::*;

    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd"))]
    use core::fmt::Write;

    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd"))]
    pub struct BufWriter {
        pub buf: [u8; 80],
        pub i: usize,
    }

    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd"))]
    impl BufWriter {
        pub fn new() -> Self {
            Self { buf: [0; 80], i: 0 }
        }

        pub fn iter_bytes(&'_ self) -> impl Iterator<Item = u8> + '_ {
            self.buf.iter().take(self.i).cloned()
        }
    }

    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd"))]
    impl Write for BufWriter {
        fn write_str(&mut self, s: &str) -> core::fmt::Result {
            if self.i + s.len() > self.buf.len() {
                return Err(core::fmt::Error);
            }

            for &ch in s.as_bytes() {
                self.buf[self.i] = ch;
                self.i += 1;
            }

            Ok(())
        }
    }

    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd"))]
    #[test]
    fn test_parse_int_bytes() {
        assert_eq!(parse_int_bytes(b"0".iter().cloned()), Some(0));
        assert_eq!(parse_int_bytes(b"10".iter().cloned()), Some(10));
        assert_eq!(parse_int_bytes(b"1423".iter().cloned()), Some(1423));

        assert_eq!(parse_int_bytes(b" 0".iter().cloned()), None);
        assert_eq!(parse_int_bytes(b"0 ".iter().cloned()), None);
        assert_eq!(parse_int_bytes(b"-1".iter().cloned()), None);
        assert_eq!(parse_int_bytes(b"+1".iter().cloned()), None);
        assert_eq!(parse_int_bytes(b"1.".iter().cloned()), None);
        assert_eq!(parse_int_bytes(b"".iter().cloned()), None);

        let mut buf = BufWriter::new();
        write!(&mut buf, "{}", libc::c_int::MAX as libc::c_uint + 1).unwrap();
        assert_eq!(parse_int_bytes(buf.iter_bytes()), None);
    }
}
