# Real Example — End-to-End xfetch Packaging with xpkg for xpm

This document records a real, reproducible packaging run for `xfetch` using `xpkg`, plus synchronization validation from `xpm` against local `file://` repositories.

Execution date: 2026-03-17

## Goals

1. Create an `XBUILD` recipe for `xfetch`.
2. Build a real `.xp` package with `xpkg`.
3. Validate the artifact with `xpkg info` and `xpkg lint`.
4. Publish it to static repository layouts.
5. Verify that `xpm sync` downloads and parses the `.db`.
6. Reproduce the same flow directly inside `x-repo` layout.

## Repository URLs (updated owner)

The project owner moved from the personal `xscriptor` account to the `equislinux` organization.

- New repository URL: `https://github.com/equislinux/x-repo`
- Current GitHub Pages endpoint still used in tooling: `https://equislinux.github.io/x-repo`

## Repositories Used

- `~/Documents/repos/xpkgrepos/xfetch`
- `~/Documents/repos/xpkgrepos/xpkg`
- `~/Documents/repos/xpkgrepos/xpm`
- `~/Documents/repos/xpkgrepos/x-repo`

`xfetch` snapshot used:

- short commit: `41ad407`
- total commits: `31`

## Recipe File Created

File:

- `~/Documents/repos/xpkgrepos/xfetch/packaging/xpkg/XBUILD`

Recipe content:

```toml
[package]
name = "xfetch"
version = "0.1.0"
release = 1
description = "Cross-platform system information fetch tool written in Rust"
url = "https://github.com/xfetch-cli/xfetch"
license = ["MIT"]
arch = ["x86_64"]

[dependencies]
depends = ["glibc"]
makedepends = ["cargo", "gcc", "rust"]

[source]
urls = []

[build]
prepare = """
REPO_ROOT=\"$(cd \"$startdir/../..\" && pwd)\"
rm -rf \"$SRCDIR/xfetch-src\"
mkdir -p \"$SRCDIR/xfetch-src\"
cp -a \"$REPO_ROOT/.\" \"$SRCDIR/xfetch-src/\"
rm -rf \"$SRCDIR/xfetch-src/target\"
"""

build = """
cd \"$SRCDIR/xfetch-src\"
cargo build --release --locked
"""

package = """
cd \"$SRCDIR/xfetch-src\"
install -Dm755 \"target/release/xfetch\" \"$PKGDIR/usr/bin/xfetch\"
install -Dm644 LICENSE \"$PKGDIR/usr/share/licenses/xfetch/LICENSE\"
install -Dm644 README.md \"$PKGDIR/usr/share/doc/xfetch/README.md\"
cp -a configs \"$PKGDIR/usr/share/xfetch/\"
cp -a logos \"$PKGDIR/usr/share/xfetch/\"
"""
```

## Full Reproduction Run (Fresh)

### 1. Build package

```bash
cd ~/Documents/repos/xpkgrepos/xpkg
cargo run -p xpkg -- build \
  -f ~/Documents/repos/xpkgrepos/xfetch/packaging/xpkg/XBUILD \
  --builddir /tmp/xpkg-real2-build \
  --outdir /tmp/xpkg-real2-out \
  --no-check -vv
```

Main result:

- Build succeeded.
- Binary stripping applied (`Stripped 1 ELF binaries`).
- Artifact generated:
  - `/tmp/xpkg-real2-out/xfetch-0.1.0-1-x86_64.xp`
  - reported size: `1302.1 KiB`

### 2. Inspect package metadata

```bash
cd ~/Documents/repos/xpkgrepos/xpkg
cargo run -p xpkg -- info /tmp/xpkg-real2-out/xfetch-0.1.0-1-x86_64.xp --files
```

Key output:

- `Name: xfetch`
- `Architecture: x86_64`
- `Depends On: glibc`
- `Files: 47`
- Includes:
  - `usr/bin/xfetch`
  - `usr/share/licenses/xfetch/LICENSE`
  - `usr/share/doc/xfetch/README.md`
  - assets under `usr/share/xfetch/` (`configs`, `logos`)

### 3. Lint package

```bash
cd ~/Documents/repos/xpkgrepos/xpkg
cargo run -p xpkg -- lint /tmp/xpkg-real2-out/xfetch-0.1.0-1-x86_64.xp
```

Result:

- `0 error(s), 0 warning(s), 4 info(s)`
- Info lines for expected ELF dependencies (`libc`, `libm`, `libgcc_s`, `ld-linux-x86-64.so.2`)

### 4. Compute checksum

```bash
cd /tmp/xpkg-real2-out
sha256sum xfetch-0.1.0-1-x86_64.xp
```

Checksum:

- `15a4091ae2c61750c0c2201116fd128eb6197ab580bb7ad03f55e901a389bec0`

## Local Static Repo Test (xlocal)

### 1. Create repository database

```bash
mkdir -p /tmp/xpkg-real-repo/xlocal/os/x86_64
cd ~/Documents/repos/xpkgrepos/xpkg
cargo run -p xpkg -- repo-add \
  /tmp/xpkg-real-repo/xlocal/os/x86_64/xlocal.db.tar.gz \
  /tmp/xpkg-real2-out/xfetch-0.1.0-1-x86_64.xp
```

