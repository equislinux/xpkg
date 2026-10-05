//! xpkg — Package builder for X Distribution
//!
//! Entry point for the xpkg binary. Handles CLI parsing, configuration loading,
//! logging initialization, and dispatching to the appropriate subcommand handler.

mod cli;

use anyhow::{Context, Result};
use clap::Parser;
use tracing::Level;
use tracing_subscriber::EnvFilter;

use cli::{Cli, Command};
use xpkg_core::recipe;
use xpkg_core::XpkgConfig;

fn main() -> Result<()> {
    let cli = Cli::parse();

    // ── Initialize logging ──────────────────────────────────────────────
    let log_level = match cli.verbose {
        0 => Level::WARN,
        1 => Level::INFO,
        2 => Level::DEBUG,
        _ => Level::TRACE,
    };

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(log_level.into())
                .from_env_lossy(),
        )
        .with_target(false)
        .init();

    tracing::debug!("xpkg v{}", env!("CARGO_PKG_VERSION"));

    // ── Load configuration ──────────────────────────────────────────────
    let config_path = cli.config.clone().unwrap_or_else(XpkgConfig::default_path);

    let config = XpkgConfig::load_or_default(&config_path)
        .with_context(|| format!("failed to load config from {}", config_path.display()))?;

    tracing::info!(
        builddir = %config.options.builddir.display(),
        outdir = %config.options.outdir.display(),
        "configuration loaded"
    );

    // ── Dispatch subcommands ────────────────────────────────────────────
    match &cli.command {
        Command::Build(args) => cmd_build(&config, args),
        Command::Lint(args) => cmd_lint(&config, args),
        Command::New(args) => cmd_new(args),
        Command::Srcinfo(args) => cmd_srcinfo(args),
        Command::Info(args) => cmd_info(args),
        Command::Verify(args) => cmd_verify(args),
        Command::RepoAdd(args) => cmd_repo_add(&config, args),
        Command::RepoRemove(args) => cmd_repo_remove(&config, args),
        Command::RepoPrune(args) => cmd_repo_prune(&config, args),
    }
}

// ── Subcommand stubs ────────────────────────────────────────────────────────
//
// Each function below is a placeholder that will be filled with real logic
// as the corresponding roadmap phase is implemented.

