//! PKGBUILD parser — extract variables and functions from Arch Linux PKGBUILDs.
//!
//! This is a best-effort parser for the subset of bash used in PKGBUILDs.
//! It does NOT evaluate shell code — it extracts known variable assignments
//! and function bodies via pattern matching.

use crate::error::XpkgError;
use crate::recipe::types::{
    BuildSection, DependencySection, PackageSection, Recipe, SourceSection,
};

/// Parse a PKGBUILD file from a path.
pub fn parse_pkgbuild(path: &std::path::Path) -> Result<Recipe, XpkgError> {
    let contents = std::fs::read_to_string(path)
        .map_err(|e| XpkgError::RecipeParse(format!("failed to read {}: {e}", path.display())))?;

    parse_pkgbuild_str(&contents)
}

/// Parse a PKGBUILD from a string.
pub fn parse_pkgbuild_str(input: &str) -> Result<Recipe, XpkgError> {
    let pkgname = extract_var(input, "pkgname").ok_or_else(|| {
        XpkgError::RecipeParse("PKGBUILD missing required variable: pkgname".into())
    })?;
    let pkgver = extract_var(input, "pkgver").ok_or_else(|| {
        XpkgError::RecipeParse("PKGBUILD missing required variable: pkgver".into())
    })?;
    let pkgrel = extract_var(input, "pkgrel")
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(1);
    let pkgdesc = extract_var(input, "pkgdesc").unwrap_or_default();
    let url = extract_var(input, "url");

    let license = extract_array(input, "license");
    let arch = extract_array(input, "arch");
    let provides = extract_array(input, "provides");
    let conflicts = extract_array(input, "conflicts");
    let replaces = extract_array(input, "replaces");
    let backup = extract_array(input, "backup");

    let depends = extract_array(input, "depends");
    let makedepends = extract_array(input, "makedepends");
    let checkdepends = extract_array(input, "checkdepends");
    let optdepends = extract_array(input, "optdepends");

    let source: Vec<String> = extract_array(input, "source")
        .into_iter()
        .map(|s| {
            expand_pkgvars(
                &s,
                &pkgname,
                &pkgver,
                pkgrel,
                url.as_deref().unwrap_or_default(),
            )
        })
        .collect();
    let sha256sums = extract_array(input, "sha256sums");
    let sha512sums = extract_array(input, "sha512sums");

    let prepare = extract_function(input, "prepare").unwrap_or_default();
    let build = extract_function(input, "build").unwrap_or_default();
    let check = extract_function(input, "check").unwrap_or_default();
    let package = extract_function(input, "package").unwrap_or_default();

    Ok(Recipe {
        package: PackageSection {
            name: pkgname,
            version: pkgver,
            release: pkgrel,
            description: pkgdesc,
            url,
            license,
            arch,
            provides,
            conflicts,
            replaces,
            backup,
        },
        dependencies: DependencySection {
            depends,
            makedepends,
            checkdepends,
            optdepends,
        },
        source: SourceSection {
            urls: source,
            sha256sums,
            sha512sums,
            patches: Vec::new(),
        },
        build: BuildSection {
            prepare,
            build,
            check,
            package,
        },
    })
}

// ── Extraction helpers ──────────────────────────────────────────────────────

/// Extract a simple variable: `varname=value` or `varname="value"` or `varname='value'`.
fn extract_var(input: &str, name: &str) -> Option<String> {
    for line in input.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix(name) {
            if let Some(value) = rest.strip_prefix('=') {
                return Some(unquote(value.trim()));
            }
        }
    }
    None
}

/// Extract a bash array: `varname=(elem1 elem2 ...)` — potentially multi-line.
fn extract_array(input: &str, name: &str) -> Vec<String> {
    let pattern = format!("{name}=(");
    let mut collecting = false;
    let mut buf = String::new();

    for line in input.lines() {
        let trimmed = line.trim();

        if !collecting {
            if let Some(rest) = trimmed.strip_prefix(&pattern) {
                collecting = true;
                buf.push_str(rest);
                // Single-line array: closes on same line
                if buf.contains(')') {
                    break;
                }
            }
        } else {
            buf.push(' ');
            buf.push_str(trimmed);
            if trimmed.contains(')') {
                break;
            }
        }
    }

    if buf.is_empty() {
        return Vec::new();
    }

    // Remove trailing )
    if let Some(idx) = buf.rfind(')') {
        buf.truncate(idx);
    }

    parse_array_elements(&buf)
}

