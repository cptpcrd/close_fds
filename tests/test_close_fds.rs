#[cfg(unix)]
fn is_fd_open(fd: libc::c_int) -> bool {
    unsafe { libc::fcntl(fd, libc::F_GETFD) >= 0 }
}
#[cfg(windows)]
fn is_fd_open(fd: libc::c_int) -> bool {
    unsafe { libc::get_osfhandle(fd) >= 0 }
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

#[cfg(unix)]
fn run_basic_test(callback: fn(fd1: libc::c_int, fd2: libc::c_int, fd3: libc::c_int)) {
    use std::os::unix::prelude::*;

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

#[cfg(windows)]
fn run_basic_test(callback: fn(fd1: libc::c_int, fd2: libc::c_int, fd3: libc::c_int)) {
    fn openfd() -> libc::c_int {
        let path = std::ffi::CString::new(
            std::env::current_exe()
                .unwrap()
                .into_os_string()
                .into_string()
                .unwrap()
                .into_bytes(),
        )
        .unwrap();
        let fd = unsafe { libc::open(path.as_ptr(), libc::O_RDONLY) };
        assert!(fd >= 0, "{}", std::io::Error::last_os_error());
        fd
    }

    let fd1 = openfd();
    let fd2 = openfd();
    let fd3 = openfd();

    unsafe {
        libc::close(fd3);
    }

    assert!(is_fd_open(fd1));
    assert!(is_fd_open(fd2));
    assert!(!is_fd_open(fd3));

    callback(fd1, fd2, fd3);

    unsafe {
        libc::close(fd1);
        libc::close(fd2);
    }
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

    unsafe {
        close_fds::close_open_fds(fd1, &[]);
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

    #[cfg(unix)]
    {
        use std::os::unix::prelude::*;

        for _ in 0..150 {
            openfds.push(std::fs::File::open("/").unwrap().into_raw_fd());
        }
    }
    #[cfg(windows)]
    {
        let path = std::ffi::CString::new(
            std::env::current_exe()
                .unwrap()
                .into_os_string()
                .into_string()
                .unwrap()
                .into_bytes(),
        )
        .unwrap();
        for _ in 0..150 {
            let fd = unsafe { libc::open(path.as_ptr(), libc::O_RDONLY) };
            assert!(fd >= 0, "{}", std::io::Error::last_os_error());
            openfds.push(fd);
        }
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
    }

    let mut cur_open_fds: Vec<libc::c_int>;

    // Check that our expectations of which should be open and which
    // should be closed are correct
    cur_open_fds = close_fds::iter_open_fds(lowfd).collect();
    check_sorted(&cur_open_fds);
    for fd in openfds.iter() {
        assert!(cur_open_fds.contains(fd));
    }
    for fd in closedfds.iter() {
        assert!(!cur_open_fds.contains(fd));
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

    for &fd in openfds.iter() {
        unsafe {
            libc::close(fd);
        }
    }
}

#[test]
fn run_tests() {
    // Run all tests here because these tests can't be run in parallel

    #[cfg(windows)]
    {
        // On Windows, we have to set an "invalid parameter handler" to keep our
        // is_fd_open() helper from segfaulting if the file descriptor is invalid.

        extern "C" fn handle_invalid_param(
            _expression: *const libc::wchar_t,
            _function: *const libc::wchar_t,
            _file: *const libc::wchar_t,
            _line: libc::c_uint,
            _p_reserved: libc::uintptr_t,
        ) {
        }
        extern "C" {
            fn _set_invalid_parameter_handler(
                p_new: extern "C" fn(
                    expression: *const libc::wchar_t,
                    function: *const libc::wchar_t,
                    file: *const libc::wchar_t,
                    line: libc::c_uint,
                    p_reserved: libc::uintptr_t,
                ),
            ) -> extern "C" fn(
                expression: *const libc::wchar_t,
                function: *const libc::wchar_t,
                file: *const libc::wchar_t,
                line: libc::c_uint,
                p_reserved: libc::uintptr_t,
            );
        }

        unsafe {
            _set_invalid_parameter_handler(handle_invalid_param);
        }
    }

    run_basic_test(iter_open_fds_test);

    run_basic_test(iter_possible_fds_test);

    run_basic_test(close_fds_test);

    run_basic_test(close_fds_keep1_test);
    run_basic_test(close_fds_keep2_test);

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
