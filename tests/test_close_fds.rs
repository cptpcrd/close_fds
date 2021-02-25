use std::os::unix::prelude::*;

fn is_fd_open(fd: libc::c_int) -> bool {
    unsafe { libc::fcntl(fd, libc::F_GETFD) >= 0 }
}

fn set_fd_cloexec(fd: libc::c_int, cloexec: bool) {
    let mut flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if flags < 0 {
        return;
    }

    // We could eliminate the second fcntl() in some cases, but this is just testing code.
    if cloexec {
        flags |= libc::FD_CLOEXEC;
    } else {
        flags &= !libc::FD_CLOEXEC;
    }

    unsafe {
        libc::fcntl(fd, libc::F_SETFD, flags);
    }
}

fn is_fd_cloexec(fd: libc::c_int) -> Option<bool> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };

    if flags < 0 {
        None
    } else if flags & libc::FD_CLOEXEC == libc::FD_CLOEXEC {
        Some(true)
    } else {
        Some(false)
    }
}

fn check_sorted<T: Ord>(items: &[T]) {
    let mut last_item = None;

    for item in items.iter() {
        if let Some(last_item) = last_item {
            assert!(last_item <= item);
        }

        last_item = Some(item);
    }
}

fn run_basic_test(callback: fn(fd1: libc::c_int, fd2: libc::c_int, fd3: libc::c_int)) {
    let f1 = std::fs::File::open("/").unwrap();
    let f2 = std::fs::File::open("/").unwrap();
    let f3 = std::fs::File::open("/").unwrap();

    let fd1 = f1.as_raw_fd();
    let fd2 = f2.as_raw_fd();
    let fd3 = f3.as_raw_fd();

    drop(f3);

    assert!(is_fd_open(fd1));
    assert!(is_fd_open(fd2));
    assert!(!is_fd_open(fd3));

    callback(fd1, fd2, fd3);
}

fn iter_open_fds_test(fd1: libc::c_int, fd2: libc::c_int, fd3: libc::c_int) {
    let mut fds: Vec<libc::c_int>;

    fds = close_fds::iter_open_fds(-1).collect();
    check_sorted(&fds);
    assert!(!fds.contains(&-1));
    assert!(fds.contains(&0));
    assert!(fds.contains(&fd1));
    assert!(fds.contains(&fd2));
    assert!(!fds.contains(&fd3));

    assert_eq!(close_fds::iter_open_fds(-1).min(), Some(0));
    assert_eq!(close_fds::iter_open_fds(-1).max().as_ref(), fds.last());
    assert_eq!(close_fds::iter_open_fds(-1).count(), fds.len());

    fds = close_fds::iter_open_fds_threadsafe(-1).collect();
    check_sorted(&fds);
    assert!(!fds.contains(&-1));
    assert!(fds.contains(&0));
    assert!(fds.contains(&fd1));
    assert!(fds.contains(&fd2));
    assert!(!fds.contains(&fd3));

    assert_eq!(close_fds::iter_open_fds_threadsafe(-1).min(), Some(0));
    assert_eq!(
        close_fds::iter_open_fds_threadsafe(-1).max().as_ref(),
        fds.last()
    );
    assert_eq!(close_fds::iter_open_fds_threadsafe(-1).count(), fds.len());

    fds = close_fds::iter_open_fds(0).collect();
    check_sorted(&fds);
    assert!(fds.contains(&0));
    assert!(fds.contains(&fd1));
    assert!(fds.contains(&fd2));
    assert!(!fds.contains(&fd3));

    // Test handling of minfd

    fds = close_fds::iter_open_fds(fd1).collect();
    check_sorted(&fds);
    assert!(!fds.contains(&0));
    assert!(fds.contains(&fd1));
    assert!(fds.contains(&fd2));
    assert!(!fds.contains(&fd3));

    fds = close_fds::iter_open_fds(fd2).collect();
    check_sorted(&fds);
    assert!(!fds.contains(&0));
    assert!(!fds.contains(&fd1));
    assert!(fds.contains(&fd2));
    assert!(!fds.contains(&fd3));

    fds = close_fds::iter_open_fds(fd3).collect();
    check_sorted(&fds);
    assert!(!fds.contains(&0));
    assert!(!fds.contains(&fd1));
    assert!(!fds.contains(&fd2));
    assert!(!fds.contains(&fd3));
}

