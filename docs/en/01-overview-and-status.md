# xpkg - Overview and Status

This is one of four documents describing `xpkg`, the Rust package builder of
the X distribution. It covers what the tool is, its role in the X ecosystem,
and its current status in the toolchain.

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

## Current status

`xpkg` is not a pending project: it is the builder behind the **native `.xp`
endpoint** that `xpm` consumes end to end.

- **Endpoint pipeline.** `x-repo/scripts/build-xp.sh` builds and signs every X
  package (`xpm`, `xpkg`, `x-release`, `x-dev` via XBUILD; `opencode-bin`,
  `xfetch-bin`, `xtop-git`, `x-scripts` via PKGBUILD) and regenerates
  `public/x/x86_64` (database, `history.json`, `SHA256SUMS`), published on
  GitHub Pages under `https://equislinux.github.io/x-repo/x/x86_64/`.
- **Two parallel paths.** The system itself is still provisioned through the
  pacman-compatible `[x]` repository built with `build-packages.sh` (makepkg +
  `repo-add`); the native `.xp` endpoint is the xpm-facing channel. Releasing
  regenerates both.
- **Consumed end to end.** `xpm sync`, `search`, dependency resolution and
  `install` were validated against the published endpoint; the xpkg↔xpm
  lifecycle suite builds with xpkg and installs/upgrades/removes with xpm.
- **makepkg parity.** Reviving the endpoint closed real parity gaps in xpkg:
  local sources next to the recipe, `name::URL` source syntax, expansion of
  recipe variables (including private `_helpers`) in sources and phases,
  `CARCH`/`CHOST` exported to phases, and `$srcdir` reset before fetching.

## Maturity of the codebase

The repository's own `ROADMAP.md` reports:

- **Phases 0-3 complete** - Cargo workspace scaffolded, CLI with 8
  subcommands, TOML configuration parser, XBUILD/PKGBUILD parsers, recipe
  validation, srcinfo generator, `xpkg new`, HTTP downloader with retries,
  SHA-256/512 checksum verification, archive extraction (tar.gz/xz/bz2/zst,
  zip), Git clone support and source caching. `xpkg build` wires that pipeline
  into the build itself: declared sources are fetched (cached under
  `~/.cache/xpkg/sources` by default), verified and extracted before the
  phases run, and phases execute wrapped in a shell function so
  `local`/`return` in real PKGBUILDs work.
- The **later phases** (build engine, metadata generation, archive creation,
  package linting, repository management, verify/info commands) are complete,
  including the `.files` database maintenance and signing. Phase 9 closed with
  the xpkg↔xpm lifecycle tests and the comparative benchmark vs `makepkg`
  (`cargo test -p xpkg --test bench_makepkg`, methodology and recorded numbers
  in `docs/BENCHMARKS.md`). The suite counts 265+ unit tests plus integration
  tests against a local HTTP server, real `makepkg` packages and gzip/xz
  archives. Phase 10 (post-v1.0 future goals) remains open.
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
