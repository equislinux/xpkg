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
pkgdesc="xlnux xpkg<->xpm E2E fixture"
arch=('x86_64')
url="https://example.com/e2e-hello"
license=('MIT')

build() {
  printf '#!/bin/sh\necho @MARKER@\n' > e2e-hello
  chmod +x e2e-hello
  printf 'xlnux e2e fixture\n' > README
}

package() {
  install -Dm755 e2e-hello "$PKGDIR/usr/bin/e2e-hello"
  install -Dm644 README "$PKGDIR/usr/share/doc/e2e-hello/README"
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

    // `<workspace>/xpkg/crates/xpkg` → `<workspace>` (the xlnux checkout root).
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

        // Local file repository named `x` (x-repo layout: `$arch` dir with
        // `x.db` + `x.db.tar.gz`). Signature checks are disabled for the test.
        let repo_base = self.repo_dir.parent().expect("repo base dir");
        let config = format!(
            "[options]\narchitecture = \"x86_64\"\nsig_level = \"never\"\ncolor = false\n\n\
             [[repo]]\nname = \"x\"\nserver = [\"file://{}/$arch\"]\nsig_level = \"never\"\n",
            repo_base.display()
        );
        fs::write(&self.xpm_config, config).expect("write xpm config");
    }

    fn write_recipe(&self, version: &str, marker: &str) {
        let recipe = RECIPE
            .replace("@VERSION@", version)
            .replace("@MARKER@", marker);
        fs::write(&self.recipe, recipe).expect("write PKGBUILD fixture");
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
        self.write_recipe(version, marker);

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
            out.contains(&format!("==> Built {PKG}-{version}-1")),
            "unexpected xpkg build output:\n{out}"
        );

        let archive = self.repo_dir.join(format!("{PKG}-{version}-1-x86_64.xp"));
        assert!(
            archive.is_file(),
            "missing built package {}",
            archive.display()
        );
        archive
    }

    /// Add the package to the local repo DB and mirror the x-repo layout:
    /// `x.db.tar.gz` is the real archive, `x.db` the alias xpm downloads.
    fn publish(&self, archive: &Path) {
        let db_gz = self.repo_dir.join("x.db.tar.gz");
        let output = self.xpkg(&[
            "repo-add",
            db_gz.to_str().expect("utf-8 db path"),
            archive.to_str().expect("utf-8 archive path"),
        ]);
        assert_success(&output, "xpkg repo-add");
        assert!(stdout(&output).contains("Repository now contains 1 package(s)"));

        fs::copy(&db_gz, self.repo_dir.join("x.db")).expect("mirror x.db alias");
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
    assert!(h.repo_dir.join("x.db").is_file());
    assert!(h.repo_dir.join("x.db.tar.gz").is_file());
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
        "xlnux e2e fixture\n"
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
}
