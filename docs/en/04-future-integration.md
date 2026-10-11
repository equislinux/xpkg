# xpkg - Integration Notes

How `xpkg` fits into the distribution flow: what already works end to end, the
open items, and the agreements worth keeping stable.

Related documents in this folder:

- [01 - Overview and status](01-overview-and-status.md)
- [02 - Usage](02-usage.md)
- [03 - Architecture](03-architecture.md)

---

## Where packaging stands today

There are **two live paths**, and releasing regenerates both:

- **pacman `[x]` repository** — `x-repo/build-packages.sh` builds the system
  packages with **makepkg + repo-add** (`x-release`, `x-dev`, `x-scripts`,
  `opencode-bin`, `xfetch-bin`, `xtop-git`, `xpm`). This is the path used by
  the installer and installed systems.
- **Native `.xp` endpoint** — `x-repo/scripts/build-xp.sh` builds and signs the
  same packages with **xpkg** (XBUILD or PKGBUILD) and regenerates
  `public/x/x86_64` (database, `history.json`, `SHA256SUMS`), served at
  `https://equislinux.github.io/x-repo/x/x86_64/` and consumed by `xpm`.

`xpkg` is **not pending or out of scope**: it is the builder of the native
channel, and its makepkg parity was closed while reviving the endpoint (local
sources, `name::URL`, recipe-variable expansion, `CARCH`/`CHOST` in phases,
`$srcdir` reset).

## What already works end to end

- `xpkg build` produces real `.xp` packages; `xpkg info` and `xpkg lint`
  validate them.
- `xpkg repo-add` creates and maintains ALPM databases (`.db.tar.gz` + `.db`
  copy, `.files`) in static layouts, including the GitHub Pages one.
- `xpm` syncs, resolves dependencies, downloads, verifies and installs both
  from the native endpoint and from Arch-style databases without `FILENAME`
  (it derives the conventional `name-version-arch.pkg.tar.zst` name).
- The xpkg↔xpm lifecycle suite (`tests/e2e_xpm_lifecycle.rs`) builds with xpkg
  and installs/upgrades/removes with xpm in an isolated root.

## Open items

From this repository's `ROADMAP.md` (phase 9 is closed; phase 10 remains):

- Post-v1.0 goals: split packages from one XBUILD, cross-compilation, clean
  chroot builds via namespaces, batch builds in dependency order, AUR-like
  helper integration, VCS package support and translations.

## Notes

- **No duplicated responsibilities**: `xpm` and `xpkg` are independent binaries
  sharing one package format; keep them as the two complementary tools (builder
  vs manager) rather than merging responsibilities.
- **Compatibility**: keep the PKGBUILD path so any Arch-style recipe can be
  built unchanged with `xpkg build --pkgbuild`.
- **Format in lockstep**: `.xp` == ALPM structure (`tar.zst` with `.PKGINFO`,
  `.BUILDINFO`, `.MTREE`, optional `.INSTALL`); any format change must land in
  both repos at once.
