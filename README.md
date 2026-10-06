<h1 align="center">xpkg</h1>

<p align="center">
  <strong>Package builder for the X</strong><br>
  Build, lint, sign, and publish <code>.xp</code> packages — the developer companion to
  <a href="https://github.com/equislinux/xpm">xpm</a>.
</p>

<p align="center">
    <img alt="License" src="https://img.shields.io/badge/license-GPL--3.0--or--later-blue.svg">
    <img alt="Rust" src="https://img.shields.io/badge/Rust-000000?logo=rust&logoColor=fff">
    <img alt="Platform" src="https://img.shields.io/badge/Platform-Linux-FCC624?logo=linux&logoColor=000">
    <img alt="Version" src="https://img.shields.io/github/v/release/equislinux/xpkg?include_prereleases&label=version">
    <img alt="CI" src="https://img.shields.io/github/actions/workflow/status/equislinux/xpkg/ci.yml?branch=main&label=CI&logo=github">
</p>

---

## Overview

`xpkg` reads build recipes (**XBUILD** or **PKGBUILD** files), fetches sources,
compiles software in an isolated environment, and produces `.xp` packages ready
for installation with `xpm`. Think of it as the `makepkg` + `repo-add` +
`namcap` equivalent for the X ecosystem — written entirely in Rust.

### Highlights

| Feature | Description |
|---------|-------------|
| **Pure Rust** | Zero C dependencies — consistent with the xpm ecosystem |
| **XBUILD format** | Declarative TOML-based recipes as a modern alternative to PKGBUILD |
| **PKGBUILD compat** | Seamlessly build from Arch Linux PKGBUILD files |
| **Fakeroot builds** | Isolated packaging without real root privileges (unshare / fakeroot / tar-rewrite) |
| **Package signing** | OpenPGP detached signatures via sequoia-openpgp (pure Rust) |
| **Linting** | Automated quality checks: dependencies, permissions, paths, metadata, ELF analysis |
| **Repository tools** | Create and manage ALPM-compatible package databases for `xpm` |
| **Source management** | HTTP download with retries, SHA-256/512 verification, Git clone, local cache |

## Quick Start

```bash
# 1. Install xpkg
git clone https://github.com/equislinux/xpkg.git
cd xpkg
cargo build --release
sudo install -Dm755 target/release/xpkg /usr/local/bin/xpkg

# 2. Create a new package recipe
xpkg new hello
cd hello
# Edit the XBUILD file with your package details

# 3. Build the package
xpkg build

# 4. Inspect the result
xpkg info hello-1.0.0-1-x86_64.xp
xpkg info hello-1.0.0-1-x86_64.xp --files

# 5. Lint for quality issues
xpkg lint hello-1.0.0-1-x86_64.xp

# 6. Install with xpm
sudo xpm install hello-1.0.0-1-x86_64.xp
```

## Commands

| Command | Description |
|---------|-------------|
| `xpkg build` | Build a `.xp` package from an XBUILD or PKGBUILD recipe |
| `xpkg lint <pkg>` | Run quality checks on a built package |
| `xpkg info <pkg>` | Display package metadata (supports `--files` and `--json`) |
| `xpkg verify <pkg>` | Verify package integrity and OpenPGP signature |
| `xpkg new <name>` | Generate a new XBUILD template |
| `xpkg srcinfo` | Generate .SRCINFO-style output from an XBUILD |
| `xpkg repo-add <db> <pkg>` | Add a package to a repository database |
| `xpkg repo-remove <db> <name>` | Remove a package from a repository database |

### Global Flags

| Flag | Description |
|------|-------------|
| `-c, --config <PATH>` | Custom configuration file |
| `-v, --verbose` | Increase verbosity (`-v`, `-vv`, `-vvv`) |
| `--no-confirm` | Skip confirmation prompts |
| `--no-color` | Disable colored output |

See the [CLI Reference](docs/CLI.md) for full details on every command and flag.

## XBUILD Format

XBUILD is a TOML-based build recipe — structured, type-safe, and easy to parse:

```toml
[package]
name = "hello"
version = "2.12"
release = 1
description = "GNU Hello — the friendly greeter"
url = "https://www.gnu.org/software/hello/"
license = ["GPL-3.0-or-later"]
arch = ["x86_64"]

[dependencies]
depends = ["glibc"]
makedepends = ["gcc", "make"]

[source]
urls = ["https://ftp.gnu.org/gnu/hello/hello-2.12.tar.gz"]
sha256sums = ["cf04af86dc085268c5f4470fbae49b18afbc221b78096aab842d934a76bad0ab"]

[build]
build = """
cd hello-2.12
./configure --prefix=/usr
make
"""

package = """
cd hello-2.12
make DESTDIR=$PKGDIR install
"""
```

See the full [XBUILD Specification](docs/XBUILD.md).

## Configuration

Configuration file: `~/.config/xpkg/xpkg.conf` (TOML). See
[`etc/xpkg.conf.example`](etc/xpkg.conf.example) for all options.

```toml
[options]
builddir = "/tmp/xpkg-build"
outdir = "."
strip_binaries = true
compress = "zstd"       # zstd | gzip | xz
compress_level = 19

[environment]
makeflags = "-j$(nproc)"
cflags = "-march=x86-64 -O2 -pipe"
cxxflags = "-march=x86-64 -O2 -pipe"
```

## Documentation

| Document | Description |
|----------|-------------|
| [Installation Guide](docs/INSTALLATION.md) | Build from source, requirements, setup |
| [Packaging Guide](docs/PACKAGING-GUIDE.md) | Step-by-step tutorial to create your first package |
| [CLI Reference](docs/CLI.md) | Complete command and flag reference |
| [XBUILD Specification](docs/XBUILD.md) | The TOML recipe format in detail |
| [Source Management](docs/SOURCES.md) | Download, verify, extract, and cache sources |
| [Package Signing](docs/SIGNING.md) | Key generation, signing, and verification |
| [Repository Management](docs/REPOSITORY.md) | Create and host package repositories |
| [Linting Rules](docs/LINTING.md) | All lint checks and their severity levels |

## Project Structure

```text
xpkg/
├── crates/
│   ├── xpkg/               # Binary crate — CLI frontend
│   │   └── src/
│   │       ├── main.rs      # Entry point, dispatch
│   │       └── cli.rs       # clap CLI definitions
│   └── xpkg-core/           # Library crate — core logic
│       └── src/
│           ├── config.rs     # Configuration parser
│           ├── error.rs      # Error types (thiserror)
│           ├── recipe/       # XBUILD + PKGBUILD parsers
│           ├── source/       # Download, checksum, extraction, cache
│           ├── builder/      # Build pipeline + fakeroot
│           ├── metadata/     # .PKGINFO, .BUILDINFO, .MTREE, .INSTALL
│           ├── archive/      # .xp archive creation + ELF stripping
│           ├── lint/         # Linting framework + rules
│           ├── signing/      # OpenPGP signing (sequoia-openpgp)
│           └── repo/         # Repository database management
├── docs/                     # User documentation
├── etc/                      # Example configuration
└── ROADMAP.md                # Development roadmap
```

## Relationship with xpm

| Tool | Role | Analogy |
|------|------|---------|
| **xpm** | Package manager — install, remove, upgrade, resolve deps | `pacman` |
| **xpkg** | Package builder — compile, package, lint, manage repos | `makepkg` + `repo-add` + `namcap` |

`xpkg` produces `.xp` packages that `xpm` installs. They share the same
package format and metadata structures but are independent binaries.

## Roadmap

See [ROADMAP.md](ROADMAP.md) for the full development plan.

| Version | Milestone |
|---------|-----------|
| `v0.1.0` | CLI with configuration |
| `v0.3.0` | Recipe parsing and source management |
| `v0.5.0` | Build engine, metadata, and archives |
| `v0.7.0` | Package linting |
| `v0.9.0` | Repository tooling, signing, and integration |
| `v1.0.0` | Benchmarked, tested, production-ready |

## License

GPL-3.0-or-later — see [LICENSE](LICENSE).

- Documentation: https://github.com/equislinux/wiki
