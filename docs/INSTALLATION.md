# Installation Guide

How to build and install xpkg from source.

---

## Requirements

| Dependency | Version | Purpose |
|------------|---------|---------|
| **Rust** | 1.70+ | Compiler (2021 edition) |
| **Cargo** | (bundled with Rust) | Build system |
| **git** | any | Clone the repository and Git source support |

### Optional runtime dependencies

| Tool | Purpose |
|------|---------|
| `fakeroot` | Preferred method for packaging without root (auto-detected) |
| `strip` | ELF binary stripping (from `binutils`) |

> **Note:** xpkg has a 3-layer fakeroot fallback. If `unshare --user` is
> available (Linux ≥ 3.8), it uses kernel namespaces. Otherwise it falls
> back to `fakeroot`, and finally to direct execution with tar header
> rewriting. All three produce packages with correct `uid=0`/`gid=0`.

---

## Build from Source

```bash
# Clone the repository
git clone https://github.com/equislinux/xpkg.git
cd xpkg

# Build in release mode
cargo build --release

# The binary is at target/release/xpkg
./target/release/xpkg --version
```

## Install

```bash
# Install to /usr/local/bin (system-wide)
sudo install -Dm755 target/release/xpkg /usr/local/bin/xpkg

# Or install to ~/.local/bin (user-only, ensure it's in PATH)
install -Dm755 target/release/xpkg ~/.local/bin/xpkg
```

## Configuration Setup

xpkg works out of the box with sensible defaults. To customize, create a
configuration file:

```bash
# Create config directory
mkdir -p ~/.config/xpkg

# Copy the example configuration
cp etc/xpkg.conf.example ~/.config/xpkg/xpkg.conf

# Edit to your preferences
$EDITOR ~/.config/xpkg/xpkg.conf
```

Key settings you may want to adjust:

| Setting | Default | Description |
|---------|---------|-------------|
| `builddir` | `/tmp/xpkg-build` | Where source is compiled |
| `outdir` | `.` | Where `.xp` packages are placed |
| `compress` | `zstd` | Compression method (`zstd`, `gzip`, `xz`) |
| `compress_level` | `19` | Compression level |
| `strip_binaries` | `true` | Strip debug symbols from ELF binaries |
| `makeflags` | `-j$(nproc)` | Flags passed to `make` |

See [`etc/xpkg.conf.example`](../etc/xpkg.conf.example) for the full
reference.

---

## Verify the Installation

```bash
# Check the version
xpkg --version

# Generate a template to confirm everything works
xpkg new test-package
cat test-package/XBUILD
rm -rf test-package
```

---

## Updating

```bash
cd xpkg
git pull
cargo build --release
sudo install -Dm755 target/release/xpkg /usr/local/bin/xpkg
```

---

## Uninstall

```bash
sudo rm /usr/local/bin/xpkg
# Optional: remove configuration
rm -rf ~/.config/xpkg
```
