//! End-to-end lifecycle test: `xpkg` builds a package from a PKGBUILD,
//! publishes it to a local file repository, and `xpm` consumes it
//! (`sync → install → upgrade → remove`) inside an isolated root.
//!
//! The test is hermetic:
//! - every artifact lives under one `tempfile::TempDir`;
//! - the repository is consumed through a `file://` URL (no network, no
//!   Arch mirrors);
//! - `HOME` and `XPM_HOOKS_DIR` are redirected into the temp dir so shell
//!   integration and transaction hooks never touch the host;
//! - no `sudo` and no btrfs/generations are required.
//!
//! `xpm` lives in a sibling Git repository, so its binary is located via
//! `XPM_BIN`, a sibling checkout's `target/{debug,release}/xpm`, or `PATH`.
//! When none is found the test prints a reason and returns, keeping
//! `cargo test` green in an xpkg-only checkout.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

/// Package name used by the generated fixture recipe.
const PKG: &str = "e2e-hello";

/// PKGBUILD fixture template. `@VERSION@` and `@MARKER@` are substituted per
/// build. `package()` installs with `$PKGDIR` because that is the environment
/// variable the xpkg build pipeline exposes (PKGBUILD `$pkgdir` is not set).
const RECIPE: &str = r#"pkgname=e2e-hello
pkgver=@VERSION@
pkgrel=1
pkgdesc="equislinux xpkg<->xpm E2E fixture"
arch=('x86_64')
url="https://example.com/e2e-hello"
license=('MIT')

build() {
  printf '#!/bin/sh\necho @MARKER@\n' > e2e-hello
  chmod +x e2e-hello
  printf 'equislinux e2e fixture\n' > README
}

package() {
  install -Dm755 e2e-hello "$PKGDIR/usr/bin/e2e-hello"
  install -Dm644 README "$PKGDIR/usr/share/doc/e2e-hello/README"
}
"#;

/// Dependency package for the resolver E2E (no dependencies itself).
const DEP_RECIPE: &str = r#"pkgname=e2e-lib
pkgver=@VERSION@
pkgrel=1
pkgdesc="equislinux resolver E2E dependency"
arch=('x86_64')
url="https://example.com/e2e-lib"
license=('MIT')

build() {
  printf '#!/bin/sh\necho lib\n' > e2e-lib
  chmod +x e2e-lib
}

package() {
  install -Dm755 e2e-lib "$PKGDIR/usr/bin/e2e-lib"
}
"#;

/// Dependent package for the resolver E2E (`depends=('e2e-lib')`).
const APP_RECIPE: &str = r#"pkgname=e2e-app
pkgver=@VERSION@
pkgrel=1
pkgdesc="equislinux resolver E2E dependent"
arch=('x86_64')
url="https://example.com/e2e-app"
license=('MIT')
depends=('e2e-lib')

build() {
  printf '#!/bin/sh\ne2e-lib\n' > e2e-app
  chmod +x e2e-app
}

package() {
  install -Dm755 e2e-app "$PKGDIR/usr/bin/e2e-app"
}
"#;

/// Standalone package used for the local-file install scenario.
const LOCAL_RECIPE: &str = r#"pkgname=e2e-local
pkgver=@VERSION@
pkgrel=1
pkgdesc="xpm local install fixture"
arch=('x86_64')
url="https://example.com/e2e-local"
license=('MIT')

build() {
  printf '#!/bin/sh\necho local\n' > e2e-local
  chmod +x e2e-local
}

package() {
  install -Dm755 e2e-local "$PKGDIR/usr/bin/e2e-local"
}
"#;

/// Locate the sibling `xpm` binary without ever invoking the network.
fn find_xpm_binary() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("XPM_BIN") {
        let path = PathBuf::from(path);
        assert!(
            path.is_file(),
            "XPM_BIN does not point to a file: {}",
            path.display()
        );
        return Some(path);
    }

    // `<workspace>/xpkg/crates/xpkg` → `<workspace>` (the equislinux checkout root).
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut candidates = Vec::new();
    if let Some(workspace) = manifest_dir
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
    {
        for profile in ["debug", "release"] {
            candidates.push(workspace.join("xpm/target").join(profile).join("xpm"));
        }
    }
    if let Some(target_dir) = std::env::var_os("CARGO_TARGET_DIR") {
        for profile in ["debug", "release"] {
            candidates.push(PathBuf::from(&target_dir).join(profile).join("xpm"));
        }
    }

    if let Some(found) = candidates.into_iter().find(|p| p.is_file()) {
        return Some(found);
    }

    let path_var = std::env::var_os("PATH")?;
    std::env::split_paths(&path_var)
        .map(|dir| dir.join("xpm"))
        .find(|p| p.is_file())
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn assert_success(output: &Output, what: &str) {
    assert!(
        output.status.success(),
        "{what} failed with {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        stdout(output),
        stderr(output),
    );
}

