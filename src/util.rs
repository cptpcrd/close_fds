pub fn inspect_keep_fds(keep_fds: &[libc::c_int]) -> (libc::c_int, bool) {
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

pub fn check_should_keep(keep_fds: &mut &[libc::c_int], fd: libc::c_int, fds_sorted: bool) -> bool {
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
pub fn is_wsl_1() -> bool {
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

#[cfg(unix)]
#[inline]
pub fn is_fd_valid(fd: libc::c_int) -> bool {
    unsafe { libc::fcntl(fd, libc::F_GETFD) >= 0 }
}

#[cfg(windows)]
#[inline]
pub fn is_fd_valid(fd: libc::c_int) -> bool {
    unsafe { libc::get_osfhandle(fd) >= 0 }
}
