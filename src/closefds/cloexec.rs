use crate::util;

#[cfg(target_os = "linux")]
use core::sync::atomic::{AtomicBool, Ordering};

#[cfg(target_os = "linux")]
static MAY_HAVE_CLOSE_RANGE_CLOEXEC: AtomicBool = AtomicBool::new(true);

#[cfg(target_os = "linux")]
#[inline]
fn set_cloexec_range(minfd: libc::c_uint, maxfd: libc::c_uint) -> Result<(), ()> {
    debug_assert!(minfd <= maxfd, "{} > {}", minfd, maxfd);

    if unsafe {
        libc::syscall(
            libc::SYS_close_range,
            minfd as libc::c_uint,
            maxfd as libc::c_uint,
            crate::sys::CLOSE_RANGE_CLOEXEC,
        )
    } == 0
    {
        Ok(())
    } else {
        MAY_HAVE_CLOSE_RANGE_CLOEXEC.store(false, Ordering::Relaxed);
        Err(())
    }
}

#[cfg(target_os = "linux")]
#[inline]
fn set_cloexec_shortcut(
    minfd: libc::c_int,
    keep_fds: &[libc::c_int],
    max_keep_fd: libc::c_int,
    fds_sorted: bool,
) -> Result<(), ()> {
    if !MAY_HAVE_CLOSE_RANGE_CLOEXEC.load(Ordering::Relaxed) {
        Err(())
    } else if max_keep_fd < minfd {
        set_cloexec_range(minfd as libc::c_uint, libc::c_uint::MAX)
    } else if fds_sorted {
        util::apply_range(minfd, keep_fds, |low, high| {
            set_cloexec_range(low as libc::c_uint, high as libc::c_uint)
        })
    } else {
        Err(())
    }
}

pub(crate) fn set_fds_cloexec(
    mut minfd: libc::c_int,
    keep_fds: super::KeepFds,
    mut itbuilder: crate::FdIterBuilder,
) {
    let super::KeepFds {
        max: max_keep_fd,
        fds: mut keep_fds,
        sorted: fds_sorted,
    } = keep_fds;

    keep_fds = util::simplify_keep_fds(keep_fds, fds_sorted, &mut minfd);

    #[cfg(target_os = "linux")]
    if set_cloexec_shortcut(minfd, keep_fds, max_keep_fd, fds_sorted).is_ok() {
        return;
    }

    itbuilder.possible(true);

    let mut fditer = itbuilder.iter_from(minfd);

    while let Some(fd) = fditer.next() {
        if fd > max_keep_fd {
            // We know that none of the file descriptors we encounter from here onward can be in
            // keep_fds.
            set_cloexec_rest(fd, fditer);
            return;
        } else if !util::check_should_keep(&mut keep_fds, fd, fds_sorted) {
            // It's not in keep_fds
            util::set_cloexec(fd);
        }
    }
}

fn set_cloexec_rest(fd: libc::c_int, fditer: crate::FdIter) {
    // On Linux, we may be able to use close_range() with the CLOSE_RANGE_CLOEXEC flag to set them
    // as close-on-exec directly
    #[cfg(target_os = "linux")]
    if MAY_HAVE_CLOSE_RANGE_CLOEXEC.load(Ordering::Relaxed)
        && set_cloexec_range(fd as libc::c_uint, libc::c_uint::MAX).is_ok()
    {
        return;
    }

    // Fall back on looping through and closing manually
    util::set_cloexec(fd);
    for fd in fditer {
        util::set_cloexec(fd);
    }
}

#[inline]
pub(crate) fn probe() {
    // If we specify an invalid range (like in close.rs), we won't know whether EINVAL means
    // "invalid flags" or "invalid file descriptor range". So we have to make a call like this
    // (which *should* do nothing; it shouldn't be possible to open and use file descriptors in
    // the vicinity of 2^32).
    #[cfg(target_os = "linux")]
    let _ = set_cloexec_range(libc::c_uint::MAX, libc::c_uint::MAX);
}