struct Harness {
    _tmp: TempDir,
    xpm: PathBuf,
    xpkg_config: PathBuf,
    xpm_config: PathBuf,
    repo_dir: PathBuf,
    root: PathBuf,
    db: PathBuf,
    cache: PathBuf,
    home: PathBuf,
    hooks: PathBuf,
    recipe: PathBuf,
    builddir: PathBuf,
}

impl Harness {
    fn new(xpm: PathBuf) -> Self {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let base = tmp.path();

        let harness = Self {
            xpkg_config: base.join("missing-xpkg.conf"),
            xpm_config: base.join("xpm.conf"),
            repo_dir: base.join("repo/x86_64"),
            root: base.join("root"),
            db: base.join("db"),
            cache: base.join("cache"),
            home: base.join("home"),
            hooks: base.join("hooks"),
            recipe: base.join("recipe/PKGBUILD"),
            builddir: base.join("build"),
            xpm,
            _tmp: tmp,
        };
        harness.prepare();
        harness
    }

    fn prepare(&self) {
        for dir in [
            &self.repo_dir,
            &self.root,
            &self.db,
            &self.cache,
            &self.home,
            &self.hooks,
            self.recipe.parent().expect("recipe parent"),
        ] {
            fs::create_dir_all(dir).expect("create dir");
        }

        // Local file repository named `x` (x-repo layout: `$arch` dir with a
        // zstd `x.db`). Signature checks are disabled for the test.
        let repo_base = self.repo_dir.parent().expect("repo base dir");
        let config = format!(
            "[options]\narchitecture = \"x86_64\"\nsig_level = \"never\"\ncolor = false\n\n\
             [[repo]]\nname = \"x\"\nserver = [\"file://{}/$arch\"]\nsig_level = \"never\"\n",
            repo_base.display()
        );
        fs::write(&self.xpm_config, config).expect("write xpm config");
    }

    /// Run the real `xpkg` binary against the isolated (absent) config.
    fn xpkg(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_xpkg"))
            .arg("--config")
            .arg(&self.xpkg_config)
            .args(args)
            .output()
            .expect("run xpkg")
    }

    /// Run `xpm` with the host-mutating knobs redirected into the temp dir.
    fn xpm(&self, args: &[&str]) -> Output {
        Command::new(&self.xpm)
            .arg("--config")
            .arg(&self.xpm_config)
            .arg("--root")
            .arg(&self.root)
            .arg("--dbpath")
            .arg(&self.db)
            .arg("--cachedir")
            .arg(&self.cache)
            .arg("--no-confirm")
            .args(args)
            .env("HOME", &self.home)
            .env("XPM_HOOKS_DIR", &self.hooks)
            .output()
            .expect("run xpm")
    }

    /// Build `PKG` at `version` with the recipe fixture; returns the `.xp`.
    fn build_package(&self, version: &str, marker: &str) -> PathBuf {
        let recipe = RECIPE
            .replace("@VERSION@", version)
            .replace("@MARKER@", marker);
        self.build_recipe_body(&recipe, PKG, version)
    }

    /// Build an arbitrary PKGBUILD body (`@VERSION@` substituted).
    fn build_recipe_body(&self, recipe_body: &str, name: &str, version: &str) -> PathBuf {
        fs::write(&self.recipe, recipe_body.replace("@VERSION@", version))
            .expect("write PKGBUILD fixture");

        let outdir = self.repo_dir.to_str().expect("utf-8 repo dir");
        let builddir = self.builddir.to_str().expect("utf-8 build dir");
        let recipe = self.recipe.to_str().expect("utf-8 recipe path");

        let output = Command::new(env!("CARGO_BIN_EXE_xpkg"))
            .arg("--config")
            .arg(&self.xpkg_config)
            .args(["build", "-f", recipe, "--pkgbuild", "--no-check"])
            .args(["-o", outdir, "-d", builddir])
            .env("SOURCE_DATE_EPOCH", "1700000000")
            .output()
            .expect("run xpkg build");
        assert_success(&output, "xpkg build");

        let out = stdout(&output);
        assert!(
            out.contains(&format!("==> Built {name}-{version}-1")),
            "unexpected xpkg build output:\n{out}"
        );

        let archive = self.repo_dir.join(format!("{name}-{version}-1-x86_64.xp"));
        assert!(
            archive.is_file(),
            "missing built package {}",
            archive.display()
        );
        archive
    }

