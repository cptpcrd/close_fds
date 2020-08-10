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
    assert!(fds.contains(&0));
    assert!(fds.contains(&fd1));
    assert!(fds.contains(&fd2));
    assert!(!fds.contains(&fd3));

    fds = close_fds::iter_open_fds(0).collect();
    assert!(fds.contains(&0));
    assert!(fds.contains(&fd1));
    assert!(fds.contains(&fd2));
    assert!(!fds.contains(&fd3));

    // Test handling of minfd

    fds = close_fds::iter_open_fds(fd1).collect();
    assert!(!fds.contains(&0));
    assert!(fds.contains(&fd1));
    assert!(fds.contains(&fd2));
    assert!(!fds.contains(&fd3));

    fds = close_fds::iter_open_fds(fd2).collect();
    assert!(!fds.contains(&0));
    assert!(!fds.contains(&fd1));
    assert!(fds.contains(&fd2));
    assert!(!fds.contains(&fd3));

    fds = close_fds::iter_open_fds(fd3).collect();
    assert!(!fds.contains(&0));
    assert!(!fds.contains(&fd1));
    assert!(!fds.contains(&fd2));
    assert!(!fds.contains(&fd3));
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

fn close_fds_test(fd1: RawFd, fd2: RawFd, fd3: RawFd) {
    let mut fds: Vec<RawFd>;

    fds = close_fds::iter_open_fds(fd1).collect();
    assert!(fds.contains(&fd1));
    assert!(fds.contains(&fd2));
    assert!(!fds.contains(&fd3));

    unsafe {
        close_fds::close_open_fds(fd1, &[]);
    }

    fds = close_fds::iter_open_fds(fd1).collect();
    assert!(!fds.contains(&fd1));
    assert!(!fds.contains(&fd2));
    assert!(!fds.contains(&fd3));
}

fn close_fds_keep1_test(fd1: RawFd, fd2: RawFd, fd3: RawFd) {
    let mut fds: Vec<RawFd>;

    fds = close_fds::iter_open_fds(fd1).collect();
    assert!(fds.contains(&fd1));
    assert!(fds.contains(&fd2));
    assert!(!fds.contains(&fd3));

    unsafe {
        close_fds::close_open_fds(fd1, &[fd1, fd3]);
    }

    fds = close_fds::iter_open_fds(fd1).collect();
    assert!(fds.contains(&fd1));
    assert!(!fds.contains(&fd2));
    assert!(!fds.contains(&fd3));
}

fn close_fds_keep2_test(fd1: RawFd, fd2: RawFd, fd3: RawFd) {
    let mut fds: Vec<RawFd>;

    fds = close_fds::iter_open_fds(fd1).collect();
    assert!(fds.contains(&fd1));
    assert!(fds.contains(&fd2));
    assert!(!fds.contains(&fd3));

    unsafe {
        close_fds::close_open_fds(fd1, &[fd2, fd3]);
    }

    fds = close_fds::iter_open_fds(fd1).collect();
    assert!(!fds.contains(&fd1));
    assert!(fds.contains(&fd2));
    assert!(!fds.contains(&fd3));
}

fn large_open_fds_test() {
    // Open a lot of file descriptors
    let mut files = Vec::with_capacity(150);
    for _ in 0..150 {
        files.push(fs::File::open("/").unwrap());
    }
    let lowfd = files[0].as_raw_fd();

    let mut openfds = Vec::new();
    let mut closedfds = Vec::new();

    // Close a few
    for &index in &[140, 100, 10] {
        let f: fs::File = files.remove(index);
        closedfds.push(f.as_raw_fd());
    }
    for f in files.iter() {
        openfds.push(f.as_raw_fd());
    }

    let mut cur_open_fds: Vec<RawFd>;

    // Check that our expectations of which should be open and which
    // should be closed are correct
    cur_open_fds = close_fds::iter_open_fds(lowfd).collect();
    println!("{:?}", cur_open_fds);
    for fd in openfds.iter() {
        assert!(cur_open_fds.contains(fd));
    }
    for fd in closedfds.iter() {
        assert!(!cur_open_fds.contains(fd));
    }

    // Now, let's only keep a few file descriptors open.
    closedfds.extend_from_slice(&openfds);
    closedfds.sort_unstable();
    openfds.clear();
    for &index in &[130, 90, 3] {
        openfds.push(closedfds.remove(index));
    }

    unsafe {
        close_fds::close_open_fds(lowfd, &openfds);
    }

    // Again, check that our expectations match reality
    cur_open_fds = close_fds::iter_open_fds(lowfd).collect();
    for fd in openfds.iter() {
        assert!(cur_open_fds.contains(fd));
    }
    for fd in closedfds.iter() {
        assert!(!cur_open_fds.contains(fd));
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

    large_open_fds_test();
}