fn cmd_build(config: &XpkgConfig, args: &cli::BuildArgs) -> Result<()> {
    use xpkg_core::archive::{create_package, strip_binaries};
    use xpkg_core::builder::{build_package, BuildOptions};

    // ── Resolve recipe path ─────────────────────────────────────────
    let recipe_path = args
        .file
        .clone()
        .unwrap_or_else(|| std::path::PathBuf::from("XBUILD"));

    let recipe_dir = recipe_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .to_path_buf();
    let recipe_dir = if recipe_dir.as_os_str().is_empty() {
        std::path::PathBuf::from(".")
    } else {
        recipe_dir
    };

    // ── Parse recipe ────────────────────────────────────────────────
    let raw_recipe = if args.pkgbuild {
        recipe::parse_pkgbuild(&recipe_path)
            .with_context(|| format!("failed to parse PKGBUILD {}", recipe_path.display()))?
    } else {
        recipe::parse_xbuild(&recipe_path)
            .with_context(|| format!("failed to parse {}", recipe_path.display()))?
    };

    recipe::validate_recipe(&raw_recipe)
        .with_context(|| format!("recipe validation failed for {}", recipe_path.display()))?;

    tracing::info!(
        name = %raw_recipe.package.name,
        version = %raw_recipe.package.version,
        "recipe loaded"
    );

    // ── Recipe lint (source integrity) ──────────────────────────────
    // Warnings are reported but never stop the build.
    let lint = xpkg_core::lint::lint_recipe(&raw_recipe, false);
    if lint.total() > 0 {
        use xpkg_core::lint::{format_report, ReportFormat};
        eprintln!("==> Recipe lint:");
        eprint!("{}", format_report(&lint, ReportFormat::Human));
    }

    // ── Collect provenance for the extended .BUILDINFO ──────────────
    // Captured before building so the hashed recipe and resolved commit
    // match the inputs of this build.
    let provenance = xpkg_core::metadata::BuildProvenance::collect(&recipe_path, &raw_recipe, None);

    // ── Apply CLI overrides to config ───────────────────────────────
    let mut build_config = config.clone();
    if let Some(ref builddir) = args.builddir {
        build_config.options.builddir = builddir.clone();
    }
    if let Some(ref outdir) = args.outdir {
        build_config.options.outdir = outdir.clone();
    }

    // ── Build options ───────────────────────────────────────────────
    let options = BuildOptions {
        skip_check: args.no_check,
        keep_builddir: false,
    };

    // ── Run build pipeline ──────────────────────────────────────────
    let result = build_package(&build_config, &raw_recipe, &recipe_dir, None, &options)?;

    println!(
        "==> Built {}-{}-{} in {:.1}s",
        result.pkgname,
        result.pkgver,
        result.pkgrel,
        result.duration.as_secs_f64()
    );

    // ── Strip binaries (optional) ───────────────────────────────────
    if build_config.options.strip_binaries {
        let stripped =
            strip_binaries(&result.pkgdir).with_context(|| "failed to strip binaries")?;
        if stripped > 0 {
            println!("    Stripped {stripped} ELF binaries");
        }
    }

    // ── Create .xp archive ──────────────────────────────────────────
    let outdir = &build_config.options.outdir;
    let pkg = create_package(
        &build_config,
        &raw_recipe,
        &result.pkgdir,
        outdir,
        &provenance,
    )
    .with_context(|| "failed to create package archive")?;

    println!("==> Package: {}", pkg.archive_path.display());
    println!("    Size: {:.1} KiB", pkg.archive_size as f64 / 1024.0);

    // ── Sign package (optional) ─────────────────────────────────────
    if args.sign || build_config.options.sign {
        use xpkg_core::signing::{load_secret_key, sign_file};

        let key_path = std::path::PathBuf::from(&build_config.options.sign_key);
        if key_path.as_os_str().is_empty() {
            anyhow::bail!("--sign requires a signing key; set sign_key in xpkg.conf");
        }

        let secret_key =
            load_secret_key(&key_path).with_context(|| "failed to load signing key")?;
        let sig = sign_file(&pkg.archive_path, &secret_key, false)
            .with_context(|| "failed to sign package")?;

        println!(
            "    Signature: {} ({} bytes, key {})",
            sig.sig_path.display(),
            sig.sig_size,
            sig.key_id
        );
    }

    Ok(())
}

fn cmd_lint(_config: &XpkgConfig, args: &cli::LintArgs) -> Result<()> {
    use xpkg_core::lint::{format_report, lint_package, ReportFormat};

    let archive_path = &args.package;
    tracing::info!(path = %archive_path.display(), "linting package");

    // ── Extract the archive to a temp directory ─────────────────────
    let tmp = tempfile::tempdir().with_context(|| "failed to create temp directory")?;
    let extract_dir = tmp.path();

    let file = std::fs::File::open(archive_path)
        .with_context(|| format!("failed to open {}", archive_path.display()))?;

    let decoder = zstd::Decoder::new(file)
        .with_context(|| format!("failed to decompress {}", archive_path.display()))?;
    let mut archive = tar::Archive::new(decoder);
    archive
        .unpack(extract_dir)
        .with_context(|| format!("failed to extract {}", archive_path.display()))?;

    // ── Read .PKGINFO if present ────────────────────────────────────
    let pkginfo_path = extract_dir.join(".PKGINFO");
    let pkginfo_content = if pkginfo_path.exists() {
        Some(std::fs::read_to_string(&pkginfo_path).with_context(|| "failed to read .PKGINFO")?)
    } else {
        None
    };

    // ── Run lint checks ─────────────────────────────────────────────
    println!("==> Linting {}", archive_path.display());

    let result = lint_package(extract_dir, pkginfo_content.as_deref(), args.strict)
        .with_context(|| "lint checks failed")?;

    let report = format_report(&result, ReportFormat::Human);
    print!("{report}");

    if result.has_errors() {
        anyhow::bail!(
            "lint failed with {} error(s)",
            result.count(xpkg_core::lint::Severity::Error)
        );
    }

    if !result.has_warnings() && result.total() == 0 {
        println!("==> Package passed all lint checks");
    }

    Ok(())
}

