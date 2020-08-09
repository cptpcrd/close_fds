use std::fs;
use std::os::unix::prelude::*;

fn is_fd_open(fd: RawFd) -> bool {
    unsafe { libc::fcntl(fd, libc::F_GETFD) >= 0 }
}

fn run_basic_test(callback: fn(fd1: RawFd, fd2: RawFd, fd3: RawFd)) {
    let f1 = fs::File::open("/").unwrap();
    let f2 = fs::File::open("/").unwrap();
    let f3 = fs::File::open("/").unwrap();

    let fd1 = f1.as_raw_fd();
    let fd2 = f2.as_raw_fd();
    let fd3 = f3.as_raw_fd();

    drop(f3);

    assert!(is_fd_open(fd1));
    assert!(is_fd_open(fd2));
    assert!(!is_fd_open(fd3));

    callback(fd1, fd2, fd3);
}

fn iter_open_fds_test(fd1: RawFd, fd2: RawFd, fd3: RawFd) {
    let mut fds: Vec<RawFd>;

    fds = close_fds::iter_open_fds(-1).collect();
    assert!(!fds.contains(&-1));
    assert!(fds.contains(&fd1));
    assert!(fds.contains(&fd2));
    assert!(!fds.contains(&fd3));

    fds = close_fds::iter_open_fds(0).collect();
    assert!(fds.contains(&fd1));
    assert!(fds.contains(&fd2));
    assert!(!fds.contains(&fd3));

    // Test handling of minfd
    assert_eq!(close_fds::iter_open_fds(fd1).collect::<Vec<RawFd>>(), [fd1, fd2]);
    assert_eq!(close_fds::iter_open_fds(fd2).collect::<Vec<RawFd>>(), [fd2]);
    assert_eq!(close_fds::iter_open_fds(fd3).collect::<Vec<RawFd>>(), []);
}

fn iter_possible_fds_test(fd1: RawFd, fd2: RawFd, fd3: RawFd) {
    let mut fds: Vec<RawFd>;

    fds = close_fds::iter_possible_fds(-1).collect();
    assert!(!fds.contains(&-1));
    assert!(fds.contains(&fd1));
    assert!(fds.contains(&fd2));
    // Don't check for fd3 not being present because it might legitimately be
    // returned by iter_possible_fds()

    fds = close_fds::iter_possible_fds(0).collect();
    assert!(fds.contains(&fd1));
    assert!(fds.contains(&fd2));

    // Test handling of minfd

    fds = close_fds::iter_possible_fds(fd1).collect();
    assert!(fds.contains(&fd1));
    assert!(fds.contains(&fd2));

    fds = close_fds::iter_possible_fds(fd2).collect();
    assert!(!fds.contains(&fd1));
    assert!(fds.contains(&fd2));

    fds = close_fds::iter_possible_fds(fd3).collect();
    assert!(!fds.contains(&fd1));
    assert!(!fds.contains(&fd2));
}

fn close_fds_test(fd1: RawFd, fd2: RawFd, _fd3: RawFd) {
    assert_eq!(close_fds::iter_open_fds(fd1).collect::<Vec<RawFd>>(), [fd1, fd2]);

    unsafe {
        close_fds::close_open_fds(fd1, &[]);
    }

    assert_eq!(close_fds::iter_open_fds(fd1).collect::<Vec<RawFd>>(), []);
}

fn close_fds_keep1_test(fd1: RawFd, fd2: RawFd, fd3: RawFd) {
    assert_eq!(close_fds::iter_open_fds(fd1).collect::<Vec<RawFd>>(), [fd1, fd2]);

    unsafe {
        close_fds::close_open_fds(fd1, &[fd1, fd3]);
    }

    assert_eq!(close_fds::iter_open_fds(fd1).collect::<Vec<RawFd>>(), [fd1]);
}

fn close_fds_keep2_test(fd1: RawFd, fd2: RawFd, fd3: RawFd) {
    assert_eq!(close_fds::iter_open_fds(fd1).collect::<Vec<RawFd>>(), [fd1, fd2]);

    unsafe {
        close_fds::close_open_fds(fd1, &[fd2, fd3]);
    }

    assert_eq!(close_fds::iter_open_fds(fd1).collect::<Vec<RawFd>>(), [fd2]);
}

#[test]
fn run_tests() {
    // Run all tests here because these tests can't be run in parallel

    run_basic_test(iter_open_fds_test);

    run_basic_test(iter_possible_fds_test);

    run_basic_test(close_fds_test);

    run_basic_test(close_fds_keep1_test);
    run_basic_test(close_fds_keep2_test);
}
