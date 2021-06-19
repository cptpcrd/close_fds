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
type RawDirent = crate::sys::dirent;
#[cfg(target_os = "freebsd")]
#[inline]
unsafe fn getdents(fd: libc::c_int, buf: &mut [u8]) -> isize {
    crate::sys::getdirentries(
        fd,
        buf.as_mut_ptr() as *mut libc::c_char,
        buf.len(),
        core::ptr::null_mut(),
    ) as isize
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
type RawDirent = libc::dirent;
#[cfg(any(target_os = "macos", target_os = "ios"))]
#[inline]
unsafe fn getdents(fd: libc::c_int, buf: &mut [u8]) -> isize {
    let mut offset = core::mem::MaybeUninit::<libc::off_t>::uninit();

    libc::syscall(
        crate::sys::SYS_GETDIRENTRIES64,
        fd,
        buf.as_mut_ptr(),
        buf.len(),
        offset.as_mut_ptr(),
    ) as isize
}

#[cfg(any(target_os = "netbsd", target_os = "solaris", target_os = "illumos"))]
type RawDirent = libc::dirent;
#[cfg(any(target_os = "netbsd", target_os = "solaris", target_os = "illumos"))]
#[inline]
unsafe fn getdents(fd: libc::c_int, buf: &mut [u8]) -> isize {
    crate::sys::getdents(fd, buf.as_mut_ptr() as *mut _, buf.len()) as isize
}

fn parse_int_bytes<I: Iterator<Item = u8>>(it: I) -> Option<libc::c_int> {
    let mut num: libc::c_int = 0;
    let mut seen_any = false;

    for ch in it {
        if (b'0'..=b'9').contains(&ch) {
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

#[repr(align(8))]
struct DirFdIterBuf {
    data: [u8; core::mem::size_of::<RawDirent>()],
}

pub struct DirFdIter {
    minfd: libc::c_int,
    // This is ONLY < 0 if the iterator was exhausted during iteration and has now been closed.
    dirfd: libc::c_int,
    dirent_buf: DirFdIterBuf,
    dirent_nbytes: usize,
    dirent_offset: usize,
}

impl DirFdIter {
    #[inline]
    pub fn open(minfd: libc::c_int) -> Option<Self> {
        #[cfg(target_os = "linux")]
        let dirfd = unsafe {
            // Try /proc/self/fd on Linux.
            // However, on WSL 1, getdents64() doesn't always return the entries in order, and also
            // seems to skip some file descriptors. So skip it on WSL 1.

            if crate::util::is_wsl_1() {
                return None;
            }

            libc::open(
                "/proc/self/fd\0".as_ptr() as *const libc::c_char,
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
            )
        };

        #[cfg(target_os = "freebsd")]
        let dirfd = {
            // On FreeBSD platforms, /dev/fd is usually a static directory with only entries
            // for 0, 1, and 2. This is obviously incorrect.
            // However, it can also be a fdescfs filesystem, in which case it's correct.
            // So we only trust /dev/fd if it's on a different device than /dev.

            let mut dev_stat = core::mem::MaybeUninit::uninit();
            let mut devfd_stat = core::mem::MaybeUninit::uninit();

            unsafe {
                let dirfd = libc::open(
                    "/dev/fd\0".as_ptr() as *const _,
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
                );

                if dirfd >= 0
                    && (libc::stat("/dev\0".as_ptr() as *const _, dev_stat.as_mut_ptr()) != 0
                        || libc::fstat(dirfd, devfd_stat.as_mut_ptr()) != 0
                        || dev_stat.assume_init().st_dev == devfd_stat.assume_init().st_dev)
                {
                    // We were able to open /dev/fd. However, one of the following happened:
                    // 1. We weren't able to stat() /dev.
                    // 2. We weren't able to fstat() dirfd (which is open to /dev/fd).
                    // 3. /dev's device number is the same as /dev/fd's device number.
                    //
                    // Case (3) means that /dev/fd is almost definitely NOT an fdescfs, so we can't
                    // trust it. Cases (1) and (2) mean that we can't tell, so we must
                    // conservatively assume that it isn't an fdescfs.
                    libc::close(dirfd);
                    -1
                } else {
                    dirfd
                }
            }
        };

        #[cfg(target_os = "netbsd")]
        let dirfd = unsafe {
            // On NetBSD, /dev/fd is a static directory, but /proc/self/fd is correct

            libc::open(
                "/proc/self/fd\0".as_ptr() as *const libc::c_char,
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
            )
        };

        #[cfg(any(target_os = "macos", target_os = "ios"))]
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
                dirent_buf: DirFdIterBuf {
                    data: [0; core::mem::size_of::<RawDirent>()],
                },
                dirent_nbytes: 0,
                dirent_offset: 0,
            })
        } else {
            None
        }
    }

    #[inline]
    unsafe fn get_entry_info(&self, offset: usize) -> (Option<libc::c_int>, usize) {
        #[allow(clippy::cast_ptr_alignment)] // We trust the kernel not to make us segfault
        let entry = &*(self.dirent_buf.data.as_ptr().add(offset) as *const RawDirent);

        cfg_if::cfg_if! {
            if #[cfg(any(
                target_os = "freebsd",
                target_os = "netbsd",
                target_os = "macos",
                target_os = "ios",
            ))] {
                // Trailing NUL
                debug_assert_eq!(entry.d_name[entry.d_namlen as usize], 0);
                // No NULs before that
                debug_assert_eq!(
                    entry.d_name[..entry.d_namlen as usize]
                        .iter()
                        .position(|&c| c == 0),
                    None
                );

                let fd = parse_int_bytes(
                    entry.d_name[..entry.d_namlen as usize]
                        .iter()
                        .map(|c| *c as u8),
                );
            } else {
                let fd = parse_int_bytes(
                    entry
                        .d_name
                        .iter()
                        .take_while(|c| **c != 0)
                        .map(|c| *c as u8),
                );
            }
        }

        (fd, entry.d_reclen as usize)
    }

    #[inline]
    pub fn next(&mut self) -> Result<Option<libc::c_int>, ()> {
        if self.dirfd < 0 {
            // Exhausted
            return Ok(None);
        }

        loop {
            if self.dirent_offset >= self.dirent_nbytes {
                let nbytes = unsafe { getdents(self.dirfd, &mut self.dirent_buf.data) };

                match nbytes.cmp(&0) {
                    // > 0 -> Found at least one entry
                    core::cmp::Ordering::Greater => {
                        self.dirent_nbytes = nbytes as usize;
                        self.dirent_offset = 0;
                    }

                    // 0 -> EOF
                    core::cmp::Ordering::Equal => {
                        // Close the directory file descriptor and return None
                        unsafe {
                            libc::close(self.dirfd);
                        }
                        self.dirfd = -1;
                        return Ok(None);
                    }

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

    #[inline]
    pub fn size_hint(&self) -> (usize, Option<usize>) {
        if self.dirfd < 0 {
            // Exhausted
            return (0, Some(0));
        }

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
    #[inline]
    fn drop(&mut self) {
        // Close the directory file descriptor if it's still open
        if self.dirfd >= 0 {
            unsafe {
                libc::close(self.dirfd);
            }
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
