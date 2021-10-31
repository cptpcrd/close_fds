pub fn inspect_keep_fds(keep_fds: &[libc::c_int]) -> (libc::c_int, bool) {
    // Get the maximum file descriptor from the list, and also check if it's sorted.

    let mut max_keep_fd = libc::c_int::MIN;
    let mut last_fd = libc::c_int::MIN;
    let mut fds_sorted = true;

    for fd in keep_fds.iter().cloned() {
        // Check for a new maximum file descriptor
        if fd > max_keep_fd {
            max_keep_fd = fd;
            debug_assert!(last_fd <= fd);
        } else if last_fd > fd {
            // Out of order
            fds_sorted = false;
        }

        last_fd = fd;
    }

    (max_keep_fd, fds_sorted)
}

pub fn simplify_keep_fds<'a>(
    mut keep_fds: &'a [libc::c_int],
    fds_sorted: bool,
    minfd: &mut libc::c_int,
) -> &'a [libc::c_int] {
    use core::cmp::Ordering;

    if fds_sorted {
        // Example: specifying keep_fds=[3, 4, 6, 7]; minfd=3 has the same result as specifying
        // keep_fds=[6, 7]; minfd=5.
        // In some cases, this translation may reduce the number of syscalls and/or eliminate the
        // need to call iter_fds() in the first place.

        while let Some((first, rest)) = keep_fds.split_first() {
            match first.cmp(minfd) {
                // keep_fds[0] > minfd
                // No further simplification can be done
                Ordering::Greater => break,

                // keep_fds[0] == minfd
                // We can remove keep_fds[0] and increment minfd
                Ordering::Equal => {
                    keep_fds = rest;
                    *minfd += 1;
                }

                // keep_fds[0] < minfd
                // We can remove keep_fds[0]
                Ordering::Less => keep_fds = rest,
            }
        }
    }

    keep_fds
}

