# XBUILD Specification

**XBUILD** is the native TOML-based build recipe format for xpkg. It
replaces the bash-based PKGBUILD with a structured, declarative format
that is easier to parse, validate, and generate programmatically.

> xpkg also supports parsing legacy **PKGBUILD** files for Arch Linux
> compatibility. See `xpkg build --pkgbuild` in the CLI reference.

---

## File Conventions

- **File name:** `XBUILD` (no extension)
- **Format:** TOML v1.0
- **Encoding:** UTF-8

---

## Sections

An XBUILD file has four top-level sections:

```toml
[package]        # Required — identity and metadata
[dependencies]   # Optional — runtime, build, check, and optional deps
[source]         # Optional — source URLs and checksums
[build]          # Optional — shell scripts for each build phase
```

---

## `[package]` — Identity and Metadata

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | `String` | **yes** | Package name — lowercase, alphanumeric, hyphens only. Must start with a letter. |
| `version` | `String` | **yes** | Upstream version string (e.g. `"2.12"`, `"1.0.0-rc1"`) |
| `release` | `Integer` | no | Package release number (default: `1`). Increment for packaging changes only. |
| `description` | `String` | no | Short human-readable description of the package |
| `url` | `String` | no | Upstream project URL |
| `license` | `String[]` | no | SPDX license identifiers (e.g. `["GPL-3.0-or-later"]`) |
| `arch` | `String[]` | no | Supported architectures: `"x86_64"`, `"aarch64"`, `"any"`, etc. |
| `provides` | `String[]` | no | Virtual package names this package provides |
| `conflicts` | `String[]` | no | Packages this package conflicts with |
| `replaces` | `String[]` | no | Packages this package replaces on upgrade |

**Example:**

```toml
[package]
name = "hello"
version = "2.12"
release = 1
description = "GNU Hello — the friendly greeter"
url = "https://www.gnu.org/software/hello/"
license = ["GPL-3.0-or-later"]
arch = ["x86_64"]
```

### Name Validation Rules

- Must not be empty
- Must start with a lowercase ASCII letter (`a-z`)
- May contain only lowercase letters, digits, hyphens, and underscores
- Maximum length: 128 characters

---

## `[dependencies]` — Dependency Declarations

All fields are optional string arrays. If the section is omitted, all
dependency lists default to empty.

| Field | Type | Description |
|-------|------|-------------|
| `depends` | `String[]` | Runtime dependencies (required at install time) |
| `makedepends` | `String[]` | Build-time dependencies (needed only during build) |
| `checkdepends` | `String[]` | Test/check dependencies (needed only for the `check()` phase) |
| `optdepends` | `String[]` | Optional dependencies with descriptions (format: `"pkg: reason"`) |

**Example:**

```toml
[dependencies]
depends = ["glibc"]
makedepends = ["gcc", "make"]
checkdepends = ["dejagnu"]
optdepends = ["gettext: NLS support"]
```

---

## `[source]` — Source Archives and Integrity

| Field | Type | Description |
|-------|------|-------------|
| `urls` | `String[]` | Source URLs to download. Supports `http://`, `https://`, `ftp://`, `file://` and Git sources (`git://`, `git+https://`, `git+http://`, or any URL ending in `.git`). Variables like `${version}` are **not** expanded by the parser. |
| `sha256sums` | `String[]` | SHA-256 checksums — one per URL in order. Use `"SKIP"` to bypass a check. |
| `sha512sums` | `String[]` | SHA-512 checksums — one per URL in order. Use `"SKIP"` to bypass. |
| `patches` | `String[]` | Patch files to apply during the `prepare()` phase |

When both `sha256sums` and `sha512sums` are provided, both are verified.
The number of checksum entries must match the number of source URLs.

### Pinned Git Sources

A Git URL may pin the revision with a fragment, following the makepkg
convention:

| Fragment | Meaning |
|----------|---------|
| `#commit=<sha>` | Exact commit to check out |
| `#tag=<tag>` | Tag to clone (immutable once published) |
| `#branch=<branch>` | Branch to clone (can move; recorded but not pinned) |

```toml
[source]
urls = [
    "git+https://github.com/user/tool.git#tag=v1.0",
]
```

