/// Close all open file descriptors starting at `minfd`, except for the file descriptors in
/// `keep_fds`.
///
/// # Safety
///
/// This function is NOT safe to use if other threads are interacting with files, networking, or
/// anything else that could possibly involve file descriptors in any way, shape, or form. (Note: On
/// some systems, file descriptor use may be more common than you think! For example, on Linux with
/// musl libc, `std::fs::canonicalize()` will open a file descriptor to the given path.)
///
/// In addition, some objects, such as `std::fs::File`, may open file descriptors and then assume
/// that they will remain open. This function, by closing those file descriptors, violates those
/// assumptions.
///
/// This function is safe to use if it can be verified that these are not concerns. For example, it
/// *should* be safe at startup or just before an `exec()`. At all other times, exercise extreme
/// caution when using this function, as it may lead to race conditions and/or security issues.
///
/// (Note: The above warnings, by definition, make it unsafe to call this function concurrently from
/// multiple threads. As a result, this function may perform other non-thread-safe operations.)
///
/// # Efficiency
///
/// ## Efficiency of using `keep_fds`
///
/// If you're going to be passing more than a few file descriptors in `keep_fds`, or if the file
/// descriptors that you pass are high-numbered (i.e. 50 or 100), sort the slice first. This will
/// give you significant performance improvements (especially on Linux 5.9+).
///
/// `close_fds` can't just copy the slice and sort it for you because it's a `#![no_std]` crate, so
/// it can't allocate memory.
pub unsafe fn close_open_fds(mut minfd: libc::c_int, mut keep_fds: &[libc::c_int]) {
    if minfd < 0 {
        minfd = 0;
    }

    let (max_keep_fd, fds_sorted) = crate::util::inspect_keep_fds(keep_fds);

    keep_fds = crate::util::simplify_keep_fds(keep_fds, fds_sorted, &mut minfd);

    // These OSes have (or may have) a closefrom() or close_range() syscall that we can use to
    // improve performance if certain conditions are true.
    #[cfg(any(
        target_os = "linux",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd",
        target_os = "dragonfly"
    ))]
    if close_fds_shortcut(minfd, keep_fds, max_keep_fd, fds_sorted).is_ok() {
        return;
    }

    let mut fditer = crate::fditer::iter_fds(
        minfd,
        // Include "possible" file descriptors
        true,
        // On these systems, tell iter_fds() to prefer speed over accuracy when determining maxfd
        // (if it has to use a maxfd loop) -- these systems have a working closefrom(), so we can
        // just call that once we pass the end of keep_fds.
        cfg!(any(
            target_os = "freebsd",
            target_os = "netbsd",
            target_os = "openbsd",
            target_os = "dragonfly"
        )),
    );

    // We have to use a while loop so we can drop() the iterator in the closefrom() case
    #[allow(clippy::while_let_on_iterator)]
    while let Some(fd) = fditer.next() {
        #[allow(clippy::if_same_then_else)]
        if fd > max_keep_fd {
            // If fd > max_keep_fd, we know that none of the file descriptors we encounter from
            // here onward can be in keep_fds.

            cfg_if::cfg_if! {
                if #[cfg(any(
                    target_os = "freebsd",
                    target_os = "netbsd",
                    target_os = "openbsd",
                    target_os = "dragonfly",
                ))] {
                    // On the BSDs we can use closefrom() to close the rest

                    // Close the directory file descriptor (if one is being used) first
                    drop(fditer);
                    crate::sys::closefrom(fd);
                    return;
                } else {
                    // On Linux we can do the same thing with close_range() if it's available
                    #[cfg(target_os = "linux")]
                    if MAY_HAVE_CLOSE_RANGE
                        && try_close_range(fd as libc::c_uint, libc::c_uint::MAX).is_ok()
                    {
                        // We can't close the directory file descriptor *first*, because
                        // close_range() might not be available. So there's a slight race condition
                        // here where the call to close() might accidentally close another file
                        // descriptor.
                        // Then again, this function is documented as being unsafe if other threads
                        // are interacting with file descriptors.

                        drop(fditer);
                        return;
                    }

                    // On other systems, this just allows us to skip the contains() check
                    libc::close(fd);
                }
            }
        } else if !crate::util::check_should_keep(&mut keep_fds, fd, fds_sorted) {
            // Close it if it's not in keep_fds
            libc::close(fd);
        }
    }
}

