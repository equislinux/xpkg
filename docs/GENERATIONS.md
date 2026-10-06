# xpkg — Generation alignment: version retention and provenance

Context: X Linux rolls the system back with **generations** (whole-root btrfs
snapshots; `equislinux/scripts`, `docs/en/generations.md`). Generations cover the
common case, but **partial downgrades** (one package, without reverting the
system) need something the current repositories do not provide: old package
versions still indexed and fetchable. This document proposes that, plus the
provenance fields a generation manifest needs to be meaningful.

## Proposal

### 1. Version retention in repositories

- `xpkg repo-add --keep N` (implemented default: `0`, i.e. no pruning):
  retain package files **and** their database entries for the last N versions
  of each package. Today `repo-add` upserts by name, so the database only
  exposes the newest version even if old files remain on disk.
- `xpkg repo-prune --keep N [--dry-run]`: sweep old `.xp`/`.sig` files and
  rebuild the database. Never deletes the current version or a pinned one.
- The GitHub Pages layout consumed by `equislinux/x-repo` stays unchanged; retention
  only adds old files next to the current ones.

### 2. History index

- Write `history.json` (+ `history.json.sig`) per repo/arch:

```json
{
  "schema": 1,
  "repo": "x",
  "arch": "x86_64",
  "packages": {
    "kitty": [
      {"version": "0.44.0-1", "filename": "kitty-0.44.0-1-x86_64.xp",
       "sha256": "...", "sig": "kitty-...xp.sig", "builddate": 1780000000,
       "source": {"url": "...", "sha256": "...", "commit": "..."}}
    ]
  }
}
```

- `xpm` consumes it for `install <pkg>=<ver>` (see
  `../xpm/docs/GENERATIONS.md`); the resolver gets a candidate list per
  version instead of only the latest.
- The index is signed like the repository database.

### 3. Provenance in packages

- `.BUILDINFO` additions (extended, backward compatible):
  `x:source_commit`, `x:recipe_sha256`, `x:tool_version`.
- Lint rule: warn when a source has neither a checksum nor a pinned
  commit/tag. A manifest that records "kitty 0.44.0-1, sha256 ..., built from
  commit ..." is only useful if the builder knows it.

### 4. Reproducibility groundwork

- Honor `SOURCE_DATE_EPOCH` for tar entries and metadata timestamps, so the
  same recipe + sources produce the same package hash. This makes manifest
  hashes comparable across machines (the honest limit: full Nix-style
  reproducibility is not promised, but the cheap 80% is).

### 5. End-to-end integration (roadmap Phase 9, #56)

Extend the xpkg↔xpm integration test: build with `xpkg` → publish with a
retention-enabled repo → install with `xpm` → downgrade one package from
`history.json` → confirm a generation manifest captured both states.

## Implementation status

### Phase 1 — `--keep N` retention + `repo-prune` (done)

- `xpkg repo-add --keep N` prunes after adding: at most `N` versions per
  package survive (newest first by `builddate`), and the version exposed by
  the database is **never** deleted. `--keep 0` (default) disables pruning.
- `xpkg repo-prune --keep N [--dry-run]` applies the same policy to an
  existing repository and rewrites `history.json`; `--dry-run` only reports
  what would be removed.
- Retention only considers files listed in `history.json`. Files that are not
  indexed (or whose entries have no file on disk) are never touched, so
  running prune against a repository without history is safe.
- The repository database keeps exposing only the newest version: the
  candidate list for old versions lives in `history.json`. Keeping multiple
  versions in the database itself remains out of scope.

### Phase 2 — `history.json` + signature (done, with caveats)

- `repo-add` maintains `<repo-dir>/history.json` (schema 1) idempotently:
  re-adding a version updates its entry instead of duplicating it.
- Each entry records `version` (`version-release`), `filename`, the
  `.xp` `sha256`, `builddate` (epoch), the `.sig` name when present, and a
  `source` object when provenance data is available. `source` is read from
  extended `.BUILDINFO` fields when present (`x:source_url`,
  `x:source_sha256`, `x:source_commit`), falling back to the `.PKGINFO`
  `url`; when there is no data the field is omitted. The current builder
  emits `x:source_commit` (phase 3) but not `x:source_url`/`x:source_sha256`,
  so most packages expose the project URL plus, for pinned Git sources, the
  exact commit.
- `history.json.sig` is produced when a secret key is available
  (`sign_key` in `xpkg.conf`, or `repo-add --sign`). If the index changes and
  no key is configured, an existing `history.json.sig` is removed with a
  warning because it no longer verifies.
- Pending: signing cannot be derived from the package `.sig` alone (a
  detached signature does not expose the secret key), so a repository that
  signs packages but has no `sign_key` configured on the publishing machine
  gets an unsigned `history.json`. External signing (or configuring
  `sign_key`) is required in that case.
- Pending: `deploy_repo` (library helper) does not copy history-referenced
  versions yet; publishing flows that use it must keep old `.xp` files in the
  deployed layout themselves.

### Phase 3 — `.BUILDINFO` provenance + lint rule (done)

- `create_package` receives a `BuildProvenance` and `generate_buildinfo`
  appends the extended keys at the end of the file, leaving the historical
  `key = value` format intact:
  - `x:recipe_sha256` — SHA-256 of the recipe file (XBUILD or PKGBUILD)
    used for the build.
  - `x:source_commit` — exact commit of the first Git source with an
    explicit `#commit=`/`#tag=`/`#branch=` reference, when resolvable. It is
    omitted when no source declares such a reference.
  - `x:tool_version` — always emitted (`CARGO_PKG_VERSION`).
- Git source URLs now accept makepkg-style fragments: `#commit=`,
  `#tag=` and `#branch=`. The source manager clones the repository, checks
  out the requested reference (a bare commit requires clone + checkout) and,
  when a fetched source tree is provided to `BuildProvenance::collect`, the
  exact commit is read from the clone's `HEAD`. Without a local clone, tags
  and branches are resolved best-effort with `git ls-remote` (tags use the
  peeled `^{}` ref so annotated tags yield the commit); `commit=` values are
  used verbatim. Resolution failures are logged and the line is omitted
  instead of failing the build.
- Recipe validation accepts the Git schemes (`git://`, `git+https://`,
  `git+http://`) in addition to http/https/ftp/file.
- New lint rule `source-unpinned` (warning): a source with no usable
  checksum (`sha256sums`/`sha512sums`, `SKIP` does not count) and no pinned
  Git commit/tag. `#branch=` and floating Git URLs are not pinned because
  they can move. The rule is recipe-level and runs at the start of
  `xpkg build`; diagnostics are reported but never stop the build.

### Phase 4 — `SOURCE_DATE_EPOCH` support (done)

- When `SOURCE_DATE_EPOCH` is set to a valid Unix timestamp, it is used for
  the `.PKGINFO` / `.BUILDINFO` `builddate` and for the mtime of every tar
  entry (metadata, files, directories and symlinks). Without the variable
  the builder keeps using the current time.
- This is reproducibility groundwork, not a full guarantee: it makes
  metadata timestamps comparable across machines, but full Nix-style binary
  reproducibility is still not promised (compilers, absolute paths and
  archive ordering can vary).

### Phase 5 — end-to-end integration with xpm (pending, #56)

## Non-goals

- xpkg does not know about generations, snapshots or boot entries; it only
  guarantees that old versions stay retrievable and that builds are traceable.

See also: `../scripts/docs/en/generations.md` (generations engine),
`../xpm/docs/GENERATIONS.md` (consumer plan).
