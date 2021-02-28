use std::fs;
use std::os::unix::prelude::*;
use std::process::Command;

fn unset_cloexec(fd: RawFd) {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    assert!(flags >= 0);

    if flags & libc::FD_CLOEXEC != 0 {
        assert_eq!(
            unsafe { libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) },
            0
        );
    }
}

fn main() {
    // Open three files (really, directories), and unset the close-on-exec flag on all three
    let f1 = fs::File::open("/").unwrap();
    let f2 = fs::File::open("/").unwrap();
    let f3 = fs::File::open("/").unwrap();
    unset_cloexec(f1.as_raw_fd());
    unset_cloexec(f2.as_raw_fd());
    unset_cloexec(f3.as_raw_fd());

    let mut keep_fds = [f1.as_raw_fd(), f3.as_raw_fd()];
    // ALWAYS sort the list of file descriptors if possible!
    keep_fds.sort_unstable();

    let mut cmd = Command::new("true");

    unsafe {
        cmd.pre_exec(move || {
            // On macOS/iOS, just set them as close-on-exec
            // Some sources indicate libdispatch may crash if the file descriptors are *actually*
            // closed

            #[cfg(any(target_os = "macos", target_os = "ios"))]
            close_fds::set_fds_cloexec(3, &keep_fds);
            #[cfg(not(any(target_os = "macos", target_os = "ios")))]
            close_fds::close_open_fds(3, &keep_fds);

            Ok(())
        });
    }

    cmd.status().unwrap();
}