### 2. Expose pacman-style filename for xpm sync

```bash
cp /tmp/xpkg-real-repo/xlocal/os/x86_64/xlocal.db.tar.gz /tmp/xpkg-real-repo/xlocal/os/x86_64/xlocal.db
cp /tmp/xpkg-real2-out/xfetch-0.1.0-1-x86_64.xp /tmp/xpkg-real-repo/xlocal/os/x86_64/
```

### 3. Sync with xpm

Config file used:

- `/tmp/xpm-realexample.conf`

```toml
[options]
root_dir = "/"
db_path = "/tmp/xpm-real-db"
cache_dir = "/tmp/xpm-real-cache"
log_file = "/tmp/xpm-real.log"
gpg_dir = "/tmp/xpm-real-gpg"
sig_level = "optional"
parallel_downloads = 2
color = true
check_space = false
architecture = "x86_64"

[[repo]]
name = "xlocal"
server = ["file:///tmp/xpkg-real-repo/$repo/os/$arch"]
```

Run:

```bash
cd ~/Documents/repos/xpkgrepos/xpm
cargo run -p xpm -- --config /tmp/xpm-realexample.conf sync
```

Result:

- Mirror expansion succeeded.
- `xlocal.db` downloaded and parsed.
- `1 package(s) loaded`.
- `xlocal.files` missing in this test (expected warning, non-blocking).

## Integration Test with x-repo Layout

This reproduces the same flow directly in the `x-repo` publish directory used for Pages.

### 1. Add package to x-repo database

```bash
mkdir -p ~/Documents/repos/xpkgrepos/x-repo/public/repo/x86_64
cd ~/Documents/repos/xpkgrepos/xpkg
cargo run -p xpkg -- repo-add \
  ~/Documents/repos/xpkgrepos/x-repo/public/repo/x86_64/x.db.tar.gz \
  /tmp/xpkg-real2-out/xfetch-0.1.0-1-x86_64.xp
```

### 2. Prepare static files expected by current xpm sync

```bash
cp ~/Documents/repos/xpkgrepos/x-repo/public/repo/x86_64/x.db.tar.gz \
   ~/Documents/repos/xpkgrepos/x-repo/public/repo/x86_64/x.db
```

The `.xp` artifact is intentionally **not** copied into `x-repo`.
For this run, it is hosted in `xfetch` Releases:

- `https://github.com/xfetch-cli/xfetch/releases/download/xfetch-0.1.0-1/xfetch-0.1.0-1-x86_64.xp`

### 3. Validate with xpm

Config file used:

- `/tmp/xpm-realexample-xrepo.conf`

```toml
[options]
root_dir = "/"
db_path = "/tmp/xpm-real-db-xrepo"
cache_dir = "/tmp/xpm-real-cache-xrepo"
log_file = "/tmp/xpm-real-xrepo.log"
gpg_dir = "/tmp/xpm-real-gpg"
sig_level = "optional"
parallel_downloads = 2
color = true
check_space = false
architecture = "x86_64"

[[repo]]
name = "x"
server = ["file://$HOME/Documents/repos/xpkgrepos/x-repo/public/repo/$arch"]
```

Run:

```bash
cd ~/Documents/repos/xpkgrepos/xpm
cargo run -p xpm -- --config /tmp/xpm-realexample-xrepo.conf sync
```

Result:

- Mirror resolved to local `x-repo/public/repo/x86_64`.
- `x.db` downloaded and parsed.
- `1 package(s) loaded`.
- `x.files` missing in this test (expected warning, non-blocking).

## Files/Directories Created

Persistent repository files:

- `~/Documents/repos/xpkgrepos/xfetch/packaging/xpkg/XBUILD`
- `~/Documents/repos/xpkgrepos/xpkg/docs/realexample.md`
- `~/Documents/repos/xpkgrepos/x-repo/public/repo/x86_64/x.db.tar.gz`
- `~/Documents/repos/xpkgrepos/x-repo/public/repo/x86_64/x.db`

Temporary execution files:

- `/tmp/xpkg-real2-build/`
- `/tmp/xpkg-real2-out/`
- `/tmp/xpkg-real-repo/`
- `/tmp/xpm-real-db/`
- `/tmp/xpm-real-cache/`
- `/tmp/xpm-realexample.conf`
- `/tmp/xpm-real-db-xrepo/`
- `/tmp/xpm-real-cache-xrepo/`
- `/tmp/xpm-realexample-xrepo.conf`

## Notes

- This validates packaging and repository metadata generation with `xpkg`.
- This also validates `xpm sync` against static `file://` repo layouts, including `x-repo`.
- Current `xpm` implementation only syncs `.db`/`.files` mirrors. Package download/install flow is part of Phase 7.
- To use metadata-only repos with package artifacts in external hosting (for example GitHub Releases), package fetch URL composition must be implemented in `xpm` install/fetch flow.
- Full install/remove transaction testing in `xpm` still depends on Phase 7 implementation.
- Before publishing upstream, update any remaining `xscriptor` links in docs/workflows to `equislinux` where applicable.
