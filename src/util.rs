pub fn inspect_keep_fds(keep_fds: &[libc::c_int]) -> (libc::c_int, bool) {
    // Get the maximum file descriptor from the list, and also check if it's sorted.

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

        while let Some(first_fd) = keep_fds.first() {
            match first_fd.cmp(&minfd) {
                // keep_fds[0] > minfd
                // No further simplification can be done
                Ordering::Greater => break,

                // keep_fds[0] == minfd
                // We can remove keep_fds[0] and increment minfd
                Ordering::Equal => {
                    keep_fds = &keep_fds[1..];
                    *minfd += 1;
                }

                // keep_fds[0] < minfd
                // We can remove keep_fds[0]
                Ordering::Less => keep_fds = &keep_fds[1..],
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
    // It seems that on WSL 1 the kernel "release name" ends with "-Microsoft", and on WSL 2 the
    // release name ends with "-microsoft-standard". So we look for "Microsoft" at the end.

    let mut uname = unsafe { core::mem::zeroed() };

    unsafe {
        libc::uname(&mut uname);
    }

    let uname_release_len = uname
        .release
        .iter()
        .position(|c| *c == 0)
        .unwrap_or_else(|| uname.release.len());

    // uname.release is an array of `libc::c_char`s. `libc::c_char` may be either a u8 or an i8, so
    // unfortunately we have to use unsafe operations to get a reference as a &[u8].
    let uname_release = unsafe {
        core::slice::from_raw_parts(uname.release.as_ptr() as *const u8, uname_release_len)
    };

    uname_release.ends_with(b"Microsoft")
}

#[inline]
pub fn is_fd_valid(fd: libc::c_int) -> bool {
    unsafe { libc::fcntl(fd, libc::F_GETFD) >= 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inspect_keep_fds() {
        assert_eq!(inspect_keep_fds(&[]), (-1, true));

        assert_eq!(inspect_keep_fds(&[0]), (0, true));
        assert_eq!(inspect_keep_fds(&[0, 1]), (1, true));
        assert_eq!(inspect_keep_fds(&[1, 0]), (1, false));

        assert_eq!(inspect_keep_fds(&[0, 1, 2, 5, 7]), (7, true));
        assert_eq!(inspect_keep_fds(&[0, 1, 2, 7, 5]), (7, false));
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
}
