# close_fds

[![crates.io](https://img.shields.io/crates/v/close_fds.svg)](https://crates.io/crates/close_fds)
[![Docs](https://docs.rs/close_fds/badge.svg)](https://docs.rs/close_fds)
[![GitHub Actions](https://github.com/cptpcrd/close_fds/workflows/CI/badge.svg?branch=master&event=push)](https://github.com/cptpcrd/close_fds/actions?query=workflow%3ACI+branch%3Amaster+event%3Apush)
[![Cirrus CI](https://api.cirrus-ci.com/github/cptpcrd/close_fds.svg?branch=master)](https://cirrus-ci.com/github/cptpcrd/close_fds)
[![codecov](https://codecov.io/gh/cptpcrd/close_fds/branch/master/graph/badge.svg)](https://codecov.io/gh/cptpcrd/close_fds)

A small Rust library that makes it easy to close all open file descriptors.

## Usage

Add to your `Cargo.toml`:

```
[dependencies]
close_fds = "0.2"
```

In your application:

```
use close_fds::close_open_fds;

fn main() {
    // ...
    unsafe {
        close_open_fds(3, &[]);
    }
    // ...
}
```

**IMPORTANT**: Please read the documentation for [`close_open_fds()`](http://docs.rs/close_fds/latest/close_fds/fn.close_open_fds.html) for an explanation of why it is `unsafe`.

The first argument to `close_open_fds()` is the lowest file descriptor that should be closed; all file descriptors less than this will be left open. The second argument is a slice containing a list of additional file descriptors that should be left open. (Note: `close_open_fds()` will be more efficient if this list is sorted, especially if it is more than a few elements long.)

`close_open_fds()` *always* succeeds. If one method of closing the file descriptors fails, it will fall back on another.

Some other helpful functions in this crate (more details in the [documentation](http://docs.rs/close_fds/latest)):

- `set_fds_cloexec(minfd, keep_fds)`: Identical to `close_open_fds()`, but sets the `FD_CLOEXEC` flag on the file descriptors instead of closing them.
- `iter_open_fds(minfd)`: Iterates over all open file descriptors for the current process, starting at `minfd`.
- `iter_possible_fds(minfd)` (not recommended): Identical to `iter_open_fds()`, but may yield invalid file descriptors; the caller is responsible for checking whether they are valid.

Note that `close_open_fds()` should be preferred whenever possible, as it may be able to take advantage of platform-specific optimizations that these other functions cannot.

## OS support

`close_fds` has three OS support tiers, similar to [Rust's support tiers](https://forge.rust-lang.org/release/platform-support.html):

#### Tier 1: "Guaranteed to work" (tested in CI)

- Linux (glibc and musl)
- macOS
- FreeBSD

#### Tier 2: "Guaranteed to build"  (built, but not tested, in CI)

- NetBSD
- Solaris

#### Tier 3: "Should work"

- OpenBSD
- DragonflyBSD
- Illumos

*Note: As stated in the [license](LICENSE), `close_fds` comes with no warranty.*


## OS-specific notes

Here is a list of the methods that `iter_open_fds()`, `iter_possible_fds()`, `close_open_fds()`, and `set_fds_cloexec()` will try on various platforms to improve performance when listing the open file descriptors:

- Linux
    - `/proc/self/fd` if `/proc` is mounted (very efficient)
- macOS
    - `/dev/fd` (very efficient)
- FreeBSD
    - `/dev/fd` if an [`fdescfs`](https://www.freebsd.org/cgi/man.cgi?query=fdescfs) appears to be mounted there (very efficient)
    - The `kern.proc.nfds` sysctl to get the number of open file descriptors (moderately efficient unless large numbers of file descriptors are open; not used by `close_open_fds()`)
- NetBSD
    - `fcntl(0, F_MAXFD)` to get the maximum open file descriptor (moderately efficient)
- Solaris and Illumos
    - `/dev/fd` or `/proc/self/fd` if either is available (very efficient)

On the BSDs, `close_open_fds()` may also call `closefrom()`, which is very efficient.

If none of the methods listed above are available, it will fall back on a simple loop through every possible file descriptor number -- from `minfd` to `sysconf(_SC_OPEN_MAX)`.

Note: The most common use case, `close_open_fds(3, &[])`, is very efficient on Linux (with `/proc` mounted), macOS, all of the BSDs, and Solaris/Illumos.