Pinned sources are recorded in the package `.BUILDINFO` as
`x:source_commit` (the exact resolved commit). A source with neither a
checksum nor a pinned commit/tag triggers the `source-unpinned` lint
warning during `xpkg build`. See [Sources](SOURCES.md) for resolution
details and [Linting](LINTING.md) for the rule.

**Example:**

```toml
[source]
urls = [
    "https://ftp.gnu.org/gnu/hello/hello-2.12.tar.gz",
]
sha256sums = [
    "cf04af86dc085268c5f4470fbae49b18afbc221b78096aab842d934a76bad0ab",
]
```

---

## `[build]` — Build Phase Scripts

Each field is a multiline string containing shell commands executed
during the corresponding build phase. The working directory is the
build directory. The following environment variables are available:

| Variable | Description |
|----------|-------------|
| `$PKGDIR` | Destination directory for installed files (fakeroot) |
| `$SRCDIR` | Directory containing extracted source files |
| `$BUILDDIR` | Top-level build directory |
| `$MAKEFLAGS` | Flags passed to make (from config) |
| `$CFLAGS` | C compiler flags (from config) |
| `$CXXFLAGS` | C++ compiler flags (from config) |
| `$LDFLAGS` | Linker flags (from config) |

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `prepare` | `String` | no | Run before building — apply patches, autoreconf, etc. |
| `build` | `String` | no | Main compilation step — configure and make |
| `check` | `String` | no | Run test suite (skipped with `--no-check`) |
| `package` | `String` | **yes** *(logical)* | Install files into `$PKGDIR` — this is the only phase that actually populates the package |

**Example:**

```toml
[build]
prepare = """
cd hello-2.12
patch -p1 < ../fix-typo.patch
"""

build = """
cd hello-2.12
./configure --prefix=/usr
make
"""

check = """
cd hello-2.12
make check
"""

package = """
cd hello-2.12
make DESTDIR=$PKGDIR install
"""
```

---

## Complete Example

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
checkdepends = ["dejagnu"]
optdepends = ["gettext: NLS support"]

[source]
urls = [
    "https://ftp.gnu.org/gnu/hello/hello-2.12.tar.gz",
]
sha256sums = [
    "cf04af86dc085268c5f4470fbae49b18afbc221b78096aab842d934a76bad0ab",
]

[build]
prepare = """
cd hello-2.12
"""

build = """
cd hello-2.12
./configure --prefix=/usr
make
"""

check = """
cd hello-2.12
make check
"""

package = """
cd hello-2.12
make DESTDIR=$PKGDIR install
"""
```

---

## Comparison with PKGBUILD

| Aspect | XBUILD | PKGBUILD |
|--------|--------|----------|
| Format | TOML | Bash script |
| Parseable | Trivially (standard TOML library) | Requires bash evaluation or regex heuristics |
| Variables | Static fields | Dynamic bash variables and substitution |
| Functions | Multiline strings | Bash functions |
| Validation | Structural (serde + custom checks) | Manual / runtime |
| Compatibility | Native xpkg format | Supported for Arch migration path |

---

## Reproducibility

When the `SOURCE_DATE_EPOCH` environment variable is set to a Unix
timestamp, xpkg uses it for the `builddate` of `.PKGINFO` and `.BUILDINFO`
and for the mtime of every tar entry. Without it, the current time is used.
This makes metadata timestamps stable across machines; it is groundwork for
reproducible builds, not a full guarantee.

```bash
SOURCE_DATE_EPOCH=1700000000 xpkg build
```

The extended provenance fields (`x:recipe_sha256`, `x:source_commit`,
`x:tool_version`) are appended to `.BUILDINFO` and ignored by readers that
do not know them.

---

## Validation Rules

The XBUILD parser applies the following checks:

1. `name` must be non-empty and follow naming rules (see above)
2. `version` must be non-empty
3. `release` must be ≥ 1
4. `arch` values must be one of: `x86_64`, `aarch64`, `i686`, `armv7h`, `any`
5. Source URL schemes must be `http`, `https`, `ftp`, `file`, `git`,
   `git+https`, or `git+http`
6. If `sha256sums` is provided, its length must equal `urls` length
7. If `sha512sums` is provided, its length must equal `urls` length

Validation errors are collected and reported together rather than
failing on the first error.