    /// Add the package to the local repo DB: xpkg writes a zstd-compressed
    /// `x.db`, which xpm downloads and parses natively.
    fn publish(&self, archive: &Path) {
        self.publish_expect(archive, 1);
    }

    fn publish_expect(&self, archive: &Path, expected: usize) {
        let db = self.repo_dir.join("x.db");
        let output = self.xpkg(&[
            "repo-add",
            db.to_str().expect("utf-8 db path"),
            archive.to_str().expect("utf-8 archive path"),
        ]);
        assert_success(&output, "xpkg repo-add");
        assert!(
            stdout(&output).contains(&format!("Repository now contains {expected} package(s)")),
            "unexpected repo-add output:\n{}",
            stdout(&output)
        );
    }

    fn local_db_file(&self, file: &str) -> PathBuf {
        self.db.join("local").join(PKG).join(file)
    }
}

#[test]
fn xpkg_builds_and_publishes_and_xpm_installs_upgrades_removes() {
    let Some(xpm) = find_xpm_binary() else {
        eprintln!(
            "skipping xpkg <-> xpm E2E: xpm binary not found; \
             set XPM_BIN or build the sibling checkout (cargo build -p xpm)"
        );
        return;
    };

    let h = Harness::new(xpm);

    // ── 1. xpkg builds v1 from a PKGBUILD fixture ───────────────────────
    let v1 = h.build_package("1.0", "e2e-hello");

    // ── 2. xpkg repo-add publishes it to the local file repository ──────
    h.publish(&v1);
    let db_bytes = fs::read(h.repo_dir.join("x.db")).expect("read published x.db");
    assert_eq!(
        db_bytes.get(..4),
        Some(&[0x28, 0xb5, 0x2f, 0xfd][..]),
        "xpkg should publish a zstd-compressed database"
    );
    assert!(h.repo_dir.join("history.json").is_file());
    let history: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(h.repo_dir.join("history.json")).unwrap())
            .expect("parse history.json");
    assert_eq!(history["packages"][PKG][0]["version"], "1.0-1");

    // ── 3. xpm sync consumes the file:// repo into an isolated db ───────
    let sync = h.xpm(&["sync"]);
    assert_success(&sync, "xpm sync");
    let sync_out = stdout(&sync);
    assert!(
        sync_out.contains("remote: x.db updated"),
        "sync did not confirm the local repo:\n{sync_out}"
    );
    assert!(
        sync_out.contains("1 package(s) loaded"),
        "sync did not parse the xpkg database:\n{sync_out}"
    );
    assert!(h.db.join("sync/x.db").is_file());

    // Search matches name/description case-insensitively in the sync db.
    let search = h.xpm(&["search", "E2E-HELLO"]);
    assert_success(&search, "xpm search");
    let search_out = stdout(&search);
    assert!(
        search_out.contains("x/e2e-hello 1.0-1"),
        "sync search missed the package:\n{search_out}"
    );
    assert!(
        search_out.contains("equislinux xpkg<->xpm E2E fixture"),
        "sync search did not print the description:\n{search_out}"
    );

    // ── 4. xpm install into the isolated root ───────────────────────────
    let install = h.xpm(&["install", PKG]);
    assert_success(&install, "xpm install");
    assert!(stdout(&install).contains("1 package(s) installed successfully"));

    let bin = h.root.join("usr/bin/e2e-hello");
    assert_eq!(
        fs::read_to_string(&bin).expect("installed binary"),
        "#!/bin/sh\necho e2e-hello\n"
    );
    assert_ne!(
        fs::metadata(&bin).unwrap().permissions().mode() & 0o111,
        0,
        "installed binary should be executable"
    );
    assert_eq!(
        fs::read_to_string(h.root.join("usr/share/doc/e2e-hello/README"))
            .expect("installed README"),
        "equislinux e2e fixture\n"
    );
    assert!(h.root.join("var/log/xpm.log").is_file());

    // Local database was updated: version, reason, origin and file manifest.
    assert_eq!(
        fs::read_to_string(h.local_db_file("version")).unwrap(),
        "1.0-1"
    );
    assert_eq!(
        fs::read_to_string(h.local_db_file("reason")).unwrap(),
        "explicit"
    );
    assert_eq!(fs::read_to_string(h.local_db_file("origin")).unwrap(), "x");
    assert!(fs::read_to_string(h.local_db_file("files"))
        .unwrap()
        .contains("usr/bin/e2e-hello"));

    // Shell integration shims went to the redirected HOME, not the host.
    let shim = h.home.join(".local/bin/e2e-hello");
    assert!(
        shim.symlink_metadata()
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false),
        "shell shim should live under the temp HOME"
    );

    // Journal recorded the transaction.
    let journal = h.xpm(&["history", "--json"]);
    assert_success(&journal, "xpm history");
    assert!(stdout(&journal).contains("\"action\":\"install\""));

    // ── 5. Query surfaces the installed package ─────────────────────────
    let query = h.xpm(&["query", "--format", "tsv"]);
    assert_success(&query, "xpm query");
    assert_eq!(stdout(&query), format!("{PKG}\t1.0-1\n"));

    let files = h.xpm(&["files", PKG]);
    assert_success(&files, "xpm files");
    assert!(stdout(&files).contains("usr/bin/e2e-hello"));

    let info = h.xpm(&["info", "--local", PKG]);
    assert_success(&info, "xpm info");
    assert!(stdout(&info).contains("Version         : 1.0-1"));

    let local_search = h.xpm(&["search", "--local", "e2e-hello"]);
    assert_success(&local_search, "xpm search --local");
    assert!(
        stdout(&local_search).contains("local/e2e-hello 1.0-1"),
        "local search missed the installed package:\n{}",
        stdout(&local_search)
    );

    // ── 6. Rebuild v2, republish and upgrade ────────────────────────────
    let v2 = h.build_package("2.0", "e2e-hello-v2");
    h.publish(&v2);

    let upgrade = h.xpm(&["upgrade"]);
    assert_success(&upgrade, "xpm upgrade");
    let upgrade_out = stdout(&upgrade);
    assert!(
        upgrade_out.contains("1.0-1 -> 2.0-1"),
        "upgrade did not plan the new version:\n{upgrade_out}"
    );
    assert_eq!(
        fs::read_to_string(&bin).expect("upgraded binary"),
        "#!/bin/sh\necho e2e-hello-v2\n"
    );
    assert_eq!(
        fs::read_to_string(h.local_db_file("version")).unwrap(),
        "2.0-1"
    );
    assert_eq!(
        fs::read_to_string(h.local_db_file("reason")).unwrap(),
        "explicit"
    );
    assert_eq!(fs::read_to_string(h.local_db_file("origin")).unwrap(), "x");

    // ── 7. Remove cleans files and the local database entry ─────────────
    let remove = h.xpm(&["remove", PKG]);
    assert_success(&remove, "xpm remove");
    assert!(stdout(&remove).contains("1 package(s) removed successfully"));
    assert!(!bin.exists(), "binary should be gone after remove");
    assert!(
        !h.root.join("usr/share/doc/e2e-hello/README").exists(),
        "README should be gone after remove"
    );
    assert!(
        !h.db.join("local").join(PKG).exists(),
        "local database entry should be gone after remove"
    );

    let query = h.xpm(&["query"]);
    assert_success(&query, "xpm query after remove");
    assert_eq!(stdout(&query), "");

    // ── 8. Exit codes: unknown package fails cleanly ────────────────────
    let missing = h.xpm(&["install", "e2e-not-a-package"]);
    assert!(
        !missing.status.success(),
        "installing an unknown package should fail:\n{}",
        stdout(&missing)
    );

    // ── 9. Orphans: a dependency nothing requires is reported ───────────
    let as_dep = h.xpm(&["install", "--as-deps", PKG]);
    assert_success(&as_dep, "xpm install --as-deps");

    let orphans = h.xpm(&["query", "--orphans"]);
    assert_success(&orphans, "xpm query --orphans");
    assert_eq!(
        stdout(&orphans),
        format!("{PKG} 2.0-1\n"),
        "a dependency package with no explicit requirer should be an orphan"
    );

    // Marking it explicit clears the orphan status.
    let as_explicit = h.xpm(&["install", "--as-explicit", PKG]);
    assert_success(&as_explicit, "xpm install --as-explicit");
    let orphans = h.xpm(&["query", "--orphans"]);
    assert_success(&orphans, "xpm query --orphans after explicit");
    assert_eq!(stdout(&orphans), "");
}