fn cmd_new(args: &cli::NewArgs) -> Result<()> {
    let template = recipe::generate_template(&args.pkgname);

    let outdir = args
        .outdir
        .clone()
        .unwrap_or_else(|| std::path::PathBuf::from(&args.pkgname));

    std::fs::create_dir_all(&outdir)
        .with_context(|| format!("failed to create directory {}", outdir.display()))?;

    let xbuild_path = outdir.join("XBUILD");
    std::fs::write(&xbuild_path, &template)
        .with_context(|| format!("failed to write {}", xbuild_path.display()))?;

    println!("Created {}", xbuild_path.display());
    tracing::info!(path = %xbuild_path.display(), "generated XBUILD template");
    Ok(())
}

fn cmd_srcinfo(args: &cli::SrcinfoArgs) -> Result<()> {
    let path = args
        .file
        .clone()
        .unwrap_or_else(|| std::path::PathBuf::from("XBUILD"));

    let raw_recipe = recipe::parse_xbuild(&path)
        .with_context(|| format!("failed to parse {}", path.display()))?;

    recipe::validate_recipe(&raw_recipe)
        .with_context(|| format!("validation failed for {}", path.display()))?;

    let srcinfo = recipe::generate_srcinfo(&raw_recipe);
    print!("{srcinfo}");
    Ok(())
}

fn cmd_info(args: &cli::InfoArgs) -> Result<()> {
    use xpkg_core::repo::{entry_from_package, list_package_files};

    let package_path = &args.package;
    tracing::info!(path = %package_path.display(), "inspecting package");

    let entry = entry_from_package(package_path)
        .with_context(|| format!("failed to inspect {}", package_path.display()))?;

    // ── JSON output ─────────────────────────────────────────────────
    if args.json {
        let mut info =
            serde_json::to_value(&entry).with_context(|| "failed to serialize package info")?;

        if args.files {
            let files =
                list_package_files(package_path).with_context(|| "failed to list package files")?;
            info["files"] = serde_json::json!(files);
        }

        println!(
            "{}",
            serde_json::to_string_pretty(&info).with_context(|| "failed to format JSON")?
        );
        return Ok(());
    }

    // ── Human-readable output ───────────────────────────────────────
    let w = 18; // label width
    println!("{:w$}: {}", "Name", entry.name);
    println!("{:w$}: {}", "Version", entry.full_version());
    println!("{:w$}: {}", "Description", entry.description);
    println!("{:w$}: {}", "Architecture", entry.arch);

    if !entry.url.is_empty() {
        println!("{:w$}: {}", "URL", entry.url);
    }

    if !entry.license.is_empty() {
        println!("{:w$}: {}", "License", entry.license);
    }

    println!(
        "{:w$}: {}",
        "Installed Size",
        format_size(entry.installed_size)
    );
    println!(
        "{:w$}: {}",
        "Compressed Size",
        format_size(entry.compressed_size)
    );

    if entry.build_date > 0 {
        println!(
            "{:w$}: {}",
            "Build Date",
            format_timestamp(entry.build_date)
        );
    }

    if !entry.packager.is_empty() {
        println!("{:w$}: {}", "Packager", entry.packager);
    }

    println!("{:w$}: {}", "SHA-256", entry.sha256sum);

    print_list("Depends On", &entry.depends, w);
    print_list("Make Depends", &entry.makedepends, w);
    print_list("Check Depends", &entry.checkdepends, w);
    print_list("Optional Deps", &entry.optdepends, w);
    print_list("Provides", &entry.provides, w);
    print_list("Conflicts With", &entry.conflicts, w);
    print_list("Replaces", &entry.replaces, w);

    // ── File listing ────────────────────────────────────────────────
    if args.files {
        let files =
            list_package_files(package_path).with_context(|| "failed to list package files")?;
        println!();
        println!("{:w$}: {}", "Files", files.len());
        for f in &files {
            println!("  {f}");
        }
    }

    Ok(())
}

// ── Formatting helpers ──────────────────────────────────────────────────────

fn format_size(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * KIB;
    const GIB: u64 = 1024 * MIB;

    if bytes >= GIB {
        format!("{:.1} GiB", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:.1} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{bytes} B")
    }
}

fn format_timestamp(unix_secs: u64) -> String {
    // Simple UTC formatting without pulling in chrono.
    let secs = unix_secs as i64;
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Days since Unix epoch → year/month/day (civil calendar).
    let (y, m, d) = days_to_civil(days + 719_468);
    format!("{y:04}-{m:02}-{d:02} {hours:02}:{minutes:02}:{seconds:02} UTC")
}