fn iter_possible_fds_test(fd1: libc::c_int, fd2: libc::c_int, fd3: libc::c_int) {
    let mut fds: Vec<libc::c_int>;

    fds = close_fds::iter_possible_fds(-1).collect();
    check_sorted(&fds);
    assert!(!fds.contains(&-1));
    assert!(fds.contains(&fd1));
    assert!(fds.contains(&fd2));
    // Don't check for fd3 not being present because it might legitimately be
    // returned by iter_possible_fds()

    assert_eq!(close_fds::iter_possible_fds(-1).min(), Some(0));
    assert_eq!(close_fds::iter_possible_fds(-1).max().as_ref(), fds.last());
    assert_eq!(close_fds::iter_possible_fds(-1).count(), fds.len());

    fds = close_fds::iter_possible_fds_threadsafe(-1).collect();
    check_sorted(&fds);
    assert!(!fds.contains(&-1));
    assert!(fds.contains(&fd1));
    assert!(fds.contains(&fd2));

    assert_eq!(close_fds::iter_possible_fds_threadsafe(-1).min(), Some(0));
    assert_eq!(
        close_fds::iter_possible_fds_threadsafe(-1).max().as_ref(),
        fds.last()
    );
    assert_eq!(
        close_fds::iter_possible_fds_threadsafe(-1).count(),
        fds.len()
    );

    fds = close_fds::iter_possible_fds(0).collect();
    check_sorted(&fds);
    assert!(fds.contains(&fd1));
    assert!(fds.contains(&fd2));

    // Test handling of minfd

    fds = close_fds::iter_possible_fds(fd1).collect();
    check_sorted(&fds);
    assert!(fds.contains(&fd1));
    assert!(fds.contains(&fd2));

    fds = close_fds::iter_possible_fds(fd2).collect();
    check_sorted(&fds);
    assert!(!fds.contains(&fd1));
    assert!(fds.contains(&fd2));

    fds = close_fds::iter_possible_fds(fd3).collect();
    check_sorted(&fds);
    assert!(!fds.contains(&fd1));
    assert!(!fds.contains(&fd2));
}

fn close_fds_test(fd1: libc::c_int, fd2: libc::c_int, fd3: libc::c_int) {
    let mut fds: Vec<libc::c_int>;

    fds = close_fds::iter_open_fds(fd1).collect();
    check_sorted(&fds);
    assert!(fds.contains(&fd1));
    assert!(fds.contains(&fd2));
    assert!(!fds.contains(&fd3));

    // Add 0 just to check the case where keep_fds[0] < minfd
    unsafe {
        close_fds::close_open_fds(fd1, &[0]);
    }

    fds = close_fds::iter_open_fds(fd1).collect();
    check_sorted(&fds);
    assert!(!fds.contains(&fd1));
    assert!(!fds.contains(&fd2));
    assert!(!fds.contains(&fd3));
}

fn close_fds_keep1_test(fd1: libc::c_int, fd2: libc::c_int, fd3: libc::c_int) {
    let mut fds: Vec<libc::c_int>;

    fds = close_fds::iter_open_fds(fd1).collect();
    check_sorted(&fds);
    assert!(fds.contains(&fd1));
    assert!(fds.contains(&fd2));
    assert!(!fds.contains(&fd3));

    unsafe {
        close_fds::close_open_fds(fd1, &[fd1, fd3]);
    }

    fds = close_fds::iter_open_fds(fd1).collect();
    check_sorted(&fds);
    assert!(fds.contains(&fd1));
    assert!(!fds.contains(&fd2));
    assert!(!fds.contains(&fd3));
}

fn close_fds_keep2_test(fd1: libc::c_int, fd2: libc::c_int, fd3: libc::c_int) {
    let mut fds: Vec<libc::c_int>;

    fds = close_fds::iter_open_fds(fd1).collect();
    check_sorted(&fds);
    assert!(fds.contains(&fd1));
    assert!(fds.contains(&fd2));
    assert!(!fds.contains(&fd3));

    unsafe {
        close_fds::close_open_fds(fd1, &[fd2, fd3]);
    }

    fds = close_fds::iter_open_fds(fd1).collect();
    check_sorted(&fds);
    assert!(!fds.contains(&fd1));
    assert!(fds.contains(&fd2));
    assert!(!fds.contains(&fd3));
}

