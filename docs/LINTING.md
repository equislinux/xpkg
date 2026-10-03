# Linting Rules

Complete reference for all `xpkg lint` checks.

---

## Overview

The linter analyzes built `.xp` packages for common issues before
distribution. Each rule has an ID, a severity level, and a description.

```bash
xpkg lint mypackage-1.0-1-x86_64.xp
xpkg lint mypackage-1.0-1-x86_64.xp --strict   # Treat warnings as errors
```

Most rules operate on a built package archive. The source checks run on the
recipe instead, before a package exists, and are reported as warnings at the
start of `xpkg build` (they never stop the build).

---

## Severity Levels

| Level | Marker | Behavior |
|-------|--------|----------|
| **Error** | `[error]` | Always causes lint failure (exit code 1) |
| **Warning** | `[warning]` | Causes failure only with `--strict` |
| **Info** | `[info]` | Informational — never causes failure |

---

## Permission Checks

| Rule ID | Severity | Description |
|---------|----------|-------------|
| `permissions-world-writable` | Error | File is world-writable (`o+w`) — security risk |
| `permissions-world-writable-dir` | Warning | Directory is world-writable without sticky bit |
| `permissions-suid` | Warning | File has the SUID bit set |
| `permissions-sgid` | Warning | File has the SGID bit set (non-directory) |

**Why it matters:** World-writable files allow any user to modify them,
which is a security vulnerability. SUID/SGID binaries run with elevated
privileges and should be reviewed carefully.

---

## Path Checks

| Rule ID | Severity | Description |
|---------|----------|-------------|
| `paths-forbidden-directory` | Error | Package installs files to a forbidden location |
| `paths-non-standard-toplevel` | Warning | Non-standard top-level directory in the package |
| `paths-empty-directory` | Info | Empty directory found |

**Forbidden directories:**

`usr/local`, `var/local`, `home`, `root`, `tmp`, `run`, `dev`, `proc`,
`sys`, `mnt`, `media`

**Allowed top-level directories:**

`usr`, `etc`, `var`, `srv`, `boot`, `opt`

---

## Metadata Checks

| Rule ID | Severity | Description |
|---------|----------|-------------|
| `metadata-missing-field` | Error | Required `.PKGINFO` field is missing |
| `metadata-empty-pkgver` | Error | `pkgver` field is empty |
| `metadata-empty-description` | Warning | Package description is empty |
| `metadata-placeholder-description` | Warning | Description contains `TODO` or `FIXME` |
| `metadata-pkgver-no-release` | Warning | Version string missing release component |

**Required fields:** `pkgname`, `pkgver`, `pkgdesc`, `arch`, `size`

---

## Dependency Checks

| Rule ID | Severity | Description |
|---------|----------|-------------|
| `dependency-needed-library` | Info | ELF binary requires a shared library (`DT_NEEDED`) |

This check lists shared library dependencies found in ELF binaries. It
is informational only — use it to verify that all runtime dependencies
are declared in your XBUILD `depends` array.

---

## ELF Analysis

| Rule ID | Severity | Description |
|---------|----------|-------------|
| `elf-textrel` | Warning | Binary contains text relocations (`TEXTREL`) |
| `elf-rpath` | Warning | Binary has a non-standard `RPATH` |

**TEXTREL:** Text relocations prevent shared library code from being
shared across processes and are a security concern. Fix by compiling with
`-fPIC`.

**RPATH:** Allowed values are empty, `$ORIGIN`, and `$ORIGIN/../lib`.
Non-standard RPATH values may cause the binary to load libraries from
unexpected locations.

---

## Source Checks (recipe-level)

| Rule ID | Severity | Description |
|---------|----------|-------------|
| `source-unpinned` | Warning | Source has no checksum and no pinned Git commit/tag |

A source is considered verifiable when a `sha256sums` or `sha512sums` entry
exists at its index (a literal `"SKIP"` does not count) or when it is a Git
URL pinned to a commit or tag:

```toml
[source]
urls = [
    "https://example.com/foo-1.0.tar.gz",        # needs a checksum
    "git+https://example.com/repo.git#tag=v1.0", # pinned by tag
    "git+https://example.com/repo.git#branch=main", # warning: branch can move
]
sha256sums = ["...", "SKIP", "SKIP"]
```

These checks run at the start of `xpkg build`; they are not part of
`xpkg lint <package>` because a built `.xp` no longer carries source
declarations.

---

## Output Formats

### Human-readable (default)

```
[error] permissions-world-writable: file is world-writable (usr/share/data.txt)
[warning] elf-textrel: binary contains TEXTREL (usr/lib/libfoo.so)
[info] dependency-needed-library: needs libz.so.1 (usr/bin/foo)

Summary: 1 error, 1 warning, 1 info
```

### JSON (`--json` planned)

```json
{
  "errors": 1,
  "warnings": 1,
  "info": 1,
  "issues": [
    {
      "rule": "permissions-world-writable",
      "severity": "error",
      "path": "usr/share/data.txt",
      "message": "file is world-writable"
    }
  ]
}
```

---

## Summary

| Category | Rules | Errors | Warnings | Info |
|----------|-------|--------|----------|------|
| Permissions | 4 | 1 | 3 | 0 |
| Paths | 3 | 1 | 1 | 1 |
| Metadata | 5 | 2 | 3 | 0 |
| Dependencies | 1 | 0 | 0 | 1 |
| ELF | 2 | 0 | 2 | 0 |
| Sources (recipe-level) | 1 | 0 | 1 | 0 |
| **Total** | **16** | **4** | **10** | **2** |
