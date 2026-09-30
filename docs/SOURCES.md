# Source Management

How xpkg downloads, verifies, extracts, and caches package sources.

---

## Overview

Before building a package, xpkg fetches every source declared in the
recipe's `[source]` section. The source manager handles HTTP/HTTPS
downloads, Git clones, checksum verification, archive extraction, and
local caching to avoid redundant downloads.

---

## Source URLs

Sources are declared as an array of URLs in the recipe:

```toml
[source]
urls = [
    "https://example.com/releases/hello-2.12.tar.gz",
    "https://example.com/extra-data.zip",
]
```

### HTTP/HTTPS Sources

Standard archive downloads. The filename is derived from the last
component of the URL path. Supported archive formats are extracted
automatically after download.

### Git Sources

Git repositories are cloned using the system `git` command. A URL is
recognized as a Git source if it matches any of:

| Pattern | Example |
|---------|---------|
| `git://` prefix | `git://github.com/user/repo.git` |
| `git+https://` prefix | `git+https://github.com/user/repo.git` |
| `git+http://` prefix | `git+http://host/repo` |
| `.git` suffix | `https://github.com/user/repo.git` |

The `git+` prefix is stripped before passing to `git clone`.

#### Pinned references

A Git URL may select an exact revision with a fragment:

| Fragment | Behavior |
|----------|----------|
| `#commit=<sha>` | Full clone followed by `git checkout <sha>` |
| `#tag=<tag>` | `git clone --depth 1 --branch <tag>` |
| `#branch=<branch>` | `git clone --depth 1 --branch <branch>` |

`#tag=` and `#commit=` are immutable pins; `#branch=` is recorded but can
move. When building, the exact commit of the first pinned Git source is
resolved (from the clone's `HEAD`, or with `git ls-remote` for tags and
branches when no local clone exists) and stored in `.BUILDINFO` as
`x:source_commit`.

---

## Checksum Verification

After downloading a source, xpkg verifies its integrity using SHA-256
and/or SHA-512 checksums declared in the recipe:

```toml
[source]
urls = ["https://example.com/hello-2.12.tar.gz"]
sha256sums = ["a948904f2f0f479b8f8564e9d7563891e5c23fd4f3a9b62c1b9e8f05e6c84d73"]
# sha512sums = ["..."]
```

- Each checksum corresponds to the source URL at the same index.
- Use `"SKIP"` to bypass verification for a specific source.
- Both SHA-256 and SHA-512 are supported simultaneously.
- Checksums are compared case-insensitively.

If a checksum does not match, the build is aborted with a
`ChecksumMismatch` error.

---

## Archive Extraction

Downloaded archives are automatically extracted into the source
directory. Format detection is based on the file extension:

| Extension | Format |
|-----------|--------|
| `.tar.gz`, `.tgz` | gzip-compressed tar |
| `.tar.xz`, `.txz` | xz-compressed tar |
| `.tar.bz2`, `.tbz2` | bzip2-compressed tar |
| `.tar.zst`, `.tzst` | zstd-compressed tar |
| `.zip` | ZIP archive |

Files that do not match a recognized extension are kept as-is (e.g.
patch files, standalone binaries).

---

## Source Caching

To avoid re-downloading the same source across builds, xpkg maintains a
local cache:

| Setting | Value |
|---------|-------|
| Default location | `$XDG_CACHE_HOME/xpkg/sources/` or `~/.cache/xpkg/sources/` |
| Cache key | Truncated SHA-256 hash of the URL (16 hex chars) |
| File naming | `<key>.<original-extension>` |

**Cache behavior:**

1. Before downloading, check if the URL is already cached.
2. On cache hit, copy the cached file to the source directory.
3. On cache miss, download and store a copy in the cache.
4. Cache storage failures are non-fatal — the build continues.

Git sources are **not** cached (they are always cloned fresh).

---

## Fetch Workflow

For each source URL in the recipe, in order:

```
1. Is it a git URL?
   ├─ Yes → parse #commit=/#tag=/#branch=
   │        ├─ commit → git clone + git checkout
   │        ├─ tag/branch → git clone --depth 1 --branch
   │        └─ none → git clone (default branch)
   └─ No  → continue
2. Is it cached?
   ├─ Yes → copy from cache to srcdir/
   └─ No  → download via HTTP/HTTPS
             └─ store in cache (best-effort)
3. Verify SHA-256 checksum (if declared)
4. Verify SHA-512 checksum (if declared)
5. Is it a recognized archive?
   ├─ Yes → extract into srcdir/
   └─ No  → keep as-is
```

---

## Download Options

| Option | Default | Description |
|--------|---------|-------------|
| Retries | 3 | Number of download attempts on failure |
| Connect timeout | 30 s | TCP connection timeout |
| Read timeout | 300 s | Data transfer timeout |

Retries occur on any transient failure (network error, timeout). HTTP
4xx/5xx responses are treated as errors.

---

## API Usage (Library)

The source module is available through `xpkg_core::source`:

```rust
use xpkg_core::source::{SourceManager, SourceCache};

let cache_dir = SourceCache::default_dir();
let manager = SourceManager::new(cache_dir);

// Fetch all sources from a parsed recipe.
let srcdir = std::path::Path::new("/tmp/xpkg-build/src");
let paths = manager.fetch_sources(&recipe, srcdir)?;
```

Individual components can also be used directly:

```rust
use xpkg_core::source::{
    download_file, DownloadOptions,
    verify_checksum, ChecksumAlgo,
    extract_archive,
    is_git_url, git_clone,
};
```
