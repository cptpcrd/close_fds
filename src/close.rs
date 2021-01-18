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
/// # Efficiency
///
/// ## Efficiency of using `keep_fds`
///
/// **TL;DR**: If you're going to be passing more than a few file descriptors in `keep_fds`, sort
/// the slice first for best performance.
///
/// On some systems, the `keep_fds` list may see massive numbers of lookups, especially if it
/// contains high-numbered file descriptors.
///
/// If `keep_fds` is sorted, since `iter_open_fds()` goes in ascending order it is easy to check
/// for the presence of a given file descriptor in `keep_fds`. However, because `close_fds` is a
/// `#![no_std]` crate, it can't allocate memory for a *copy* of `keep_fds` that it can sort.
///
/// As a result, this function first checks if `keep_fds` is sorted. If it is, the more efficient
/// method can be employed. If not, it falls back on `.contains()`. which can be very slow.
pub unsafe fn close_open_fds(mut minfd: libc::c_int, mut keep_fds: &[libc::c_int]) {
    if minfd < 0 {
        minfd = 0;
    }

    let (max_keep_fd, fds_sorted) = crate::util::inspect_keep_fds(keep_fds);

    if fds_sorted {
        // Example: specifying keep_fds=[3, 4, 6, 7]; minfd=3 has the same result as specifying
        // keep_fds=[6, 7]; minfd=5.
        // In some cases, this translation may reduce the number of syscalls and/or eliminate the
        // need to call iter_fds() in the first place.

        while let Some(first_fd) = keep_fds.first() {
            match first_fd.cmp(&minfd) {
                // keep_fds[0] > minfd
                // No further simplification can be done
                core::cmp::Ordering::Greater => break,

                // keep_fds[0] == minfd
                // We can remove keep_fds[0] and increment minfd
                core::cmp::Ordering::Equal => {
                    keep_fds = &keep_fds[1..];
                    minfd += 1;
                }

                // keep_fds[0] < minfd
                // We can remove keep_fds[0]
                core::cmp::Ordering::Less => keep_fds = &keep_fds[1..],
            }
        }
    }

    #[cfg(any(
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd",
        target_os = "dragonfly"
    ))]
    if max_keep_fd < minfd {
        // On the BSDs, if all the file descriptors in keep_fds are less than minfd (or if keep_fds
        // is empty), we can just call closefrom()
        crate::externs::closefrom(minfd);
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
                    crate::externs::closefrom(fd);
                    return;
                } else {
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