#[cfg(target_os = "linux")]
static mut MAY_HAVE_CLOSE_RANGE: bool = true;

#[cfg(target_os = "linux")]
unsafe fn try_close_range(minfd: libc::c_uint, maxfd: libc::c_uint) -> Result<(), ()> {
    // Sanity check
    // This shouldn't happen -- code that calls this function is usually careful to validate the
    // arguments -- but we want to make sure it doesn't happen because it could cause close_range()
    // to fail and make the code incorrectly assume that it isn't available.
    debug_assert!(minfd <= maxfd, "{} > {}", minfd, maxfd);

    #[allow(clippy::unnecessary_cast)]
    if libc::syscall(
        crate::sys::SYS_CLOSE_RANGE,
        minfd as libc::c_uint,
        maxfd as libc::c_uint,
        0 as libc::c_uint,
    ) == 0
    {
        Ok(())
    } else {
        MAY_HAVE_CLOSE_RANGE = false;
        Err(())
    }
}

#[cfg(any(
    target_os = "linux",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "dragonfly"
))]
#[allow(unused_variables)]
#[inline]
unsafe fn close_fds_shortcut(
    minfd: libc::c_int,
    keep_fds: &[libc::c_int],
    max_keep_fd: libc::c_int,
    fds_sorted: bool,
) -> Result<(), ()> {
    #[cfg(any(
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd",
        target_os = "dragonfly"
    ))]
    if max_keep_fd < minfd {
        // On the BSDs, if all the file descriptors in keep_fds are less than
        // minfd (or if keep_fds is empty), we can just call closefrom()

        crate::sys::closefrom(minfd);
        return Ok(());
    }

    #[cfg(target_os = "linux")]
    if !MAY_HAVE_CLOSE_RANGE {
        // If we know that close_range() definitely isn't available, there's nothing we can do.
        return Err(());
    } else if max_keep_fd < minfd {
        // Same case as closefrom() on the BSDs
        return try_close_range(minfd as libc::c_uint, libc::c_uint::MAX);
    } else if fds_sorted {
        // If the list of file descriptors is sorted, we can use close_range() to close the "gaps"
        // between file descriptors.

        // Skip over any elements of keep_fds that are less than minfd
        // We know there are at least *some* elements that are >= minfd, or the `max_keep_fd <
        // minfd` case would have been triggered.
        let index = keep_fds.iter().position(|&fd| fd >= minfd).unwrap();
        let keep_fds = &keep_fds[index..];

        debug_assert!(!keep_fds.is_empty());

        // First, close the range [minfd, keep_fds[0])
        if keep_fds[0] > minfd {
            try_close_range(minfd as libc::c_uint, keep_fds[0] as libc::c_uint - 1)?;
        }

        // Go through and close the ranges *between* the file descriptors we were told to keep open
        for i in 0..(keep_fds.len() - 1) {
            // Safety: i will only ever be in the range [0, keep_fds.len() - 2].
            // So i and i + 1 will always be valid indices in keep_fds.
            let low = *keep_fds.get_unchecked(i);
            let high = *keep_fds.get_unchecked(i + 1);

            // If they aren't adjacent, then try to close the range between them
            if high - low >= 2 {
                try_close_range(low as libc::c_uint + 1, high as libc::c_uint - 1)?;
            }
        }

        // Now close from the last file descriptor onward
        return try_close_range(
            keep_fds[keep_fds.len() - 1] as libc::c_uint + 1,
            libc::c_uint::MAX,
        );
    }

    // We can't do any optimizations without calling iter_possible_fds()
    Err(())
}
