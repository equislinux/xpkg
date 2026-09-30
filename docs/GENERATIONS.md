# xpkg — Generation alignment: version retention and provenance

Context: X Linux rolls the system back with **generations** (whole-root btrfs
snapshots; `xlnux/scripts`, `docs/en/generations.md`). Generations cover the
common case, but **partial downgrades** (one package, without reverting the
system) need something the current repositories do not provide: old package
versions still indexed and fetchable. This document proposes that, plus the
provenance fields a generation manifest needs to be meaningful.

## Proposal

### 1. Version retention in repositories

- `xpkg repo-add --keep N` (default: keep the last N versions, e.g. 3):
  retain package files **and** their database entries for the last N versions
  of each package. Today `repo-add` upserts by name, so the database only
  exposes the newest version even if old files remain on disk.
- `xpkg repo-prune --keep N [--dry-run]`: sweep old `.xp`/`.sig` files and
  rebuild the database. Never deletes the current version or a pinned one.
- The GitHub Pages layout consumed by `xlnux/x-repo` stays unchanged; retention
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

## Non-goals

- xpkg does not know about generations, snapshots or boot entries; it only
  guarantees that old versions stay retrievable and that builds are traceable.

## Suggested phases

1. `--keep N` retention + `repo-prune`.
2. `history.json` + signature.
3. `.BUILDINFO` provenance + lint rule.
4. `SOURCE_DATE_EPOCH` support.
5. Integration test with xpm (#56).

See also: `../scripts/docs/en/generations.md` (generations engine),
`../xpm/docs/GENERATIONS.md` (consumer plan).
