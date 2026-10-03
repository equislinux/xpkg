# xpkg - Usage

How to install and drive `xpkg`: installation, the command surface, flags,
configuration, and the typical packaging workflows.

Related documents in this folder:

- [01 - Overview and status](01-overview-and-status.md)
- [03 - Architecture](03-architecture.md)
- [04 - Future integration](04-future-integration.md)

For full details on every command and flag, see the existing flat docs:
[CLI Reference](../CLI.md), [XBUILD Specification](../XBUILD.md),
[Packaging Guide](../PACKAGING-GUIDE.md), [Installation Guide](../INSTALLATION.md),
[Source Management](../SOURCES.md), [Package Signing](../SIGNING.md),
[Repository Management](../REPOSITORY.md) and [Linting Rules](../LINTING.md).

---

## Installation

Requirements: Rust 1.70+ (2021 edition), Cargo, and git. Optional runtime
tools: `fakeroot` (preferred for rootless packaging, auto-detected) and
`strip` (ELF stripping, from binutils).

```bash
git clone https://github.com/xlnux/xpkg.git
cd xpkg
cargo build --release
sudo install -Dm755 target/release/xpkg /usr/local/bin/xpkg
```

Configuration defaults work out of the box. To customise, copy
`etc/xpkg.conf.example` to `~/.config/xpkg/xpkg.conf` and edit it.

## Commands

| Command | Description |
|---------|-------------|
| `xpkg build` | Build a `.xp` package from an XBUILD or PKGBUILD recipe |
| `xpkg lint <pkg>` | Run quality checks on a built package |
| `xpkg info <pkg>` | Display package metadata (supports `--files` and `--json`) |
| `xpkg verify <pkg>` | Verify package integrity and OpenPGP signature |
| `xpkg new <name>` | Generate a new XBUILD template |
| `xpkg srcinfo` | Generate `.SRCINFO`-style output from an XBUILD |
| `xpkg repo-add <db> <pkg>` | Add a package to a repository database |
| `xpkg repo-remove <db> <name>` | Remove a package from a repository database |

### Global flags

| Flag | Short | Description |
|------|-------|-------------|
| `--config <PATH>` | `-c` | Custom configuration file (default `~/.config/xpkg/xpkg.conf`) |
| `--verbose` | `-v` | Increase verbosity (`-v`, `-vv`, `-vvv`) |
| `--no-confirm` | — | Skip confirmation prompts |
| `--no-color` | — | Disable colored output |

### build - Build a Package

Runs the full pipeline: parse recipe, fetch sources, prepare, build, check,
package, strip, archive, sign.

| Flag | Short | Description |
|------|-------|-------------|
| `--file <PATH>` | `-f` | Recipe file (default `./XBUILD`) |
| `--pkgbuild` | — | Parse the recipe as a PKGBUILD instead of XBUILD |
| `--builddir <PATH>` | `-d` | Build directory (overrides config) |
| `--outdir <PATH>` | `-o` | Output directory for the `.xp` (overrides config) |
| `--no-check` | — | Skip the `check()` phase |
| `--sign` | — | Sign the package after building (requires `sign_key`) |

```bash
xpkg build                             # Build from ./XBUILD
xpkg build -f path/to/XBUILD          # Build from a specific file
xpkg build --pkgbuild -f ./PKGBUILD   # Build from a PKGBUILD
xpkg build --no-check -o ./out        # Skip tests, output to ./out
xpkg build --sign                      # Build and sign the package
```

### lint - Lint a Package Archive

Extracts the archive, reads `.PKGINFO`, and runs all lint rules.

| Argument/Flag | Description |
|---------------|-------------|
| `PACKAGE` | Path to the `.xp` package archive |
| `--strict` | Treat lint warnings as errors (exit code 1) |

```bash
xpkg lint hello-2.12-1-x86_64.xp
xpkg lint hello-2.12-1-x86_64.xp --strict
```

Lint categories: permissions, paths, metadata, dependencies, and ELF
analysis. See [Linting Rules](../LINTING.md) for the complete list.

### info - Display Package Metadata

Inspect a `.xp` archive without installing it.

| Argument/Flag | Short | Description |
|---------------|-------|-------------|
| `PACKAGE` | — | Path to the `.xp` package archive |
| `--files` | `-l` | List all files contained in the package |
| `--json` | — | Output metadata as JSON (machine-readable) |

