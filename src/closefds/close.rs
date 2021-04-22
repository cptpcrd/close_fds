#[cfg(target_os = "linux")]
use core::sync::atomic::{AtomicBool, Ordering};
#[cfg(target_os = "freebsd")]
use core::sync::atomic::{AtomicU8, Ordering};

pub(crate) unsafe fn close_fds(
    mut minfd: libc::c_int,
    keep_fds: super::KeepFds,
    mut itbuilder: crate::FdIterBuilder,
) {
    let super::KeepFds {
        max: max_keep_fd,
        fds: mut keep_fds,
        sorted: fds_sorted,
    } = keep_fds;

    keep_fds = crate::util::simplify_keep_fds(keep_fds, fds_sorted, &mut minfd);

    // Some OSes have (or may have) a closefrom() or close_range() syscall that we can use to
    // improve performance if certain conditions are true.
    if close_fds_shortcut(minfd, keep_fds, max_keep_fd, fds_sorted).is_ok() {
        return;
    }

    itbuilder.possible(true);

    // On systems with closefrom(), skip the "nfds" method when determining maxfd -- these systems
    // have a working closefrom(), so we can just call that once we pass the end of keep_fds.
    #[cfg(any(
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd",
        target_os = "dragonfly",
    ))]
    itbuilder.threadsafe(true);

    let mut fditer = itbuilder.iter_from(minfd);

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
                    if MAY_HAVE_CLOSE_RANGE.load(Ordering::Relaxed)
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

                    // We also know that none of the remaining file descriptors are in keep_fds, so
                    // we can just iterate through and close all of them directly
                    for fd in fditer {
                        debug_assert!(fd > max_keep_fd);
                        libc::close(fd);
                    }
                    return;
                }
            }
        } else if !crate::util::check_should_keep(&mut keep_fds, fd, fds_sorted) {
            // Close it if it's not in keep_fds
            libc::close(fd);
        }
    }
}

#[cfg(target_os = "linux")]
static MAY_HAVE_CLOSE_RANGE: AtomicBool = AtomicBool::new(true);

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
        MAY_HAVE_CLOSE_RANGE.store(false, Ordering::Relaxed);
        Err(())
    }
}

#[cfg(target_os = "freebsd")]
fn check_has_close_range() -> Result<(), ()> {
    // On FreeBSD, trying to make a syscall that the kernel doesn't recognize will result in the
    // process being killed with SIGSYS. So before we try making a syscall(), we have to check if
    // the kernel is new enough. (We also have to cache the presence/absence differently because of
    // this).

    // 0=present, 1=absent, other values=uninitialized
    static HAS_CLOSE_RANGE: AtomicU8 = AtomicU8::new(2);

    match HAS_CLOSE_RANGE.load(Ordering::Relaxed) {
        // We know it's present
        1 => Ok(()),
        // We know it *isn't* present
        0 => Err(()),

        // Check if it's present
        // Here, we check the `kern.osreldate` sysctl
        _ => {
            const OSRELDATE_MIB: [libc::c_int; 2] = [libc::CTL_KERN, libc::KERN_OSRELDATE];

            let mut osreldate = 0;
            let mut oldlen = core::mem::size_of::<libc::c_int>();

            if unsafe {
                libc::sysctl(
                    OSRELDATE_MIB.as_ptr(),
                    OSRELDATE_MIB.len() as _,
                    &mut osreldate as *mut _ as *mut _,
                    &mut oldlen,
                    core::ptr::null(),
                    0,
                )
            } != 0
                || osreldate < 1202000
            {
                // Either:
                // - sysctl() failed somehow (???); assume close_range() is not present
                // - The kernel is too old and it doesn't support close_range()
                HAS_CLOSE_RANGE.store(0, Ordering::Relaxed);
                Err(())
            } else {
                HAS_CLOSE_RANGE.store(1, Ordering::Relaxed);
                Ok(())
            }
        }
    }
}

#[cfg(target_os = "freebsd")]
unsafe fn try_close_range(minfd: libc::c_uint, maxfd: libc::c_uint) -> Result<(), ()> {
    debug_assert!(minfd <= maxfd, "{} > {}", minfd, maxfd);

    // This should have been checked previously
    debug_assert!(check_has_close_range().is_ok());

    if libc::syscall(
        crate::sys::SYS_CLOSE_RANGE,
        minfd as libc::c_uint,
        maxfd as libc::c_uint,
        0,
    ) == 0
    {
        Ok(())
    } else {
        Err(())
    }
}

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
    if !MAY_HAVE_CLOSE_RANGE.load(Ordering::Relaxed) {
        // If we know that close_range() definitely isn't available, there's nothing we can do.
        return Err(());
    } else if max_keep_fd < minfd {
        // Same case as closefrom() on the BSDs
        return try_close_range(minfd as libc::c_uint, libc::c_uint::MAX);
    }

    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    if fds_sorted {
        // If the list of file descriptors is sorted, we can use close_range() to close the "gaps"
        // between file descriptors.

        debug_assert!(!keep_fds.is_empty());

        #[cfg(target_os = "freebsd")]
        check_has_close_range()?;

        return crate::util::apply_range(minfd, keep_fds, |low, high| {
            try_close_range(low as libc::c_uint, high as libc::c_uint)
        });
    }

    // We can't do any optimizations without calling iter_possible_fds()
    Err(())
}
