# Benchmarks — xpkg vs makepkg

The comparative benchmark (roadmap #57) is executable: it builds the **same**
PKGBUILD with both tools and asserts both the metrics and, first of all, that
the packaged payload is identical (same files, same SHA-256 per file).

```bash
# Skips cleanly when makepkg/fakeroot are not installed.
cargo test -p xpkg --test bench_makepkg -- --nocapture
```

## Methodology

- Fixture: one PKGBUILD creating 1500 deterministic text files plus one
  executable script (`1501` payload files), no compiler and no network.
- `makepkg --nodeps --noconfirm --force` with the system `makepkg.conf`.
- `xpkg build --pkgbuild --no-check` with `compress = "zstd"`,
  `compress_level = 19`.
- Wall-clock includes the whole tool invocation (process start, recipe parse,
  `package()`, metadata generation, compression, archive write).
- Both packages are then fully decompressed and walked: every path + content
  hash must match. The benchmark fails if the payloads differ.
- The test binary under `cargo test` is the **debug (unoptimized) profile**;
  release builds are faster, so the xpkg numbers below are conservative.

## Recorded results

Environment: Arch Linux, 2026-10-08, pacman/makepkg 7.1.0, xpkg debug build.

| Tool | Wall time | Package size |
|------|-----------|--------------|
| makepkg | 17.13 s | 88 332 B |
| xpkg | 1.67 s | 60 666 B |
| ratio (xpkg/makepkg) | 0.10x | 0.69x |

Payload equality: 1501/1501 files identical.

## Reading the numbers

- The wall-time gap is dominated by makepkg's shell-side `.MTREE` generation
  (per-file `md5sum`/`sha256sum` loops) and its `--ultra -20` zstd settings;
  xpkg generates metadata in Rust.
- xpkg's smaller archive here comes from metadata layout (makepkg stores a
  gzip-wrapped `.MTREE` with per-file checksums) and from zstd window/chunk
  differences; sizes are configuration-dependent, not a general claim.
- Absolute numbers vary with the machine. The test bounds are intentionally
  generous (time < 10x makepkg + 5 s, size < 2x) to stay green on noisy CI
  runners; the printed output is the authoritative measurement.