```bash
xpkg info hello-2.12-1-x86_64.xp           # Human-readable metadata
xpkg info hello-2.12-1-x86_64.xp --files   # Include file listing
xpkg info hello-2.12-1-x86_64.xp --json    # JSON output for scripting
```

### verify - Verify Package Integrity

Verifies the OpenPGP detached signature of a `.xp` package. Looks for a
`.xp.sig` file alongside the package.

| Argument/Flag | Short | Description |
|---------------|-------|-------------|
| `PACKAGE` | — | Path to the `.xp` package archive |
| `--key <PATH>` | `-k` | Public key or keyring file |

```bash
xpkg verify hello-2.12-1-x86_64.xp --key packager.pub
xpkg verify hello-2.12-1-x86_64.xp -k /etc/xpkg/trusted.gpg
```

### new - Create an XBUILD Template

| Argument/Flag | Short | Description |
|---------------|-------|-------------|
| `PKGNAME` | — | Name of the package |
| `--outdir <PATH>` | `-o` | Output directory (default `./<PKGNAME>/`) |

```bash
xpkg new hello                  # Creates hello/XBUILD
xpkg new mylib -o packages/     # Creates packages/XBUILD
```

### srcinfo - Generate Source Info

Produces `.SRCINFO`-style output from a parsed and validated XBUILD.

| Flag | Short | Description |
|------|-------|-------------|
| `--file <PATH>` | `-f` | XBUILD file (default `./XBUILD`) |

```bash
xpkg srcinfo                     # Print to stdout
xpkg srcinfo > .SRCINFO          # Write output to .SRCINFO
```

### repo-add / repo-remove - Repository management

Manage an ALPM-compatible database (`myrepo.db.tar.zst` by default; also
`.db.tar.gz` and `.db.tar.xz`, auto-detected from the extension). The
database is created automatically on first add.

```bash
xpkg repo-add myrepo.db.tar.zst hello-2.12-1-x86_64.xp
xpkg repo-add myrepo.db.tar.zst hello-2.12-1-x86_64.xp --sign

xpkg repo-remove myrepo.db.tar.zst hello
xpkg repo-remove myrepo.db.tar.zst hello --sign
```

Adding an existing package name replaces the entry with the new version.
Removing also syncs `history.json` when present (stale versions and packages no
longer in the database are dropped).
See [Repository Management](../REPOSITORY.md) for hosting instructions.

## Exit codes

| Code | Meaning |
|------|---------|
| `0` | Success |
| `1` | General error (invalid recipe, build failure, lint errors, bad signature) |
| `2` | Invalid usage (missing arguments, unknown flags) |

## Environment variables

| Variable | Description |
|----------|-------------|
| `RUST_LOG` | Override tracing log level filter (e.g. `RUST_LOG=debug`) |

During builds, these variables are set for the build scripts:

| Variable | Description |
|----------|-------------|
| `PKGDIR` | Destination directory for installed files |
| `SRCDIR` | Directory containing extracted source files |
| `BUILDDIR` | Top-level build directory |
| `MAKEFLAGS` | Make flags from config |
| `CFLAGS` | C compiler flags from config |
| `CXXFLAGS` | C++ compiler flags from config |
| `LDFLAGS` | Linker flags from config |

## Configuration

Configuration file: `~/.config/xpkg/xpkg.conf` (TOML), or any path passed
via `--config`. Key sections:

- `[options]` - builddir, outdir, sign, sign_key, compress method/level,
  strip_binaries
- `[environment]` - MAKEFLAGS, CFLAGS, CXXFLAGS, LDFLAGS
- `[lint]` - enable/disable linting, strict mode

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

See `etc/xpkg.conf.example` for every option.

## Typical workflows

1. **New package** - `xpkg new hello`, edit `hello/XBUILD`, `xpkg build`,
   inspect with `xpkg info`, quality-check with `xpkg lint`, install with
   `sudo xpm install hello-...-x86_64.xp`.
2. **Arch compatibility** - keep an existing PKGBUILD and run
   `xpkg build --pkgbuild -f ./PKGBUILD`.
3. **Publishing** - `xpkg repo-add x.db.tar.zst pkg-...-x86_64.xp` inside the
   hosted directory; optionally `--sign` the database.
4. **Signing setup** - export a secret key to `~/.config/xpkg/signing.key`,
   set `sign = true` and `sign_key` in the config, build with `--sign`.
