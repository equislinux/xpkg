# Roadmap — xpkg Package Builder

> Rust-based package building tool for the X distribution — the developer companion to xpm.

## Current Status

Phases 0–3 complete — Cargo workspace scaffolded, CLI with 8 subcommands,
TOML configuration parser, XBUILD/PKGBUILD parsers, recipe validation,
srcinfo generator, `xpkg new`, HTTP downloader with retries, SHA-256/512
checksum verification, archive extraction (tar.gz/xz/bz2/zst, zip), Git
clone support, and source caching are all implemented and tested (72 unit
tests passing). Documented in `docs/` (CLI, XBUILD, SOURCES).
Next step: Phase 4 (build engine — prepare/build/check/package pipeline).

---

## Phase 0 · Project Scaffolding <!-- phase:phase-0:scaffolding -->

- [x] Initialize Rust crate with cargo init (#1)
- [x] Configure Cargo workspace — xpkg (binary) and xpkg-core (library) (#2)
- [x] Add linter and formatter configuration — clippy.toml and rustfmt.toml (#3)
- [x] Set up CI pipeline — GitHub Actions for build, test, clippy, fmt (#4)
- [x] Add license and crate metadata — GPL-3.0-or-later, Cargo.toml fields (#5)
- [x] Create initial README with project overview (#6)

## Phase 1 · CLI and Configuration <!-- phase:phase-1:cli -->

- [x] Implement CLI interface with clap — build, lint, new, srcinfo, info, verify, repo-add, repo-remove subcommands (#7)
- [x] Implement configuration parser — ~/.config/xpkg/xpkg.conf TOML format (#8)
- [x] Implement main.rs orchestration — logging, config loading, subcommand dispatch (#9)
- [x] Implement global flags — verbose, no-confirm, no-color, builddir, outdir (#10)
- [x] Implement xpkg new subcommand — generate XBUILD template for a given package name (#11)
- [x] Define CLI reference documentation — document all commands, flags, and usage patterns (#12)

## Phase 2 · Build Recipe Format <!-- phase:phase-2:recipes -->

- [x] Define XBUILD specification — TOML-based build recipe format (#13)
- [x] Define package section — name, version, release, description, url, license, arch (#67)
- [x] Define dependencies section — depends, makedepends, checkdepends, optdepends (#68)
- [x] Define source section — urls, sha256sums, sha512sums, patches (#69)
- [x] Define build section — prepare, build, check, package functions as multiline strings (#70)
- [x] Implement XBUILD parser — deserialize TOML into Recipe struct with validation (#14)
- [x] Implement PKGBUILD parser — parse Arch Linux PKGBUILD bash scripts for compatibility (#15)
- [x] Extract variables — pkgname, pkgver, pkgrel, depends, makedepends, source, sha256sums (#71)
- [x] Extract functions — prepare(), build(), check(), package() (#72)
- [x] Implement recipe validation — check required fields, verify arch values, validate URLs (#16)
- [x] Implement srcinfo generator — produce .SRCINFO-equivalent from parsed recipe (#17)
- [x] Write recipe parser test suite — valid, invalid, edge-case XBUILD and PKGBUILD files (#18)

## Phase 3 · Source Management <!-- phase:phase-3:sources -->

- [x] Implement source downloader — HTTP/HTTPS download with progress, retries, and resume (#19)
- [x] Implement checksum verification — SHA-256 and SHA-512 validation of downloaded sources (#20)
- [x] Implement source extraction — tar.gz, tar.xz, tar.bz2, tar.zst, zip archive handling (#21)
- [x] Implement Git source support — clone, checkout specific tags, commits, or branches (#22)
- [x] Implement source caching — avoid re-downloading unchanged sources (#23)
- [x] Write source management test suite — download, verify, extract, and cache tests (#24)

## Phase 4 · Build Engine <!-- phase:phase-4:build-engine -->

- [x] Implement build orchestration — prepare → build → check → package pipeline (#25)
- [x] Implement fakeroot environment — build without real root privileges (#26)
- [x] Implement environment variables — PKGDIR, SRCDIR, BUILDDIR, MAKEFLAGS, CFLAGS, CXXFLAGS (#27)
- [x] Implement build script execution — run shell commands from recipe build/package sections (#28)
- [x] Implement build logging — capture stdout/stderr with timestamps (#29)
- [x] Implement build isolation — clean builddir per package, prevent host contamination (#30)
- [x] Write build engine test suite — end-to-end build from recipe to installed files (#31)

## Phase 5 · Package Metadata Generation <!-- phase:phase-5:metadata -->

- [x] Implement .PKGINFO generator — name, version, description, dependencies, provides, conflicts, size (#32)
- [x] Implement .BUILDINFO generator — build environment, packager, builddate, installed packages (#33)
- [x] Implement .MTREE generator — file hashes, permissions, ownership, symlinks for integrity verification (#34)
- [x] Implement .INSTALL script support — pre_install, post_install, pre_upgrade, post_upgrade, pre_remove, post_remove (#35)
- [x] Write metadata generation test suite — validate generated files against specification (#36)

## Phase 6 · Package Archive Creation <!-- phase:phase-6:archives -->

- [x] Implement .xp archive builder — tar.zst creation with metadata files and package content (#37)
  - [x] Pack .PKGINFO, .BUILDINFO, .MTREE at archive root
  - [x] Pack file tree with correct permissions and ownership
  - [x] Configure zstd compression level from config
- [x] Implement package signing — OpenPGP detached signatures (.sig) via sequoia-openpgp (#38)
- [x] Implement strip binaries — optional ELF binary stripping to reduce package size (#39)
- [x] Write archive creation test suite — round-trip build, extract, and verify tests (#40)

## Phase 7 · Package Linting <!-- phase:phase-7:linting -->

- [x] Implement linting framework — pluggable rule engine with severity levels (error, warning, info) (#41)
- [x] Implement dependency checks — verify all ELF dependencies are declared in depends (#42)
- [x] Implement permission checks — flag world-writable files, incorrect ownership, suid/sgid (#43)
- [x] Implement path checks — detect files in non-standard directories (/usr/local, /opt misuse) (#44)
- [x] Implement metadata checks — validate .PKGINFO completeness and field correctness (#45)
- [x] Implement ELF analysis — check for missing RPATH, unneeded TEXTREL, stack protector (#46)
- [x] Implement linting reports — human-readable and machine-parseable output formats (#47)
- [x] Write linting test suite — packages with known issues for each lint rule (#48)

## Phase 8 · Repository Management <!-- phase:phase-8:repo-tools -->

- [x] Implement repo-add subcommand — add packages to a repository database (.db.tar.zst) (#49)
  - [x] Create desc and depends entries from package metadata
  - [x] Update existing entries on version upgrade
  - [x] Sign repository database if configured
- [x] Implement repo-remove subcommand — remove packages from repository database (#50)
- [x] Implement repo database format — compatible with ALPM repo-db format for xpm consumption (#51)
- [x] Implement GitHub Pages deployment helper — generate static repo structure for hosting (#52)
- [x] Write repository management test suite — add, remove, update, and verify repo databases (#53)

## Phase 9 · Integration and Hardening <!-- phase:phase-9:integration -->

- [x] Implement xpkg verify subcommand — validate .xp package integrity and signatures (#54)
- [x] Implement xpkg info subcommand — display metadata from a .xp archive without installing (#55)
- [x] Integration tests with xpm — build packages with xpkg and install with xpm end-to-end (#56)
- [ ] Run comparative benchmarks vs makepkg — build time, package size, and compression performance (#57)
- [x] Complete test suite — unit, integration, and edge-case coverage (#58)
- [x] Audit error handling — corrupt sources, disk full, interrupted builds, missing dependencies (#59)

## Phase 10 · Future Goals — Post v1.0 <!-- phase:phase-10:future -->

- [ ] Implement split packages — multiple packages from a single XBUILD recipe (#60)
- [ ] Implement cross-compilation support — build for different architectures (#61)
- [ ] Implement clean chroot builds — isolated build environment using namespaces (#62)
- [ ] Implement batch builds — build multiple packages in dependency order (#63)
- [ ] Implement AUR-like helper integration — fetch and build from community recipes (#64)
- [ ] Implement VCS package support — automatic version detection for git/svn/hg sources (#65)
- [ ] Implement translations — multi-language support based on system locale (#66)

---

## Phase Diagram

```mermaid
gantt
    title xpkg Roadmap
    dateFormat  YYYY-MM
    axisFormat  %b %Y

    section Foundation
    Scaffolding            :f0, 2026-03, 2w
    CLI and configuration  :f1, after f0, 3w

    section Recipe and Sources
    Build recipe format    :f2, after f1, 4w
    Source management      :f3, after f2, 3w

    section Build Pipeline
    Build engine           :f4, after f3, 5w
    Metadata generation    :f5, after f4, 3w
    Archive creation       :f6, after f5, 3w

    section Quality
    Package linting        :f7, after f6, 3w

    section Distribution
    Repository management  :f8, after f7, 4w

    section Hardening
    Integration and tests  :f9, after f8, 4w

    section Post v1.0
    Future goals           :f10, after f9, 8w
```

---

> **Versioning convention:**
> - `v0.1.0` — Phases 0–1 complete (functional CLI with configuration)
> - `v0.3.0` — Phases 2–3 complete (recipe parsing and source management)
> - `v0.5.0` — Phases 4–6 complete (build engine and archive creation)
> - `v0.7.0` — Phase 7 complete (linting framework)
> - `v0.9.0` — Phases 8–9 complete (repository tooling and integration)
> - `v1.0.0` — Benchmarked, tested, production-ready

Nota: ya no hay sincronizacion roadmap->issues (workflow eliminado).