#[test]
fn xpm_resolves_dependency_closure_end_to_end() {
    let Some(xpm) = find_xpm_binary() else {
        eprintln!(
            "skipping resolver E2E: xpm binary not found; \
             set XPM_BIN or build the sibling checkout (cargo build -p xpm)"
        );
        return;
    };

    let h = Harness::new(xpm);

    let lib = h.build_recipe_body(DEP_RECIPE, "e2e-lib", "1.0");
    h.publish_expect(&lib, 1);
    let app = h.build_recipe_body(APP_RECIPE, "e2e-app", "1.0");
    h.publish_expect(&app, 2);

    let sync = h.xpm(&["sync"]);
    assert_success(&sync, "xpm sync");

    let install = h.xpm(&["install", "e2e-app"]);
    assert_success(&install, "xpm install");
    let out = stdout(&install);
    assert!(
        out.contains("Resolved 2 package(s)"),
        "plan summary:\n{out}"
    );
    assert!(
        out.contains("1 explicit, 1 as dependencies"),
        "plan summary:\n{out}"
    );
    assert!(out.contains("2 package(s) installed successfully"), "{out}");

    // Dependencies are downloaded before the requested package.
    let lib_pos = out.find("e2e-lib-1.0-1").expect("lib download line");
    let app_pos = out.find("e2e-app-1.0-1").expect("app download line");
    assert!(
        lib_pos < app_pos,
        "dependency must be installed first:\n{out}"
    );

    assert!(h.root.join("usr/bin/e2e-lib").is_file());
    assert!(h.root.join("usr/bin/e2e-app").is_file());

    let reason = |name: &str| {
        fs::read_to_string(h.db.join("local").join(name).join("reason")).unwrap_or_default()
    };
    assert_eq!(reason("e2e-app").trim(), "explicit");
    assert_eq!(reason("e2e-lib").trim(), "dep");
}

