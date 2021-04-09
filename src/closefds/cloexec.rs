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
            crate::sys::SYS_CLOSE_RANGE,
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

    for fd in itbuilder.iter_from(minfd) {
        if fd > max_keep_fd {
            // We know that none of the file descriptors we encounter from here onward can be in
            // keep_fds.

            #[cfg(target_os = "linux")]
            if MAY_HAVE_CLOSE_RANGE_CLOEXEC.load(Ordering::Relaxed)
                && set_cloexec_range(fd as libc::c_uint, libc::c_uint::MAX).is_ok()
            {
                return;
            }

            // Otherwise, this just lets us skip calling check_should_keep()
            util::set_cloexec(fd);
        } else if !util::check_should_keep(&mut keep_fds, fd, fds_sorted) {
            // It's not in keep_fds
            util::set_cloexec(fd);
        }
    }
}