fn large_open_fds_test(mangle_keep_fds: fn(&mut [libc::c_int])) {
    let mut openfds = Vec::new();

    for _ in 0..150 {
        let fd = std::fs::File::open("/").unwrap().into_raw_fd();
        openfds.push(fd);

        // Test that is_fd_cloexec() and set_fd_cloexec() behave properly
        assert_eq!(is_fd_cloexec(fd), Some(true));
        set_fd_cloexec(fd, false);
        assert_eq!(is_fd_cloexec(fd), Some(false));
        set_fd_cloexec(fd, true);
        assert_eq!(is_fd_cloexec(fd), Some(true));
    }

    let lowfd = openfds[0];

    let mut closedfds = Vec::new();

    // Close a few
    for &index in &[140, 110, 100, 75, 10] {
        let fd = openfds.remove(index);
        unsafe {
            libc::close(fd);
        }
        closedfds.push(fd);

        assert_eq!(is_fd_cloexec(fd), None);
    }

    let mut cur_open_fds: Vec<libc::c_int>;

    // Check that our expectations of which should be open and which
    // should be closed are correct
    cur_open_fds = close_fds::iter_open_fds(lowfd).collect();
    check_sorted(&cur_open_fds);
    for fd in openfds.iter() {
        assert_eq!(is_fd_cloexec(*fd), Some(true));
        assert!(cur_open_fds.contains(fd));
    }
    for fd in closedfds.iter() {
        assert_eq!(is_fd_cloexec(*fd), None);
        assert!(!cur_open_fds.contains(fd));
    }

    // Set them all as non-close-on-exec
    for fd in openfds.iter() {
        set_fd_cloexec(*fd, false);
        assert_eq!(is_fd_cloexec(*fd), Some(false));
    }
    // Make them all close-on-exec with set_fds_cloexec()
    close_fds::set_fds_cloexec(lowfd, &[]);
    // Now make sure they're all close-on-exec
    for fd in openfds.iter() {
        assert_eq!(is_fd_cloexec(*fd), Some(true));
    }

    // Set them all as non-close-on-exec again
    for fd in openfds.iter() {
        set_fd_cloexec(*fd, false);
        assert_eq!(is_fd_cloexec(*fd), Some(false));
    }
    // Make them all close-on-exec with set_fds_cloexec_threadsafe()
    close_fds::set_fds_cloexec_threadsafe(lowfd, &[]);
    // Now make sure they're all close-on-exec again
    for fd in openfds.iter() {
        assert_eq!(is_fd_cloexec(*fd), Some(true));
    }

    // Now, let's only keep a few file descriptors open.
    let mut extra_closedfds = openfds.to_vec();
    extra_closedfds.sort_unstable();
    openfds.clear();
    for &index in &[130, 110, 90, 89, 65, 34, 21, 3] {
        openfds.push(extra_closedfds.remove(index));
    }
    closedfds.extend_from_slice(&extra_closedfds);

    mangle_keep_fds(&mut openfds);

    // Set them all as non-close-on-exec
    for fd in openfds.iter().chain(extra_closedfds.iter()) {
        set_fd_cloexec(*fd, false);
    }
    // Make most of them close-on-exec with set_fds_cloexec_threadsafe()
    close_fds::set_fds_cloexec(lowfd, &openfds);
    // Now make sure they're all close-on-exec again
    for fd in openfds.iter() {
        assert_eq!(is_fd_cloexec(*fd), Some(false));
    }
    for fd in extra_closedfds.iter() {
        assert_eq!(is_fd_cloexec(*fd), Some(true));
    }

    // Set them all as non-close-on-exec
    for fd in openfds.iter().chain(extra_closedfds.iter()) {
        set_fd_cloexec(*fd, false);
    }
    // Make most of them close-on-exec with set_fds_cloexec_threadsafe()
    close_fds::set_fds_cloexec_threadsafe(lowfd, &openfds);
    // Check that it worked properly
    for fd in openfds.iter() {
        assert_eq!(is_fd_cloexec(*fd), Some(false));
    }
    for fd in extra_closedfds.iter() {
        assert_eq!(is_fd_cloexec(*fd), Some(true));
    }

    unsafe {
        close_fds::close_open_fds(lowfd, &openfds);
    }

    // Again, check that our expectations match reality
    cur_open_fds = close_fds::iter_open_fds(lowfd).collect();
    check_sorted(&cur_open_fds);
    for fd in openfds.iter() {
        assert!(cur_open_fds.contains(fd));
    }
    for fd in closedfds.iter() {
        assert!(!cur_open_fds.contains(fd));
    }
    assert_eq!(
        cur_open_fds,
        close_fds::iter_open_fds_threadsafe(lowfd).collect::<Vec<libc::c_int>>()
    );

    for &fd in openfds.iter() {
        unsafe {
            libc::close(fd);
        }
    }
}