/// Parse space-separated, possibly quoted elements from a bash array body.
fn parse_array_elements(input: &str) -> Vec<String> {
    let mut elements = Vec::new();
    let mut chars = input.chars().peekable();

    while let Some(&c) = chars.peek() {
        match c {
            ' ' | '\t' | '\n' => {
                chars.next();
            }
            '\'' => {
                chars.next(); // consume opening quote
                let mut elem = String::new();
                for ch in chars.by_ref() {
                    if ch == '\'' {
                        break;
                    }
                    elem.push(ch);
                }
                if !elem.is_empty() {
                    elements.push(elem);
                }
            }
            '"' => {
                chars.next(); // consume opening quote
                let mut elem = String::new();
                for ch in chars.by_ref() {
                    if ch == '"' {
                        break;
                    }
                    elem.push(ch);
                }
                if !elem.is_empty() {
                    elements.push(elem);
                }
            }
            _ => {
                let mut elem = String::new();
                while let Some(&ch) = chars.peek() {
                    if ch == ' ' || ch == '\t' || ch == '\n' || ch == ')' {
                        break;
                    }
                    elem.push(ch);
                    chars.next();
                }
                if !elem.is_empty() {
                    elements.push(elem);
                }
            }
        }
    }

    elements
}

/// Extract a function body: `funcname() { ... }`.
///
/// Uses brace counting to handle nested braces.
fn extract_function(input: &str, name: &str) -> Option<String> {
    let patterns = [format!("{name}() {{"), format!("{name}()\n{{")];

    let start_pos = patterns
        .iter()
        .filter_map(|pat| input.find(pat.as_str()))
        .min()?;

    // Find the opening brace
    let after_name = &input[start_pos..];
    let brace_start = after_name.find('{')?;
    let body_start = start_pos + brace_start + 1;

    let mut depth = 1;
    let mut end = body_start;

    for (i, ch) in input[body_start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = body_start + i;
                    break;
                }
            }
            _ => {}
        }
    }

    let body = input[body_start..end].trim().to_string();
    if body.is_empty() {
        None
    } else {
        Some(body)
    }
}