#[test]
fn xpm_installs_local_file_and_upgrades_with_closure() {
    let Some(xpm) = find_xpm_binary() else {
        eprintln!(
            "skipping resolver E2E: xpm binary not found; \
             set XPM_BIN or build the sibling checkout (cargo build -p xpm)"
        );
        return;
    };

    let h = Harness::new(xpm);
    let read_local = |name: &str, file: &str| {
        fs::read_to_string(h.db.join("local").join(name).join(file)).unwrap_or_default()
    };

    // ── Local `.xp` install (no repository involved) ────────────────────
    let local = h.build_recipe_body(LOCAL_RECIPE, "e2e-local", "1.0");
    let install = h.xpm(&["install", local.to_str().expect("utf-8 path")]);
    assert_success(&install, "xpm install <file>");
    let out = stdout(&install);
    assert!(out.contains("local: "), "local install output:\n{out}");
    assert!(out.contains("1 package(s) installed successfully"), "{out}");
    assert!(h.root.join("usr/bin/e2e-local").is_file());
    assert_eq!(read_local("e2e-local", "version").trim(), "1.0-1");
    assert_eq!(read_local("e2e-local", "origin").trim(), "local");
    assert_eq!(read_local("e2e-local", "reason").trim(), "explicit");

    // ── Repo install, then dependency-aware upgrade ─────────────────────
    let lib = h.build_recipe_body(DEP_RECIPE, "e2e-lib", "1.0");
    h.publish_expect(&lib, 1);
    let app = h.build_recipe_body(APP_RECIPE, "e2e-app", "1.0");
    h.publish_expect(&app, 2);
    assert_success(&h.xpm(&["sync"]), "xpm sync");
    assert_success(&h.xpm(&["install", "e2e-app"]), "xpm install e2e-app");
    assert_eq!(read_local("e2e-lib", "reason").trim(), "dep");

    // Publish newer versions for both packages and upgrade the system.
    let lib2 = h.build_recipe_body(DEP_RECIPE, "e2e-lib", "2.0");
    h.publish_expect(&lib2, 2);
    let app2 = h.build_recipe_body(APP_RECIPE, "e2e-app", "2.0");
    h.publish_expect(&app2, 2);

    let upgrade = h.xpm(&["upgrade"]);
    assert_success(&upgrade, "xpm upgrade");
    let out = stdout(&upgrade);
    assert!(out.contains("Packages to upgrade: 2"), "{out}");
    assert_eq!(read_local("e2e-lib", "version").trim(), "2.0-1");
    assert_eq!(read_local("e2e-app", "version").trim(), "2.0-1");
    assert_eq!(read_local("e2e-lib", "reason").trim(), "dep");
    assert_eq!(read_local("e2e-app", "reason").trim(), "explicit");
}