#[test]
fn run_tests() {
    // Run all tests here because these tests can't be run in parallel

    run_basic_test(iter_open_fds_test);

    run_basic_test(iter_possible_fds_test);

    run_basic_test(close_fds_test);

    run_basic_test(close_fds_keep1_test);
    run_basic_test(close_fds_keep2_test);

    large_open_fds_test(|keep_fds| keep_fds.sort_unstable());
    large_open_fds_test(|_keep_fds| ());
    large_open_fds_test(|keep_fds| {
        keep_fds.sort_unstable_by(|a, b| ((a + b) % 5).cmp(&3));
    });

    unsafe {
        close_fds::close_open_fds(3, &[]);
    }
    assert_eq!(
        close_fds::iter_open_fds(0).collect::<Vec::<libc::c_int>>(),
        vec![0, 1, 2]
    );

    unsafe {
        close_fds::close_open_fds(0, &[0, 1, 2]);
    }
    assert_eq!(
        close_fds::iter_open_fds(0).collect::<Vec::<libc::c_int>>(),
        vec![0, 1, 2]
    );

    unsafe {
        close_fds::close_open_fds(-1, &[0, 1, 2]);
    }
    assert_eq!(
        close_fds::iter_open_fds(0).collect::<Vec::<libc::c_int>>(),
        vec![0, 1, 2]
    );
}

#[test]
fn run_no_fds_tests() {
    // This is an edge case of the fused tests. The case where *no* file descriptors are open
    // requires special handling on some platforms, and this test exercises that code.
    //
    // We run this test in a separate process because we can't just close all file descriptors in
    // the current process. That also means that we can run it in parallel with the other tests,
    // since it only affects the subprocess.

    match unsafe { libc::fork() } {
        0 => {
            // Close all open file descriptors
            unsafe {
                close_fds::close_open_fds(0, &[]);
            }

            // (Note: From here on, panic()ing is of limited value since it can't display a
            // backtrace. So we exit() with different values on error, so that the error code will
            // tell us what failed.)

            // Create an FdIter
            let mut fditer = close_fds::iter_open_fds(0);

            // It's empty.
            if fditer.next().is_some() {
                std::process::exit(1);
            }

            // Now we open another file descriptor.
            if unsafe { libc::open("/\0".as_ptr() as *const libc::c_char, libc::O_RDONLY) } < 0 {
                std::process::exit(2);
            }
            // A new FdIter knows about the new file descriptor...
            if close_fds::iter_open_fds(0).next() != Some(0) {
                std::process::exit(3);
            }

            // But the old one doesn't, because it's fused.
            if fditer.next().is_some() {
                std::process::exit(4);
            }

            // And it knows it.
            if fditer.size_hint() != (0, Some(0)) {
                std::process::exit(5);
            }
            if fditer.count() != 0 {
                std::process::exit(6);
            }

            std::process::exit(0);
        }
        ret if ret < 0 => panic!("Error fork()ing: {}", std::io::Error::last_os_error()),
        pid => unsafe {
            let mut stat = 0;

            if libc::waitpid(pid, &mut stat, 0) < 0 {
                panic!(
                    "Error wait()ing for child: {}",
                    std::io::Error::last_os_error()
                )
            }

            assert!(libc::WIFEXITED(stat), "Process did not exit normally");
            assert_eq!(
                libc::WEXITSTATUS(stat),
                0,
                "Process exited with non-zero value"
            );
        },
    }
}
