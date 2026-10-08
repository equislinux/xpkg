//! Build script execution.
//!
//! Runs shell scripts from recipe build phases via `/bin/sh -e`.
//! Supports three privilege strategies for the `package()` phase:
//!
//! 1. **User namespaces** (`unshare --user --map-root-user`) — kernel-native,
//!    no external dependencies. The user appears as root inside the namespace.
//! 2. **fakeroot** — LD_PRELOAD-based, intercepts chown/stat. Requires the
//!    `fakeroot` binary on PATH.
//! 3. **Direct execution** — no wrapper. Works for the majority of packages
//!    that use `DESTDIR` or `make install DESTDIR=...`.
//!
//! The archive creation phase (Phase 6) rewrites tar headers to uid/gid=0,
//! which is the definitive guarantee of correct ownership in the `.xp` package.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};

use crate::error::{XpkgError, XpkgResult};

use super::log::LogWriter;
use super::types::BuildPhase;

/// Privilege wrapper strategy for the `package()` phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FakerootStrategy {
    /// Linux user namespaces via `unshare --user --map-root-user`.
    UserNamespace,
    /// External `fakeroot` binary (LD_PRELOAD).
    Fakeroot,
    /// No wrapper — direct execution.
    None,
}

impl std::fmt::Display for FakerootStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UserNamespace => write!(f, "unshare (user namespace)"),
            Self::Fakeroot => write!(f, "fakeroot"),
            Self::None => write!(f, "none"),
        }
    }
}

/// Detect the best available privilege wrapper strategy.
///
/// Tries user namespaces first, then fakeroot, then falls back to none.
pub fn detect_fakeroot_strategy() -> FakerootStrategy {
    if check_user_namespace_support() {
        tracing::info!("using user namespace (unshare) for fakeroot");
        return FakerootStrategy::UserNamespace;
    }

    if check_fakeroot_available() {
        tracing::info!("using fakeroot for package phase");
        return FakerootStrategy::Fakeroot;
    }

    tracing::warn!(
        "neither user namespaces nor fakeroot are available; \
         package ownership will be fixed at archive creation time"
    );
    FakerootStrategy::None
}