/// Convert a day count (with epoch offset) to (year, month, day).
/// Algorithm from Howard Hinnant's `chrono`-compatible date library.
fn days_to_civil(day_count: i64) -> (i64, u32, u32) {
    let era = day_count.div_euclid(146_097);
    let doe = day_count.rem_euclid(146_097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

fn print_list(label: &str, items: &[String], w: usize) {
    if items.is_empty() {
        println!("{label:w$}: None");
    } else {
        println!("{label:w$}: {}", items.join("  "));
    }
}

fn cmd_verify(args: &cli::VerifyArgs) -> Result<()> {
    use xpkg_core::signing::{load_cert, load_keyring, verify_file, VerifyOutcome};

    let package_path = &args.package;
    let sig_path = sig_path_for(package_path);

    if !sig_path.exists() {
        anyhow::bail!("signature file not found: {}", sig_path.display());
    }

    let certs = match &args.key {
        Some(key_path) => {
            // Try as keyring first, fall back to single cert.
            load_keyring(key_path).or_else(|_| load_cert(key_path).map(|c| vec![c]))?
        }
        None => {
            anyhow::bail!("no public key specified; use --key <path> to provide one");
        }
    };

    println!("==> Verifying {}", package_path.display());

    let outcome = verify_file(package_path, &sig_path, &certs)
        .with_context(|| "signature verification failed")?;

    match outcome {
        VerifyOutcome::Good { key_id } => {
            println!("    ✓ Valid signature (key {key_id})");
        }
        VerifyOutcome::UnknownKey => {
            anyhow::bail!("signature made by an unknown key");
        }
        VerifyOutcome::Bad { reason } => {
            anyhow::bail!("bad signature: {reason}");
        }
    }

    Ok(())
}

fn cmd_repo_add(config: &XpkgConfig, args: &cli::RepoAddArgs) -> Result<()> {
    use xpkg_core::repo::{
        add_entry, entry_from_package, history_entry_from_package, history_path,
        list_package_files, prune_repo, read_db, read_history, upsert_files_entry,
        upsert_history_entry, write_db, write_history,
    };

    let db_path = &args.db;
    let package_path = &args.package;

    tracing::info!(
        db = %db_path.display(),
        package = %package_path.display(),
        "adding package to repository"
    );

    // Derive repo name from the db path (e.g. "myrepo.db.tar.zst" → "myrepo").
    let repo_name = db_path
        .file_name()
        .and_then(|f| f.to_str())
        .and_then(|f| f.split('.').next())
        .unwrap_or("repo");

    let mut db =
        read_db(db_path, repo_name).with_context(|| "failed to read repository database")?;

    let entry = entry_from_package(package_path)
        .with_context(|| format!("failed to inspect {}", package_path.display()))?;

    let pkg_display = format!("{}-{}", entry.name, entry.full_version());
    let dir_name = entry.dir_name();
    let pkg_files = list_package_files(package_path)
        .with_context(|| format!("failed to list files in {}", package_path.display()))?;

    // ── Update the history index ────────────────────────────────────
    let repo_dir = db_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let hist_path = history_path(db_path);

    let mut history = read_history(&hist_path, repo_name)
        .with_context(|| format!("failed to read {}", hist_path.display()))?;
    if history.arch.is_empty() {
        history.arch = entry.arch.clone();
    }

    let history_entry = history_entry_from_package(package_path, &entry, repo_dir)
        .with_context(|| "failed to extract package provenance")?;
    upsert_history_entry(&mut history, &entry.name, history_entry);

    add_entry(&mut db, entry);
    write_db(&db).with_context(|| "failed to write repository database")?;

    // Keep the ALPM files database in sync (`xpm files` and similar tools).
    let files_path = upsert_files_entry(db_path, &dir_name, &pkg_files, &db)
        .with_context(|| "failed to write files database")?;

    // ── Version retention (optional) ────────────────────────────────
    if args.keep > 0 {
        let report = prune_repo(&db, &mut history, repo_dir, args.keep, false)
            .with_context(|| "failed to prune old package versions")?;

        for file in &report.deleted_files {
            println!("    Removed {}", file.display());
        }
        if report.is_empty() {
            println!("    Retention: nothing to prune (keep {})", args.keep);
        }
    }

    write_history(&hist_path, &history)
        .with_context(|| format!("failed to write {}", hist_path.display()))?;

    println!("==> Added {pkg_display} to {}", db_path.display());
    println!("    Repository now contains {} package(s)", db.len());
    println!(
        "    Files database: {} ({} path(s) for {})",
        files_path.display(),
        pkg_files.len(),
        dir_name
    );
    println!(
        "    History: {} ({} version(s) tracked)",
        hist_path.display(),
        history.total_versions()
    );

    // ── Sign database (optional) ────────────────────────────────────
    if args.sign {
        use xpkg_core::signing::{load_secret_key, sign_file};

        let key_path = std::path::PathBuf::from(&config.options.sign_key);
        if key_path.as_os_str().is_empty() {
            anyhow::bail!("--sign requires a signing key; set sign_key in xpkg.conf");
        }

        let secret_key =
            load_secret_key(&key_path).with_context(|| "failed to load signing key")?;
        let sig =
            sign_file(db_path, &secret_key, false).with_context(|| "failed to sign database")?;

        println!(
            "    Database signed: {} (key {})",
            sig.sig_path.display(),
            sig.key_id
        );
    }

    // ── Sign history index (optional) ───────────────────────────────
    sign_history(config, &hist_path, args.sign)?;

    // ── Sign files database (optional) ──────────────────────────────
    sign_artifact(config, &files_path, args.sign, "Files database")?;

    Ok(())
}

fn cmd_repo_prune(config: &XpkgConfig, args: &cli::RepoPruneArgs) -> Result<()> {
    use xpkg_core::repo::{
        history_path, prune_repo, read_db, read_history, seed_history_from_db, sync_files_with_db,
        write_history,
    };

    let db_path = &args.db;

    tracing::info!(
        db = %db_path.display(),
        keep = args.keep,
        dry_run = args.dry_run,
        "pruning repository versions"
    );

    let repo_name = db_path
        .file_name()
        .and_then(|f| f.to_str())
        .and_then(|f| f.split('.').next())
        .unwrap_or("repo");

    let repo_dir = db_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let hist_path = history_path(db_path);

    let db = read_db(db_path, repo_name).with_context(|| "failed to read repository database")?;

    let mut history = read_history(&hist_path, repo_name)
        .with_context(|| format!("failed to read {}", hist_path.display()))?;
    if history.arch.is_empty() {
        history.arch = db
            .entries
            .values()
            .next()
            .map(|e| e.arch.clone())
            .unwrap_or_default();
    }

    let seeded = seed_history_from_db(&mut history, &db, repo_dir);
    if seeded > 0 {
        tracing::info!(seeded, "indexed existing versions into history");
    }

    let report = prune_repo(&db, &mut history, repo_dir, args.keep, args.dry_run)
        .with_context(|| "failed to prune old package versions")?;

    if args.dry_run {
        println!(
            "==> Dry run: {} version(s) would be removed from {}",
            report.removed.len(),
            db_path.display()
        );
        for version in &report.removed {
            println!(
                "    Would remove {} ({})",
                version.filename, version.version
            );
        }
        println!("    {} version(s) would be kept", report.kept);
        return Ok(());
    }

    write_history(&hist_path, &history)
        .with_context(|| format!("failed to write {}", hist_path.display()))?;

    println!(
        "==> Pruned {} version(s) from {}",
        report.removed.len(),
        db_path.display()
    );
    for version in &report.removed {
        println!("    Removed {} ({})", version.filename, version.version);
    }
    println!(
        "    {} version(s) kept; history: {}",
        report.kept,
        hist_path.display()
    );

    // Old versions are gone from disk; drop their files-database entries.
    let _ = sync_files_with_db(db_path, &db).with_context(|| "failed to update files database")?;

    sign_history(config, &hist_path, false)?;

    Ok(())
}

fn cmd_repo_remove(config: &XpkgConfig, args: &cli::RepoRemoveArgs) -> Result<()> {
    use xpkg_core::repo::{
        history_path, read_db, read_history, remove_entry, sync_files_with_db,
        sync_history_with_db, write_db, write_history,
    };

    let db_path = &args.db;
    let pkgname = &args.pkgname;

    tracing::info!(db = %db_path.display(), package = %pkgname, "removing package from repository");

    let repo_name = db_path
        .file_name()
        .and_then(|f| f.to_str())
        .and_then(|f| f.split('.').next())
        .unwrap_or("repo");

    let mut db =
        read_db(db_path, repo_name).with_context(|| "failed to read repository database")?;

    match remove_entry(&mut db, pkgname) {
        Some(_) => {
            write_db(&db).with_context(|| "failed to write repository database")?;
            println!("==> Removed {pkgname} from {}", db_path.display());
            println!("    Repository now contains {} package(s)", db.len());

            if let Some(files_path) = sync_files_with_db(db_path, &db)
                .with_context(|| "failed to update files database")?
            {
                println!("    Files database: {} updated", files_path.display());
                sign_artifact(config, &files_path, args.sign, "Files database")?;
            }

            // Keep history.json coherent: drop versions whose file is gone and
            // packages the database no longer lists (otherwise consumers would
            // try to resolve downgrades to unserved artifacts).
            let hist_path = history_path(db_path);
            if hist_path.exists() {
                let repo_dir = db_path
                    .parent()
                    .unwrap_or_else(|| std::path::Path::new("."));
                let mut history = read_history(&hist_path, repo_name)
                    .with_context(|| format!("failed to read {}", hist_path.display()))?;
                let removed = sync_history_with_db(&mut history, &db, repo_dir);
                write_history(&hist_path, &history)
                    .with_context(|| format!("failed to write {}", hist_path.display()))?;
                if removed > 0 {
                    println!(
                        "    {removed} stale version(s) dropped from {}",
                        hist_path.display()
                    );
                }
                sign_history(config, &hist_path, args.sign)?;
            }
        }
        None => {
            anyhow::bail!("package '{pkgname}' not found in {}", db_path.display());
        }
    }

    Ok(())
}

// ── Signing helpers ─────────────────────────────────────────────────────────

/// Path of the detached signature for a file (`<file>.sig`).
fn sig_path_for(path: &std::path::Path) -> std::path::PathBuf {
    path.with_extension(format!(
        "{}.sig",
        path.extension().unwrap_or_default().to_string_lossy()
    ))
}

/// Sign `history.json` when a signing key is available.
///
/// Signing happens when `--sign` was passed or when `sign_key` is configured.
/// Without a secret key a signature cannot be produced; in that case an
/// existing (now stale) `history.json.sig` is removed with a warning, since
/// the index content has changed and the old signature no longer verifies.
fn sign_history(config: &XpkgConfig, hist_path: &std::path::Path, explicit: bool) -> Result<()> {
    sign_artifact(config, hist_path, explicit, "History")
}

/// Sign an artifact when a signing key is available.
///
/// Signing happens when `explicit` is set or when `sign_key` is configured.
/// Without a secret key a signature cannot be produced; in that case an
/// existing (now stale) `.sig` is removed with a warning, since the content
/// changed and the old signature no longer verifies.
fn sign_artifact(
    config: &XpkgConfig,
    path: &std::path::Path,
    explicit: bool,
    label: &str,
) -> Result<()> {
    use xpkg_core::signing::{load_secret_key, sign_file};

    let key_path = std::path::PathBuf::from(&config.options.sign_key);
    if key_path.as_os_str().is_empty() {
        if explicit {
            anyhow::bail!("--sign requires a signing key; set sign_key in xpkg.conf");
        }
        remove_stale_signature(path)?;
        return Ok(());
    }

    match load_secret_key(&key_path) {
        Ok(secret_key) => {
            let sig = sign_file(path, &secret_key, false)
                .with_context(|| format!("failed to sign {label}"))?;
            println!(
                "    {label} signed: {} (key {})",
                sig.sig_path.display(),
                sig.key_id
            );
        }
        Err(e) if explicit => {
            return Err(e).with_context(|| "failed to load signing key");
        }
        Err(e) => {
            eprintln!("warning: could not sign {}: {e}", path.display());
            remove_stale_signature(path)?;
        }
    }

    Ok(())
}

fn remove_stale_signature(hist_path: &std::path::Path) -> Result<()> {
    let sig_path = sig_path_for(hist_path);
    if sig_path.exists() {
        std::fs::remove_file(&sig_path)
            .with_context(|| format!("failed to remove stale signature {}", sig_path.display()))?;
        eprintln!(
            "warning: {} changed without a signing key; removed stale {}",
            hist_path.display(),
            sig_path.display()
        );
    }
    Ok(())
}