pub fn check_should_keep(keep_fds: &mut &[libc::c_int], fd: libc::c_int, fds_sorted: bool) -> bool {
    if fds_sorted {
        // If the file descriptor list is sorted, we can do a more efficient lookup

        // Skip over any elements less than the current file descriptor.
        // For example if keep_fds is [0, 1, 4, 5] and fd is either 3 or 4, we can skip over 0 and 1
        // -- those cases have been covered already.
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
#[inline]
pub fn is_wsl_1() -> bool {
    use core::sync::atomic::{AtomicU8, Ordering};

    // 0=Not running on WSL 1
    // 1=Running on WSL 1
    // >1=Uninitialized
    static IS_WSL1: AtomicU8 = AtomicU8::new(2);

    match IS_WSL1.load(Ordering::Relaxed) {
        // Already initialized; return the result
        1 => true,
        0 => false,

        _ => {
            let mut uname = unsafe { core::mem::zeroed() };
            unsafe {
                libc::uname(&mut uname);
            }

            let uname_release_len = uname
                .release
                .iter()
                .position(|c| *c == 0)
                .unwrap_or_else(|| uname.release.len());

            // uname.release is an array of `libc::c_char`s. `libc::c_char` may be either a u8 or
            // an i8, so unfortunately we have to use unsafe operations to get a reference as a
            // &[u8].
            let uname_release = unsafe {
                core::slice::from_raw_parts(uname.release.as_ptr() as *const u8, uname_release_len)
            };

            // It seems that on WSL 1 the kernel "release name" ends with "-Microsoft", and on WSL
            // 2 the release name ends with "-microsoft-standard". So we look for "Microsoft" at
            // the end to mean WSL 1.
            let is_wsl1 = uname_release.ends_with(b"Microsoft");

            // Store the result
            IS_WSL1.store(is_wsl1 as u8, Ordering::Relaxed);

            is_wsl1
        }
    }
}

#[inline]
pub fn is_fd_valid(fd: libc::c_int) -> bool {
    unsafe { libc::fcntl(fd, libc::F_GETFD) >= 0 }
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
pub fn apply_range<F: FnMut(libc::c_int, libc::c_int) -> Result<(), ()>>(
    minfd: libc::c_int,
    mut keep_fds: &[libc::c_int],
    mut func: F,
) -> Result<(), ()> {
    // Skip over any elements of keep_fds that are less than minfd
    if let Some(index) = keep_fds.iter().position(|&fd| fd >= minfd) {
        keep_fds = &keep_fds[index..];
    } else {
        // keep_fds is empty (or would be when all elements < minfd are removed)
        return func(minfd, libc::c_int::MAX);
    }

    if keep_fds[0] > minfd {
        func(minfd, keep_fds[0] - 1)?;
    }

    for i in 0..(keep_fds.len() - 1) {
        // Safety: i will only ever be in the range [0, keep_fds.len() - 2].
        // So i and i + 1 will always be valid indices in keep_fds.
        let low = unsafe { *keep_fds.get_unchecked(i) };
        let high = unsafe { *keep_fds.get_unchecked(i + 1) };

        debug_assert!(high >= low);

        if high - low >= 2 {
            func(low + 1, high - 1)?;
        }
    }

    func(keep_fds[keep_fds.len() - 1] + 1, libc::c_int::MAX)
}

pub fn set_cloexec(fd: libc::c_int) {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };

    if flags >= 0 && (flags & libc::FD_CLOEXEC) != libc::FD_CLOEXEC {
        // fcntl(F_GETFD) succeeded, and it did *not* return the FD_CLOEXEC flag
        unsafe {
            libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_fd<F: FnOnce(libc::c_int)>(f: F) {
        let fd = unsafe { libc::open(b"/\0".as_ptr() as *const _, libc::O_RDONLY) };
        assert!(fd >= 0);

        f(fd);

        unsafe {
            libc::close(fd);
        }
    }

    #[test]
    fn test_inspect_keep_fds() {
        assert_eq!(inspect_keep_fds(&[]), (libc::c_int::MIN, true));

        assert_eq!(inspect_keep_fds(&[0]), (0, true));
        assert_eq!(inspect_keep_fds(&[0, 1]), (1, true));
        assert_eq!(inspect_keep_fds(&[1, 0]), (1, false));

        assert_eq!(inspect_keep_fds(&[0, 1, 2, 5, 7]), (7, true));
        assert_eq!(inspect_keep_fds(&[0, 1, 2, 7, 5]), (7, false));

        assert_eq!(inspect_keep_fds(&[-1]), (-1, true));
        assert_eq!(
            inspect_keep_fds(&[libc::c_int::MIN]),
            (libc::c_int::MIN, true)
        );
    }

    #[test]
    fn test_check_should_keep_sorted() {
        let mut keep_fds: &[libc::c_int] = &[0, 1, 5, 8, 10];

        assert!(check_should_keep(&mut keep_fds, 0, true));
        assert_eq!(keep_fds, &[0, 1, 5, 8, 10]);

        assert!(check_should_keep(&mut keep_fds, 1, true));
        assert_eq!(keep_fds, &[1, 5, 8, 10]);

        assert!(!check_should_keep(&mut keep_fds, 2, true));
        assert_eq!(keep_fds, &[5, 8, 10]);
        assert!(!check_should_keep(&mut keep_fds, 2, true));
        assert_eq!(keep_fds, &[5, 8, 10]);

        assert!(!check_should_keep(&mut keep_fds, 3, true));
        assert_eq!(keep_fds, &[5, 8, 10]);

        assert!(check_should_keep(&mut keep_fds, 5, true));
        assert_eq!(keep_fds, &[5, 8, 10]);

        assert!(!check_should_keep(&mut keep_fds, 6, true));
        assert_eq!(keep_fds, &[8, 10]);
    }

    #[test]
    fn test_check_should_keep_not_sorted() {
        let mut keep_fds: &[libc::c_int] = &[0, 1, 5, 8, 10];

        assert!(check_should_keep(&mut keep_fds, 0, false));
        assert_eq!(keep_fds, &[0, 1, 5, 8, 10]);

        assert!(check_should_keep(&mut keep_fds, 1, false));
        assert_eq!(keep_fds, &[0, 1, 5, 8, 10]);

        assert!(!check_should_keep(&mut keep_fds, 2, false));
        assert_eq!(keep_fds, &[0, 1, 5, 8, 10]);

        assert!(!check_should_keep(&mut keep_fds, 3, false));
        assert_eq!(keep_fds, &[0, 1, 5, 8, 10]);

        assert!(check_should_keep(&mut keep_fds, 5, false));
        assert_eq!(keep_fds, &[0, 1, 5, 8, 10]);

        assert!(!check_should_keep(&mut keep_fds, 6, false));
        assert_eq!(keep_fds, &[0, 1, 5, 8, 10]);
    }

    #[test]
    fn test_simplify_keep_fds() {
        let mut keep_fds: &[libc::c_int];
        let mut minfd;

        // Here, the entire list can be emptied without changing minfd
        keep_fds = &[0, 1, 2];
        minfd = 3;
        assert_eq!(simplify_keep_fds(keep_fds, true, &mut minfd), &[]);
        assert_eq!(minfd, 3);

        // Here, it can be emptied if minfd is increased
        keep_fds = &[2, 3, 4, 5, 6, 7];
        minfd = 3;
        assert_eq!(simplify_keep_fds(keep_fds, true, &mut minfd), &[]);
        assert_eq!(minfd, 8);

        // Here, the first 3 elements can be removed if minfd is increased
        keep_fds = &[3, 4, 5, 8, 10];
        minfd = 3;
        assert_eq!(simplify_keep_fds(keep_fds, true, &mut minfd), &[8, 10]);
        assert_eq!(minfd, 6);

        // Here it's just the first element
        keep_fds = &[3, 5, 8, 10];
        minfd = 3;
        assert_eq!(simplify_keep_fds(keep_fds, true, &mut minfd), &[5, 8, 10]);
        assert_eq!(minfd, 4);

        // Can't simplify any further
        keep_fds = &[4, 5, 8, 10];
        minfd = 3;
        assert_eq!(
            simplify_keep_fds(keep_fds, true, &mut minfd),
            &[4, 5, 8, 10]
        );
        assert_eq!(minfd, 3);

        // And if fds_sorted=false, no simplification can be performed

        keep_fds = &[2, 1, 0];
        minfd = 3;
        assert_eq!(simplify_keep_fds(keep_fds, false, &mut minfd), &[2, 1, 0]);
        assert_eq!(minfd, 3);

        keep_fds = &[4, 5, 8, 10, 3];
        minfd = 3;
        assert_eq!(
            simplify_keep_fds(keep_fds, false, &mut minfd),
            &[4, 5, 8, 10, 3]
        );
        assert_eq!(minfd, 3);
    }

    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    #[test]
    fn test_apply_range() {
        macro_rules! check_ok {
            ($minfd:expr, [$($keep_fds:expr),* $(,)?], [$($calls:expr),* $(,)?] $(,)?) => {{
                let mut ranges = [(0, 0); 100];
                let mut len = 0;

                apply_range($minfd, &[$($keep_fds),*], |low, high| {
                    *ranges.get_mut(len).unwrap() = (low, high);
                    len += 1;
                    Ok(())
                }).unwrap();

                assert_eq!(&ranges[..len], [$($calls),*]);
            }}
        }

        check_ok!(0, [], [(0, libc::c_int::MAX)]);
        check_ok!(-100, [], [(-100, libc::c_int::MAX)]);

        check_ok!(3, [0, 2, 3, 4, 5, 6], [(7, libc::c_int::MAX)]);
        check_ok!(3, [0, 2], [(3, libc::c_int::MAX)]);

        check_ok!(3, [3, 4, 5, 6], [(7, libc::c_int::MAX)]);
        check_ok!(3, [4, 5, 6], [(3, 3), (7, libc::c_int::MAX)]);
        check_ok!(3, [5, 6, 9, 10], [(3, 4), (7, 8), (11, libc::c_int::MAX)]);
        check_ok!(
            3,
            [5, 6, 9, 10, 20, 23],
            [(3, 4), (7, 8), (11, 19), (21, 22), (24, libc::c_int::MAX)],
        );

        macro_rules! check_err {
            ($minfd:expr, [$($keep_fds:expr),* $(,)?], $call:expr $(,)?) => {{
                let mut call = None;

                apply_range($minfd, &[$($keep_fds),*], |low, high| {
                    assert!(call.is_none());
                    call = Some((low, high));
                    Err(())
                }).unwrap_err();

                assert_eq!(call.unwrap(), $call);
            }}
        }

        check_err!(0, [], (0, libc::c_int::MAX));
        check_err!(-100, [], (-100, libc::c_int::MAX));

        check_err!(3, [0, 2, 3, 4, 5, 6], (7, libc::c_int::MAX));
        check_err!(3, [0, 2], (3, libc::c_int::MAX));

        check_err!(3, [3, 4, 5, 6], (7, libc::c_int::MAX));
        check_err!(3, [4, 5, 6], (3, 3));
        check_err!(3, [5, 6, 9, 10], (3, 4));
        check_err!(3, [5, 6, 9, 10, 20, 23], (3, 4),);
    }

    #[test]
    fn test_is_fd_valid() {
        assert!(!is_fd_valid(-1));
        assert!(!is_fd_valid(libc::c_int::MAX));

        assert!(is_fd_valid(0));
        with_fd(|fd| {
            assert!(is_fd_valid(fd));
        });
    }

    #[test]
    fn test_set_cloexec() {
        // No panic on errors like this
        set_cloexec(-1);
        set_cloexec(libc::c_int::MAX);

        fn is_cloexec(fd: libc::c_int) -> bool {
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
            assert!(flags >= 0);
            flags & libc::FD_CLOEXEC == libc::FD_CLOEXEC
        }

        with_fd(|fd| {
            assert!(!is_cloexec(fd));
            set_cloexec(fd);
            assert!(is_cloexec(fd));
        });
    }
}
