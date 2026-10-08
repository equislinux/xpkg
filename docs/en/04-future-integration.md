# xpkg - Future Integration Notes

Context and notes for when the xpkg Rust tooling is re-activated inside the
equislinux *reboot*. This document is intentionally forward-looking and based on
`DECISIONS.md` and the workspace `ROADMAP.md` (both at the workspace root,
outside this repo), plus the open items in this repository's own `ROADMAP.md`.

Related documents in this folder:

- [01 - Overview and status](01-overview-and-status.md)
- [02 - Usage](02-usage.md)
- [03 - Architecture](03-architecture.md)

---

## Where packaging stands today

During the reboot, the distribution payload is packaged with the classical
Arch toolchain:

- Recipes are **PKGBUILD** files built with **makepkg** (the `x-scripts`
  package, built in the `scripts` repo packaging area).
- Resulting packages are published to the binary repository **`x-repo`**
  (the same layout xpkg would maintain through `repo-add`).
- The workspace roadmap keeps the Rust tooling phase (**Fase 4 - Tooling Rust
  xpm/xpkg**) **postponed**: xpm/xpkg are not part of the current delivery
  flow.

xpkg is therefore **not consumed by the reboot flow today**. It is the
intended future packager, in a REBOOT PENDING state.

## The guiding decision: ADR-0004

ADR-0004 (in `DECISIONS.md`) keeps xpkg/xpm as the distribution's own,
up-to-date tooling base (Rust + ALPM + sequoia signing) and states the
consequence that **own packages will be built with xpkg and served from
`x-repo`**. That only happens after the precondition is met:

> Wire the xpm SAT resolver to `install` (transitive deps) and support
> installing local `.xp` files; finish the remaining query stubs. Until then
> the system bootstrap must not depend on xpm.

Implication for xpkg: a full swap to xpkg-built packages is blocked upstream
in `xpm`'s install path, not by xpkg itself. xpkg already produces artifacts
that current `xpm sync` understands (see below).

## What already works end to end

`docs/realexample.md` in this repo documents a real run proving the path:

- `xpkg build` produced a real `.xp` for `xfetch`; `xpkg info` and `xpkg lint`
  validated it.
- `xpkg repo-add` created and updated ALPM databases (`.db.tar.gz` and a
  `.db` copy) in local static layouts, including the `x-repo` Pages layout.
- `xpm sync` against `file://` mirrors downloaded and parsed those databases.
- The recorded caveat (first run) that package download/install was a later
  xpm phase no longer applies: `xpm install`/`upgrade` now resolve, download,
  verify and install end to end, and Arch-style databases without `FILENAME`
  derive the conventional `name-version-arch.pkg.tar.zst` name. Metadata-only
  repos with artifacts hosted elsewhere (e.g. GitHub Releases) still require
  the fetch-URL composition in `xpm`.

## Open items in this repository

From this repository's `ROADMAP.md` (Phase 9 is now complete; only Phase 10
remains open):

- ~~Integration tests with xpm (#56).~~ Done: the xpkg↔xpm lifecycle suite
  builds with xpkg and installs/upgrades/removes with xpm.
- ~~Comparative benchmarks vs makepkg (#57).~~ Done:
  `cargo test -p xpkg --test bench_makepkg`, results in `docs/BENCHMARKS.md`.
- Phase 10 future goals (post-v1.0): split packages from one XBUILD,
  cross-compilation, clean chroot builds via namespaces, batch builds in
  dependency order, AUR-like helper integration, VCS package support,
  translations.

## Notes for re-activation

- **Trigger**: revisit when the Rust tooling is required again - the
  documented triggers are the xpm SAT resolver being wired to install and a
  `.xp` repository being the serving format.
- **Order**: land the xpm-side prerequisite first (ADR-0004), then switch the
  packaging of own packages from makepkg/PKGBUILD to xpkg/XBUILD and serve the
  result from `x-repo`. Reuse the existing `x-repo` infrastructure and the
  static layout validated in `realexample.md`.
- **Method**: the reboot works in `x/reboot` branches with commits kept local
  (no push by assistants); each phase closes with local tests per section
  before moving on; decisions are documented in the workspace `DECISIONS.md`.
- **Avoid duplication**: xpkg and xpm are independent binaries sharing one
  package format; keep them as the two complementary tools (builder vs
  manager) rather than merging responsibilities.
- **Compat**: keep the PKGBUILD parsing path while migration is ongoing so an
  existing Arch-style recipe can be built unchanged with
  `xpkg build --pkgbuild`.