/// Remove surrounding single or double quotes from a value.
fn unquote(s: &str) -> String {
    let s = s.trim();
    if (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')) {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

/// Expand the common makepkg variables in source entries
/// (`$pkgname`, `$pkgver`, `$pkgrel`, `$url`; braced or bare).
fn expand_pkgvars(s: &str, pkgname: &str, pkgver: &str, pkgrel: u32, url: &str) -> String {
    let rel = pkgrel.to_string();
    let mut out = s.to_string();
    for (pat, val) in [
        ("${pkgname}", pkgname),
        ("${pkgver}", pkgver),
        ("${pkgrel}", rel.as_str()),
        ("${url}", url),
    ] {
        out = out.replace(pat, val);
    }
    for (pat, val) in [
        ("$pkgname", pkgname),
        ("$pkgver", pkgver),
        ("$pkgrel", rel.as_str()),
        ("$url", url),
    ] {
        out = out.replace(pat, val);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_PKGBUILD: &str = r#"
# Maintainer: Someone <someone@example.com>
pkgname=hello
pkgver=2.12
pkgrel=1
pkgdesc="GNU Hello — the friendly greeter"
arch=('x86_64')
url="https://www.gnu.org/software/hello/"
license=('GPL-3.0-or-later')
depends=('glibc')
makedepends=('texinfo')
source=("https://ftp.gnu.org/gnu/hello/hello-${pkgver}.tar.gz"
        "fix-tests.patch")
sha256sums=('cf04af86dc085268c5f4470fbae49b18afbc221b78096aab842d934a76bad0ab'
            'SKIP')

prepare() {
    cd hello-${pkgver}
    patch -p1 < ../fix-tests.patch
}

build() {
    cd hello-${pkgver}
    ./configure --prefix=/usr
    make
}

check() {
    cd hello-${pkgver}
    make check
}

package() {
    cd hello-${pkgver}
    make DESTDIR="$pkgdir" install
}
"#;

    #[test]
    fn test_parse_pkgbuild_variables() {
        let recipe = parse_pkgbuild_str(SAMPLE_PKGBUILD).unwrap();
        assert_eq!(recipe.package.name, "hello");
        assert_eq!(recipe.package.version, "2.12");
        assert_eq!(recipe.package.release, 1);
        assert_eq!(
            recipe.package.description,
            "GNU Hello — the friendly greeter"
        );
        assert_eq!(
            recipe.package.url.as_deref(),
            Some("https://www.gnu.org/software/hello/")
        );
        assert_eq!(recipe.package.license, vec!["GPL-3.0-or-later"]);
        assert_eq!(recipe.package.arch, vec!["x86_64"]);
    }

    #[test]
    fn test_parse_pkgbuild_arrays() {
        let recipe = parse_pkgbuild_str(SAMPLE_PKGBUILD).unwrap();
        assert_eq!(recipe.dependencies.depends, vec!["glibc"]);
        assert_eq!(recipe.dependencies.makedepends, vec!["texinfo"]);
        assert_eq!(recipe.source.urls.len(), 2);
        assert_eq!(recipe.source.sha256sums.len(), 2);
    }

    #[test]
    fn test_parse_pkgbuild_functions() {
        let recipe = parse_pkgbuild_str(SAMPLE_PKGBUILD).unwrap();
        assert!(recipe.build.prepare.contains("patch -p1"));
        assert!(recipe.build.build.contains("./configure"));
        assert!(recipe.build.build.contains("make"));
        assert!(recipe.build.check.contains("make check"));
        assert!(recipe.build.package.contains("DESTDIR"));
    }

    #[test]
    fn test_source_vars_expanded() {
        let input = "pkgname=demo\npkgver=1.2\npkgrel=3\nurl=\"https://example.com\"\n\
                     source=(\"${pkgname}-${pkgver}.tar.gz::${url}/dl/v${pkgver}/demo.tar.gz\"\n\
                     'local-$pkgrel.txt')\n";
        let recipe = parse_pkgbuild_str(input).unwrap();
        assert_eq!(
            recipe.source.urls[0],
            "demo-1.2.tar.gz::https://example.com/dl/v1.2/demo.tar.gz"
        );
        assert_eq!(recipe.source.urls[1], "local-3.txt");
    }

    #[test]
    fn test_parse_pkgbuild_missing_name() {
        let result = parse_pkgbuild_str("pkgver=1.0\n");
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_var_quoted() {
        let input = "pkgdesc=\"A cool package\"\n";
        assert_eq!(extract_var(input, "pkgdesc"), Some("A cool package".into()));
    }

    #[test]
    fn test_extract_var_single_quoted() {
        let input = "license='MIT'\n";
        assert_eq!(extract_var(input, "license"), Some("MIT".into()));
    }

    #[test]
    fn test_extract_array_single_line() {
        let input = "depends=('foo' 'bar' 'baz')\n";
        assert_eq!(extract_array(input, "depends"), vec!["foo", "bar", "baz"]);
    }

    #[test]
    fn test_extract_array_multiline() {
        let input = "source=('url1'\n        'url2'\n        'url3')\n";
        assert_eq!(extract_array(input, "source"), vec!["url1", "url2", "url3"]);
    }

    #[test]
    fn test_parse_backup_array() {
        let input = "pkgname=hello\npkgver=1.0\nbackup=('etc/hello.conf' 'etc/other.conf')\n";
        let recipe = parse_pkgbuild_str(input).expect("parse");
        assert_eq!(
            recipe.package.backup,
            vec!["etc/hello.conf", "etc/other.conf"]
        );
    }
}