/// Check if user namespaces are supported and usable.
fn check_user_namespace_support() -> bool {
    Command::new("unshare")
        .args(["--user", "--map-root-user", "true"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Check if `fakeroot` is available on PATH.
fn check_fakeroot_available() -> bool {
    Command::new("fakeroot")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Execute a build phase script.
///
/// The script is run via `/bin/sh -e` (exit on first error) with the
/// provided environment variables and working directory.
pub fn run_phase(
    phase: BuildPhase,
    script: &str,
    workdir: &Path,
    env_vars: &HashMap<String, String>,
    fakeroot: FakerootStrategy,
    log_writer: &mut LogWriter,
) -> XpkgResult<()> {
    if script.trim().is_empty() {
        tracing::debug!(%phase, "phase has no script, skipping");
        return Ok(());
    }

    tracing::info!(%phase, "running build phase");
    log_writer.write_phase_header(phase);

    let mut cmd = build_command(phase, script, fakeroot);
    cmd.current_dir(workdir);
    cmd.env_clear();

    // Preserve essential system environment.
    for key in ["PATH", "HOME", "TERM", "LANG", "USER", "SHELL"] {
        if let Ok(val) = std::env::var(key) {
            cmd.env(key, val);
        }
    }

    // Set build-specific environment variables.
    for (key, val) in env_vars {
        cmd.env(key, val);
    }

    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| {
        XpkgError::BuildFailed(format!("failed to spawn shell for {phase} phase: {e}"))
    })?;

    // Stream stdout and stderr to log writer and tracing.
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    if let Some(stdout) = stdout {
        let reader = BufReader::new(stdout);
        for line in reader.lines().map_while(Result::ok) {
            tracing::debug!(phase = %phase, "{}", line);
            log_writer.write_line(&line);
        }
    }

    if let Some(stderr) = stderr {
        let reader = BufReader::new(stderr);
        for line in reader.lines().map_while(Result::ok) {
            tracing::debug!(phase = %phase, "[stderr] {}", line);
            log_writer.write_line(&format!("[stderr] {line}"));
        }
    }

    let status = child
        .wait()
        .map_err(|e| XpkgError::BuildFailed(format!("failed to wait for {phase} phase: {e}")))?;

    if !status.success() {
        let code = status.code().unwrap_or(-1);
        return Err(XpkgError::BuildFailed(format!(
            "{phase}() failed with exit code {code}"
        )));
    }

    tracing::info!(%phase, "phase completed successfully");
    Ok(())
}

/// Wraps a phase script in a shell function, the way makepkg runs PKGBUILD
/// functions. This makes `local`, `return` and `declare` usable at the top
/// level of a phase body, which real recipes rely on.
fn wrap_phase_script(script: &str) -> String {
    format!("xpkg_phase() {{\n{script}\n}}\nxpkg_phase\n")
}

/// Build the shell command for a given phase and fakeroot strategy.
fn build_command(phase: BuildPhase, script: &str, fakeroot: FakerootStrategy) -> Command {
    let use_wrapper = phase == BuildPhase::Package && fakeroot != FakerootStrategy::None;
    let wrapped = wrap_phase_script(script);

    if use_wrapper {
        match fakeroot {
            FakerootStrategy::UserNamespace => {
                let mut cmd = Command::new("unshare");
                cmd.args(["--user", "--map-root-user", "/bin/sh", "-e", "-c", &wrapped]);
                cmd
            }
            FakerootStrategy::Fakeroot => {
                let mut cmd = Command::new("fakeroot");
                cmd.args(["--", "/bin/sh", "-e", "-c", &wrapped]);
                cmd
            }
            FakerootStrategy::None => unreachable!(),
        }
    } else {
        let mut cmd = Command::new("/bin/sh");
        cmd.args(["-e", "-c", &wrapped]);
        cmd
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_command_no_wrapper() {
        let cmd = build_command(BuildPhase::Build, "make", FakerootStrategy::None);
        assert_eq!(cmd.get_program(), "/bin/sh");
    }

    #[test]
    fn test_build_command_fakeroot_only_for_package() {
        // Build phase should NOT use wrapper even if fakeroot is set.
        let cmd = build_command(BuildPhase::Build, "make", FakerootStrategy::Fakeroot);
        assert_eq!(cmd.get_program(), "/bin/sh");
    }

    #[test]
    fn test_build_command_package_with_unshare() {
        let cmd = build_command(
            BuildPhase::Package,
            "make install",
            FakerootStrategy::UserNamespace,
        );
        assert_eq!(cmd.get_program(), "unshare");
    }

    #[test]
    fn test_build_command_package_with_fakeroot() {
        let cmd = build_command(
            BuildPhase::Package,
            "make install",
            FakerootStrategy::Fakeroot,
        );
        assert_eq!(cmd.get_program(), "fakeroot");
    }

    #[test]
    fn test_run_phase_skips_empty_script() {
        let mut log = LogWriter::new_null();
        let env = HashMap::new();
        let result = run_phase(
            BuildPhase::Prepare,
            "",
            Path::new("/tmp"),
            &env,
            FakerootStrategy::None,
            &mut log,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_run_phase_success() {
        let mut log = LogWriter::new_null();
        let env = HashMap::new();
        let result = run_phase(
            BuildPhase::Build,
            "echo hello",
            Path::new("/tmp"),
            &env,
            FakerootStrategy::None,
            &mut log,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_run_phase_failure() {
        let mut log = LogWriter::new_null();
        let env = HashMap::new();
        let result = run_phase(
            BuildPhase::Build,
            "exit 1",
            Path::new("/tmp"),
            &env,
            FakerootStrategy::None,
            &mut log,
        );
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("exit code 1"));
    }

    #[test]
    fn test_run_phase_uses_env_vars() {
        let mut log = LogWriter::new_null();
        let mut env = HashMap::new();
        env.insert("MY_VAR".into(), "hello_xpkg".into());
        let result = run_phase(
            BuildPhase::Build,
            "test \"$MY_VAR\" = \"hello_xpkg\"",
            Path::new("/tmp"),
            &env,
            FakerootStrategy::None,
            &mut log,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_run_phase_supports_local_declaration() {
        // Real PKGBUILD bodies start with `local var=...`; phases run wrapped
        // in a function so that works under /bin/sh (dash).
        let mut log = LogWriter::new_null();
        let env = HashMap::new();
        let result = run_phase(
            BuildPhase::Package,
            "local count=3\ntest \"$count\" = 3",
            Path::new("/tmp"),
            &env,
            FakerootStrategy::None,
            &mut log,
        );
        assert!(result.is_ok(), "local must be accepted: {result:?}");
    }
}
