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

    let (max_keep_fd, fds_sorted) = crate::util::inspect_keep_fds(keep_fds);

    #[cfg(any(
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd",
        target_os = "dragonfly"
    ))]
    if max_keep_fd < minfd {
        // On the BSDs, if all the file descriptors in keep_fds are less than
        // minfd (or if keep_fds is empty), we can just call closefrom()
        crate::externs::closefrom(minfd);
        return;
    }

    let mut fditer = crate::iter_possible_fds(minfd);

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
                crate::externs::closefrom(fd);
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
        } else if !crate::util::check_should_keep(&mut keep_fds, fd, fds_sorted) {
            // Close it if it's not in keep_fds
            libc::close(fd);
        }
    }
}
