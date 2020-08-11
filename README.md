# close_fds

[![crates.io](https://img.shields.io/crates/v/close_fds.svg)](https://crates.io/crates/close_fds)
[![Docs](https://docs.rs/close_fds/badge.svg)](https://docs.rs/close_fds)
[![GitHub Actions](https://github.com/cptpcrd/close_fds/workflows/CI/badge.svg?branch=master&event=push)](https://github.com/cptpcrd/close_fds/actions?query=workflow%3ACI+branch%3Amaster+event%3Apush)
[![Cirrus CI](https://api.cirrus-ci.com/github/cptpcrd/close_fds.svg?branch=master)](https://cirrus-ci.com/github/cptpcrd/close_fds)
[![codecov](https://codecov.io/gh/cptpcrd/close_fds/branch/master/graph/badge.svg)](https://codecov.io/gh/cptpcrd/close_fds)

A small Rust library that makes it easy to close all open file descriptors.

## OS support

`close_fds` has three OS support tiers, similar to [Rust's support tiers](https://forge.rust-lang.org/release/platform-support.html):

#### Tier 1: "Guaranteed to work" (tested in CI)

- Linux (glibc and musl)
- macOS
- FreeBSD
- Windows (MSVC and GNU toolchains)
  Note: `close_fds` does NOT have support for closing open file *handles*; just file *descriptors*.

#### Tier 2: "Guaranteed to build"  (built, but not tested, in CI)

- NetBSD

#### Tier 3: "Should work"

- OpenBSD
- DragonflyBSD

*Note: As stated in the [license](LICENSE), `close_fds` comes with no warranty.*


## OS-specific notes

Here is a list of the methods that `iter_open_fds()`, `iter_possible_fds()`, and `close_open_fds()` will try on various platforms to improve performance when listing the open file descriptors:

- Linux
    - `/proc/self/fd` if `/proc` is mounted (very efficient)
- macOS
    - `/dev/fd` (very efficient)
- FreeBSD
    - `/dev/fd` if an [`fdescfs`](https://www.freebsd.org/cgi/man.cgi?query=fdescfs&manpath=FreeBSD+12.1-RELEASE+and+Ports) appears to be mounted there (very efficient)
    - `sysctl kern.proc.nfds` to get the number of open file descriptors (moderately efficient, unless large numbers of file descriptors are open)
- NetBSD
    - `fcntl(0, F_MAXFD)` to get the maximum open file descriptor (moderately efficient)

On the BSDs, `close_open_fds()` may also call `closefrom()`, which is very efficient.

If none of the methods listed above are available, it will fall back on a simple loop through every possible file descriptor number -- from `minfd` to `sysconf(_SC_OPEN_MAX)` (`getmaxstdio()` on Windows).

Note: The most common use case, `close_open_fds(3, &[])`, is very efficient on Linux, macOS, and all of the BSDs.
