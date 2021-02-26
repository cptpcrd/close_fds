use crate::{fditer, sys, util};

pub fn set_fds_cloexec_generic(
    mut minfd: libc::c_int,
    mut keep_fds: &[libc::c_int],
    thread_safe: bool,
) {
    minfd = core::cmp::max(minfd, 0);

    let (max_keep_fd, fds_sorted) = util::inspect_keep_fds(keep_fds);

    keep_fds = util::simplify_keep_fds(keep_fds, fds_sorted, &mut minfd);

    #[cfg(target_os = "linux")]
    {
        use core::sync::atomic::{AtomicBool, Ordering};
        static mut MAY_HAVE_CLOSE_RANGE_CLOEXEC: AtomicBool = AtomicBool::new(true);

        #[inline]
        unsafe fn set_cloexec_range(minfd: libc::c_uint, maxfd: libc::c_uint) -> Result<(), ()> {
            debug_assert!(minfd <= maxfd, "{} > {}", minfd, maxfd);

            if libc::syscall(
                sys::SYS_CLOSE_RANGE,
                minfd as libc::c_uint,
                maxfd as libc::c_uint,
                sys::CLOSE_RANGE_CLOEXEC,
            ) == 0
            {
                Ok(())
            } else {
                MAY_HAVE_CLOSE_RANGE_CLOEXEC.store(false, Ordering::Relaxed);
                Err(())
            }
        }

        #[inline]
        unsafe fn set_cloexec_shortcut(
            minfd: libc::c_int,
            keep_fds: &[libc::c_int],
            max_keep_fd: libc::c_int,
            fds_sorted: bool,
        ) -> Result<(), ()> {
            if max_keep_fd < minfd {
                set_cloexec_range(minfd as libc::c_uint, libc::c_uint::MAX)
            } else if fds_sorted {
                util::apply_range(minfd, keep_fds, |low, high| {
                    set_cloexec_range(low as libc::c_uint, high as libc::c_uint)
                })
            } else {
                Err(())
            }
        }

        if unsafe { MAY_HAVE_CLOSE_RANGE_CLOEXEC.load(Ordering::Relaxed) }
            && unsafe { set_cloexec_shortcut(minfd, keep_fds, max_keep_fd, fds_sorted) }.is_ok()
        {
            return;
        }
    }

    for fd in fditer::iter_fds(minfd, true, thread_safe) {
        if fd > max_keep_fd || !util::check_should_keep(&mut keep_fds, fd, fds_sorted) {
            // It's not in keep_fds

            let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };

            if flags >= 0 && (flags & libc::FD_CLOEXEC) != libc::FD_CLOEXEC {
                // fcntl(F_GETFD) succeeded, and it did *not* return the FD_CLOEXEC flag
                unsafe {
                    libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC);
                }
            }
        }
    }
}
