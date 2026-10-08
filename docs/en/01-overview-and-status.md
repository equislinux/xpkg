# xpkg - Overview and Status

This is one of four documents describing `xpkg`, the Rust package builder of
the X distribution. It covers what the tool is, its role in the X ecosystem,
and its honest status within the equislinux *reboot* initiative.

Related documents in this folder:

- [02 - Usage](02-usage.md)
- [03 - Architecture](03-architecture.md)
- [04 - Future integration](04-future-integration.md)

---

## What xpkg is

`xpkg` is the **package builder for the X distribution**. It reads build
recipes (**XBUILD** or legacy **PKGBUILD** files), fetches sources, compiles
software in an isolated environment, and produces `.xp` packages ready for
installation with `xpm`. It is the developer companion to `xpm`.

Think of it as the `makepkg` + `repo-add` + `namcap` equivalent for the X
ecosystem, written entirely in Rust.

| Feature | Description |
|---------|-------------|
| Pure Rust | Zero C dependencies - consistent with the xpm ecosystem |
| XBUILD format | Declarative TOML-based recipes, a modern alternative to PKGBUILD |
| PKGBUILD compat | Seamlessly build from Arch Linux PKGBUILD files |
| Fakeroot builds | Isolated packaging without real root privileges (unshare / fakeroot / tar-rewrite) |
| Package signing | OpenPGP detached signatures via sequoia-openpgp (pure Rust) |
| Linting | Automated quality checks: dependencies, permissions, paths, metadata, ELF analysis |
| Repository tools | Create and manage ALPM-compatible package databases for `xpm` |
| Source management | HTTP download with retries, SHA-256/512 verification, Git clone, local cache |

Project metadata: version `0.1.0`, edition 2021, license
GPL-3.0-or-later, org `equislinux`, repository `https://github.com/equislinux/xpkg`.

## Relationship with xpm

| Tool | Role | Analogy |
|------|------|---------|
| **xpm** | Package manager - install, remove, upgrade, resolve deps | `pacman` |
| **xpkg** | Package builder - compile, package, lint, manage repos | `makepkg` + `repo-add` + `namcap` |

`xpkg` produces `.xp` packages that `xpm` installs. Both share the same
package format and metadata structures but are independent binaries.

## Status inside the equislinux reboot

The equislinux *reboot* initiative (see `ROADMAP.md` and `DECISIONS.md` at the
workspace root, outside this repo) reorganised the organisation and set a new
direction for the distribution. Its status for the Rust tooling is:

- The workspace roadmap phase **"Fase 4 - Tooling Rust (xpm/xpkg)"** is
  **POSTPONED**. Per the maintainer decision, xpm/xpkg are **not worked on
  during the reboot**; the tooling is revisited only when the Rust stack is
  required again (SAT resolver, `.xp` repository).
- Packaging of the distribution payload during the reboot is done with
  **PKGBUILD + makepkg** (package `x-scripts`), published to the binary
  repository **`x-repo`**. xpkg is **not currently used** by the reboot flow.
- Decision **ADR-0004** records that xpkg/xpm remain the intended packaging
  and package-management base of the distribution, under a precondition: the
  `xpm` SAT resolver must be wired to `install` and local `.xp` file installs
  must work before depending on `xpm` for system bootstrap.

In short: xpkg is in a **REBOOT PENDING** state. The codebase is functional
and maintained as the future packager, but it is de-prioritised for now and
does not participate in the current reboot delivery flow.

## Maturity of the codebase

The repository's own `ROADMAP.md` reports:

- **"Current Status" section**: phases 0-3 complete - Cargo workspace
  scaffolded, CLI with 8 subcommands, TOML configuration parser,
  XBUILD/PKGBUILD parsers, recipe validation, srcinfo generator, `xpkg new`,
  HTTP downloader with retries, SHA-256/512 checksum verification, archive
  extraction (tar.gz/xz/bz2/zst, zip), Git clone support and source caching.
  `xpkg build` now wires that pipeline into the build itself: declared sources
  are fetched (cached under `~/.cache/xpkg/sources` by default), verified and
  extracted before the phases run, and phases execute wrapped in a shell
  function so `local`/`return` in real PKGBUILDs work.
- The **phase checklists** in the same file mark the later phases (build
  engine, metadata generation, archive creation, package linting, repository
  management, verify/info commands) as complete. Phase 9 is complete as well:
  the xpkg↔xpm lifecycle tests exist and the comparative benchmark vs
  `makepkg` runs as `cargo test -p xpkg --test bench_makepkg` (methodology and
  recorded numbers in `docs/BENCHMARKS.md`). The suite counts 265+ unit tests
  plus integration tests against a local HTTP server, real `makepkg` packages
  and gzip/xz archives. Phase 10 (post-v1.0 future goals) remains open.
- `docs/realexample.md` records a real end-to-end run: an `xfetch` package was
  built into a `.xp` artifact with xpkg, inspected, linted, added to a local
  ALPM database and a `file://` repository layout under `x-repo`, and synced
  by `xpm`.

## Versioning convention

| Release | Milestone |
|---------|-----------|
| `v0.1.0` | Phases 0-1 - functional CLI with configuration |
| `v0.3.0` | Phases 2-3 - recipe parsing and source management |
| `v0.5.0` | Phases 4-6 - build engine and archive creation |
| `v0.7.0` | Phase 7 - linting framework |
| `v0.9.0` | Phases 8-9 - repository tooling and integration |
| `v1.0.0` | Benchmarked, tested, production-ready |

The workspace `Cargo.toml` currently declares version `0.1.0` for both
crates.
