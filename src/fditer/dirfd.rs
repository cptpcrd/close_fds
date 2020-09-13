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
    crate::externs::getdirentries(
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

#[cfg(any(target_os = "solaris", target_os = "illumos"))]
type RawDirent = libc::dirent;
#[cfg(any(target_os = "solaris", target_os = "illumos"))]
#[inline]
unsafe fn getdents(fd: libc::c_int, buf: &mut [u8]) -> isize {
    crate::externs::getdents(fd, buf.as_mut_ptr() as *mut libc::dirent, buf.len()) as isize
}

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

pub struct DirFdIter {
    minfd: libc::c_int,
    dirfd: libc::c_int,
    dirents: [u8; core::mem::size_of::<RawDirent>()],
    dirent_nbytes: usize,
    dirent_offset: usize,
}

impl DirFdIter {
    pub fn open(minfd: libc::c_int) -> Option<Self> {
        #[cfg(target_os = "linux")]
        let dirfd = unsafe {
            // Try /proc/self/fd on Linux

            if crate::util::is_wsl_1() {
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

            let dev_path_ptr = "/dev\0".as_ptr() as *const libc::c_char;
            let devfd_path_ptr = "/dev/fd\0".as_ptr() as *const libc::c_char;

            let mut dev_stat = unsafe { core::mem::zeroed() };
            let mut devfd_stat = unsafe { core::mem::zeroed() };

            unsafe {
                if libc::stat(dev_path_ptr, &mut dev_stat) == 0
                    && libc::stat(devfd_path_ptr, &mut devfd_stat) == 0
                    && dev_stat.st_dev != devfd_stat.st_dev
                {
                    // /dev and /dev/fd are on different devices; /dev/fd is probably an fdescfs

                    libc::open(
                        devfd_path_ptr,
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

        #[cfg(any(target_os = "solaris", target_os = "illumos"))]
        let dirfd = unsafe {
            // On Solaris/Illumos, both /dev/fd and /proc/self/fd should be correct
            // So let's try /dev/fd, then /proc/self/fd if that fails

            let fd = libc::open(
                "/dev/fd\0".as_ptr() as *const libc::c_char,
                libc::O_RDONLY | libc::O_CLOEXEC,
            );

            if fd < 0 {
                libc::open(
                    "/proc/self/fd\0".as_ptr() as *const libc::c_char,
                    libc::O_RDONLY | libc::O_CLOEXEC,
                )
            } else {
                fd
            }
        };

        if dirfd >= 0 {
            Some(Self {
                minfd,
                dirfd,
                dirents: [0; core::mem::size_of::<RawDirent>()],
                dirent_nbytes: 0,
                dirent_offset: 0,
            })
        } else {
            None
        }
    }

    unsafe fn get_entry_info(&self, offset: usize) -> (Option<libc::c_int>, usize) {
        #[allow(clippy::cast_ptr_alignment)] // We trust the kernel not to make us segfault
        let entry = &*(self.dirents.as_ptr().add(offset) as *const RawDirent);

        let fd = parse_int_bytes(
            entry
                .d_name
                .iter()
                .take_while(|c| **c != 0)
                .map(|c| *c as u8),
        );

        (fd, entry.d_reclen as usize)
    }

    pub fn next(&mut self) -> Result<Option<libc::c_int>, ()> {
        loop {
            if self.dirent_offset >= self.dirent_nbytes {
                let nbytes = unsafe { getdents(self.dirfd, &mut self.dirents) };

                match nbytes.cmp(&0) {
                    // > 0 -> Found at least one entry
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

            let (fd, reclen) = unsafe { self.get_entry_info(self.dirent_offset) };

            // Adjust the offset for next time
            self.dirent_offset += reclen as usize;

            // Were we able to parse it?
            if let Some(fd) = fd {
                // Only return it if 1) it's in the correct range and 2) it's not
                // the directory file descriptor we're using

                if fd >= self.minfd && fd != self.dirfd {
                    return Ok(Some(fd));
                }
            }
        }
    }

    pub fn size_hint(&self) -> (usize, Option<usize>) {
        // Let's try to determine a lower limit

        let mut low = 0;
        let mut dirent_offset = self.dirent_offset;

        while dirent_offset < self.dirent_nbytes {
            // Get the next entry
            let (fd, reclen) = unsafe { self.get_entry_info(dirent_offset) };

            // Adjust the offset for next time
            dirent_offset += reclen as usize;

            // Were we able to parse it?
            if let Some(fd) = fd {
                // Sanity check
                debug_assert!(fd >= self.minfd);

                if fd != self.dirfd {
                    // We found one
                    low += 1;
                }
            }
        }

        (low, Some(libc::c_int::MAX as usize))
    }
}

impl Drop for DirFdIter {
    fn drop(&mut self) {
        // Close the directory file descriptor
        unsafe {
            libc::close(self.dirfd);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use core::fmt::Write;

    pub struct BufWriter {
        pub buf: [u8; 80],
        pub i: usize,
    }

    impl BufWriter {
        pub fn new() -> Self {
            Self { buf: [0; 80], i: 0 }
        }

        pub fn iter_bytes(&'_ self) -> impl Iterator<Item = u8> + '_ {
            self.buf.iter().take(self.i).cloned()
        }
    }

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
