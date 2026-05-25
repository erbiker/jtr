use anyhow::{Context, Result, anyhow, bail};
use colored::Colorize;
use std::fs;
use std::path::{Path, PathBuf};

use crate::index::sha256_hex;
use crate::manifest::{IndexFile, RecipeManifest};

pub fn run(path: PathBuf, tap_mode: bool, fix: bool) -> Result<()> {
    if fix && !tap_mode {
        bail!(
            "--fix requires --tap (checksums live in index.json, not in individual manifests). \
             Run `jtr lint --tap <tap-root> --fix` against the directory containing index.json."
        );
    }
    if tap_mode {
        lint_tap(&path, fix)
    } else {
        lint_manifest(&path)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone)]
struct Finding {
    scope: String,
    severity: Severity,
    message: String,
}

impl Finding {
    fn error(scope: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            scope: scope.into(),
            severity: Severity::Error,
            message: message.into(),
        }
    }
    fn warning(scope: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            scope: scope.into(),
            severity: Severity::Warning,
            message: message.into(),
        }
    }
}

fn lint_manifest(path: &Path) -> Result<()> {
    let display = path.display().to_string();
    let bytes = fs::read(path).with_context(|| format!("could not read {display}"))?;
    let findings = check_manifest_bytes(&format!("manifest {display}"), &bytes);
    print_findings(&findings);
    if findings.iter().any(|f| f.severity == Severity::Error) {
        std::process::exit(1);
    }
    println!("{} {} passed lint", "✓".green(), display.bold());
    Ok(())
}

fn lint_tap(root: &Path, fix: bool) -> Result<()> {
    let index_path = root.join("index.json");
    if !index_path.is_file() {
        bail!(
            "no index.json in '{}' — pass a tap root directory containing an index.json",
            root.display()
        );
    }

    let index_text = fs::read_to_string(&index_path)
        .with_context(|| format!("could not read {}", index_path.display()))?;
    let parsed: IndexFile = match serde_json::from_str(&index_text) {
        Ok(p) => p,
        Err(e) => {
            print_findings(&[Finding::error(
                format!("index {}", index_path.display()),
                format!("not valid JSON: {e}"),
            )]);
            std::process::exit(1);
        }
    };

    let mut findings: Vec<Finding> = Vec::new();
    if parsed.version != 1 {
        findings.push(Finding::error(
            format!("index {}", index_path.display()),
            format!("unsupported version {} (jtr supports v1)", parsed.version),
        ));
    }

    let mut seen_names: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut planned_fixes: Vec<ChecksumFix> = Vec::new();

    for entry in &parsed.recipes {
        let scope = format!("recipe '{}'", entry.name);
        if !seen_names.insert(entry.name.clone()) {
            findings.push(Finding::error(
                &scope,
                format!("duplicate entry for '{}' in index.json", entry.name),
            ));
            continue;
        }

        if entry.manifest_url.contains("://") {
            findings.push(Finding::warning(
                &scope,
                format!(
                    "manifest_url '{}' is absolute; lint can only validate local relative paths",
                    entry.manifest_url
                ),
            ));
            continue;
        }
        let manifest_path = root.join(&entry.manifest_url);
        let bytes = match fs::read(&manifest_path) {
            Ok(b) => b,
            Err(e) => {
                findings.push(Finding::error(
                    &scope,
                    format!(
                        "could not read manifest at '{}': {e}",
                        manifest_path.display()
                    ),
                ));
                continue;
            }
        };

        findings.extend(check_manifest_bytes(&scope, &bytes));

        if let Ok(manifest) = serde_json::from_slice::<RecipeManifest>(&bytes) {
            if manifest.name != entry.name {
                findings.push(Finding::error(
                    &scope,
                    format!(
                        "manifest name '{}' does not match index entry name '{}'",
                        manifest.name, entry.name
                    ),
                ));
            }
            if manifest.version != entry.version {
                findings.push(Finding::error(
                    &scope,
                    format!(
                        "manifest version '{}' does not match index entry version '{}'",
                        manifest.version, entry.version
                    ),
                ));
            }
            if manifest.description != entry.description {
                findings.push(Finding::warning(
                    &scope,
                    "manifest description differs from index entry description",
                ));
            }
        }

        let actual = sha256_hex(&bytes);
        match entry.sha256.as_deref() {
            Some(existing) if existing == actual => {}
            Some(existing) => {
                if fix {
                    planned_fixes.push(ChecksumFix {
                        manifest_url: entry.manifest_url.clone(),
                        old: Some(existing.to_string()),
                        new: actual,
                    });
                } else {
                    findings.push(Finding::error(
                        &scope,
                        format!(
                            "sha256 mismatch (index: {}, computed: {}). \
                             Run `jtr lint --tap {} --fix` to repair.",
                            existing,
                            actual,
                            root.display()
                        ),
                    ));
                }
            }
            None => {
                if fix {
                    planned_fixes.push(ChecksumFix {
                        manifest_url: entry.manifest_url.clone(),
                        old: None,
                        new: actual,
                    });
                } else {
                    findings.push(Finding::error(
                        &scope,
                        format!(
                            "missing sha256 in index entry (computed: {}). \
                             Run `jtr lint --tap {} --fix` to add it.",
                            actual,
                            root.display()
                        ),
                    ));
                }
            }
        }
    }

    print_findings(&findings);

    if fix && !planned_fixes.is_empty() {
        let updated = apply_checksum_fixes(&index_text, &planned_fixes)?;
        fs::write(&index_path, updated)
            .with_context(|| format!("could not write {}", index_path.display()))?;
        for fix in &planned_fixes {
            let label = if fix.old.is_some() {
                "updated sha256"
            } else {
                "added sha256"
            };
            println!(
                "{} {} for {} → {}",
                "✓".green(),
                label,
                fix.manifest_url.bold(),
                short_sha(&fix.new).dimmed()
            );
        }
    }

    let errors = findings
        .iter()
        .filter(|f| f.severity == Severity::Error)
        .count();
    if errors > 0 {
        std::process::exit(1);
    }

    let warnings = findings
        .iter()
        .filter(|f| f.severity == Severity::Warning)
        .count();
    let summary_recipes = parsed.recipes.len();
    if warnings == 0 {
        println!(
            "\n{} {} ({} recipes) passed lint",
            "✓".green(),
            index_path.display().to_string().bold(),
            summary_recipes
        );
    } else {
        println!(
            "\n{} {} passed lint with {} warning(s)",
            "✓".green(),
            index_path.display().to_string().bold(),
            warnings
        );
    }
    Ok(())
}

struct ChecksumFix {
    manifest_url: String,
    /// Existing value in the index, if any. `None` means we're inserting a new field.
    old: Option<String>,
    new: String,
}

fn check_manifest_bytes(scope: &str, bytes: &[u8]) -> Vec<Finding> {
    let mut findings = Vec::new();

    let raw_text = match std::str::from_utf8(bytes) {
        Ok(s) => s,
        Err(e) => {
            findings.push(Finding::error(scope, format!("not valid UTF-8: {e}")));
            return findings;
        }
    };

    let manifest: RecipeManifest = match serde_json::from_str(raw_text) {
        Ok(m) => m,
        Err(e) => {
            findings.push(Finding::error(scope, format!("schema error: {e}")));
            return findings;
        }
    };

    if manifest.name.is_empty() {
        findings.push(Finding::error(scope, "`name` is empty"));
    }
    if let Err(e) = crate::managed::validate_name(&manifest.name) {
        findings.push(Finding::error(
            scope,
            format!("invalid `name`: {:#}", anyhow::anyhow!(e)),
        ));
    }
    if manifest.version.is_empty() {
        findings.push(Finding::error(scope, "`version` is empty"));
    }
    if manifest.description.is_empty() {
        findings.push(Finding::warning(scope, "`description` is empty"));
    } else if manifest.description.starts_with("TODO") {
        findings.push(Finding::warning(
            scope,
            "`description` still contains a `TODO` placeholder",
        ));
    }

    if manifest.targets.is_empty() {
        findings.push(Finding::error(
            scope,
            "`targets` is empty — at least one of `just` or `task` is required",
        ));
    }

    for tool in &manifest.shells_out_to {
        if which(tool).is_none() {
            findings.push(Finding::warning(
                scope,
                format!(
                    "`shells_out_to` lists '{tool}', but it isn't on PATH — \
                     install it locally to verify the recipe actually runs"
                ),
            ));
        }
    }

    for (target_name, target) in &manifest.targets {
        if target_name != "just" && target_name != "task" {
            findings.push(Finding::warning(
                scope,
                format!("unknown target '{target_name}' (jtr only renders `just` and `task`)"),
            ));
            continue;
        }
        if target.snippet.trim().is_empty() {
            findings.push(Finding::error(
                scope,
                format!("`targets.{target_name}.snippet` is empty"),
            ));
            continue;
        }
        match validate_snippet(target_name, &target.snippet) {
            SnippetCheck::Ok => {}
            SnippetCheck::Skipped(reason) => {
                findings.push(Finding::warning(
                    scope,
                    format!("snippet syntax for target '{target_name}' was not checked: {reason}"),
                ));
            }
            SnippetCheck::Failed(output) => {
                findings.push(Finding::error(
                    scope,
                    format!(
                        "snippet for target '{target_name}' failed to parse:\n{}",
                        indent(&output, "    ")
                    ),
                ));
            }
        }
    }

    findings
}

enum SnippetCheck {
    Ok,
    /// Tool not available; user can install it to enable the check.
    Skipped(String),
    Failed(String),
}

fn validate_snippet(target: &str, snippet: &str) -> SnippetCheck {
    let tool = match target {
        "just" => "just",
        "task" => "task",
        _ => return SnippetCheck::Skipped(format!("unknown target '{target}'")),
    };
    if which(tool).is_none() {
        return SnippetCheck::Skipped(format!(
            "`{tool}` not found on PATH; install it to enable syntax validation"
        ));
    }
    let tmp = match tempfile::tempdir() {
        Ok(d) => d,
        Err(e) => {
            return SnippetCheck::Skipped(format!("could not create temp dir: {e}"));
        }
    };
    let (file, args): (PathBuf, Vec<String>) = match target {
        "just" => {
            let f = tmp.path().join("justfile");
            (
                f.clone(),
                vec![
                    "--justfile".to_string(),
                    f.to_string_lossy().into_owned(),
                    "--list".to_string(),
                ],
            )
        }
        "task" => {
            let f = tmp.path().join("Taskfile.yml");
            (
                f.clone(),
                vec![
                    "-t".to_string(),
                    f.to_string_lossy().into_owned(),
                    "--list-all".to_string(),
                ],
            )
        }
        _ => unreachable!(),
    };
    let body = if target == "just" {
        format!("{}\n", snippet.trim_end_matches('\n'))
    } else {
        format!(
            "version: '3'\n\ntasks:\n{}\n",
            indent(snippet.trim_end_matches('\n'), "  ")
        )
    };
    if let Err(e) = fs::write(&file, &body) {
        return SnippetCheck::Skipped(format!("could not write temp file: {e}"));
    }
    let output = match std::process::Command::new(tool).args(&args).output() {
        Ok(o) => o,
        Err(e) => {
            return SnippetCheck::Skipped(format!("could not run `{tool}`: {e}"));
        }
    };
    if output.status.success() {
        SnippetCheck::Ok
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        let trimmed = stderr.trim();
        let msg = if trimmed.is_empty() {
            format!("`{tool}` exited with status {}", output.status)
        } else {
            trimmed.to_string()
        };
        SnippetCheck::Failed(msg)
    }
}

fn which(tool: &str) -> Option<PathBuf> {
    if tool.is_empty() {
        return None;
    }
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(tool);
        if is_executable(&candidate) {
            return Some(candidate);
        }
    }
    None
}

#[cfg(unix)]
fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    match fs::metadata(p) {
        Ok(meta) => meta.is_file() && meta.permissions().mode() & 0o111 != 0,
        Err(_) => false,
    }
}

#[cfg(not(unix))]
fn is_executable(p: &Path) -> bool {
    p.is_file()
}

fn indent(text: &str, prefix: &str) -> String {
    text.lines()
        .map(|l| format!("{prefix}{l}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn short_sha(sha: &str) -> &str {
    if sha.len() <= 16 { sha } else { &sha[..16] }
}

fn print_findings(findings: &[Finding]) {
    for f in findings {
        let label = match f.severity {
            Severity::Error => "✗".red(),
            Severity::Warning => "!".yellow(),
        };
        println!("{} {}: {}", label, f.scope.bold(), f.message);
    }
}

/// Rewrite `index_text` with each fix applied to its entry's `sha256` field.
/// Preserves the file's existing whitespace and field order: we mutate only
/// the specific `sha256` substring (or insert a new field before the entry's
/// closing brace) rather than re-serializing the whole document. Otherwise
/// `--fix` would produce noisy whole-file diffs every time someone hand-edits
/// the index, which is the failure mode `scripts/recompute-checksums.sh` was
/// careful to avoid.
fn apply_checksum_fixes(index_text: &str, fixes: &[ChecksumFix]) -> Result<String> {
    let mut output = index_text.to_string();
    for fix in fixes {
        output = apply_one_fix(&output, fix)
            .ok_or_else(|| anyhow!("could not splice sha256 for {}", fix.manifest_url))?;
    }
    Ok(output)
}

fn apply_one_fix(text: &str, fix: &ChecksumFix) -> Option<String> {
    let bounds = locate_entry(text, &fix.manifest_url)?;
    let entry_slice = &text[bounds.start..bounds.end];

    if let Some(sha_field) = find_sha_field(entry_slice) {
        let value_start = bounds.start + sha_field.value_start;
        let value_end = bounds.start + sha_field.value_end;
        let mut out = String::with_capacity(text.len() + fix.new.len());
        out.push_str(&text[..value_start]);
        out.push_str(&fix.new);
        out.push_str(&text[value_end..]);
        return Some(out);
    }

    // No existing sha256 field — insert a new one before the closing `}`.
    // bounds.end points one past the brace; preceding excludes the brace so
    // rfind doesn't land on it.
    let close_brace_pos = bounds.end.checked_sub(1)?;
    let preceding = &text[..close_brace_pos];
    let last_field_end = preceding.rfind(|c: char| !c.is_whitespace())?;
    let preceding_byte = text.as_bytes()[last_field_end];
    let needs_comma = preceding_byte != b',';
    let line_start = text[..last_field_end]
        .rfind('\n')
        .map(|i| i + 1)
        .unwrap_or(0);
    let indent: String = text[line_start..]
        .chars()
        .take_while(|c| c.is_whitespace())
        .collect();
    let mut insertion = String::new();
    if needs_comma {
        insertion.push(',');
    }
    insertion.push('\n');
    insertion.push_str(&indent);
    insertion.push_str(&format!("\"sha256\": \"{}\"", fix.new));
    let insertion_point = last_field_end + 1;
    let mut out = String::with_capacity(text.len() + insertion.len());
    out.push_str(&text[..insertion_point]);
    out.push_str(&insertion);
    out.push_str(&text[insertion_point..]);
    Some(out)
}

struct EntryBounds {
    start: usize,
    end: usize,
}

/// Locate the JSON object whose `manifest_url` field equals `target_url`.
/// Returns the byte range from the opening `{` to the matching closing `}`
/// (inclusive of both braces). None if no matching entry is found.
fn locate_entry(text: &str, target_url: &str) -> Option<EntryBounds> {
    let needle = format!("\"manifest_url\": \"{target_url}\"");
    let url_pos = text.find(&needle)?;
    let bytes = text.as_bytes();

    let mut stack: Vec<usize> = Vec::new();
    let mut entry_open: Option<usize> = None;
    let mut in_string = false;
    let mut escape = false;

    for (i, &b) in bytes.iter().enumerate() {
        if entry_open.is_none() && i >= url_pos {
            // First byte at or past the needle locks in the innermost
            // containing object as the entry. The needle's first byte is the
            // opening `"` of `"manifest_url"`, so the stack's top at this
            // point is the entry's own `{`.
            if let Some(&top) = stack.last() {
                entry_open = Some(top);
            } else {
                return None;
            }
        }

        if escape {
            escape = false;
            continue;
        }
        if in_string {
            if b == b'\\' {
                escape = true;
            } else if b == b'"' {
                in_string = false;
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'{' => stack.push(i),
            b'}' => {
                let opened = stack.pop();
                if let (Some(open), Some(opened_pos)) = (entry_open, opened)
                    && opened_pos == open
                {
                    return Some(EntryBounds {
                        start: open,
                        end: i + 1,
                    });
                }
            }
            _ => {}
        }
    }
    None
}

struct ShaField {
    value_start: usize,
    value_end: usize,
}

fn find_sha_field(entry_slice: &str) -> Option<ShaField> {
    let key_pos = entry_slice.find("\"sha256\":")?;
    let after_key = &entry_slice[key_pos..];
    let quote_offset = after_key.find('"')?; // the leading quote of the key
    // Move past `"sha256":` and any whitespace to find the opening `"` of the value.
    let after_colon = &after_key[after_key.find(':')? + 1..];
    let value_quote_rel = after_colon.find('"')?;
    let value_open_abs = key_pos + (after_key.find(':')? + 1) + value_quote_rel + 1;
    let rest = &entry_slice[value_open_abs..];
    let value_close_rel = rest.find('"')?;
    let _ = quote_offset; // silence unused
    Some(ShaField {
        value_start: value_open_abs,
        value_end: value_open_abs + value_close_rel,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_INDEX: &str = "{\n  \"version\": 1,\n  \"recipes\": [\n    {\n      \"name\": \"foo\",\n      \"version\": \"0.1.0\",\n      \"description\": \"x\",\n      \"manifest_url\": \"recipes/foo.json\",\n      \"targets\": [\"just\"],\n      \"sha256\": \"deadbeef\"\n    }\n  ]\n}\n";

    #[test]
    fn locate_entry_finds_brace_boundaries() {
        let bounds = locate_entry(SAMPLE_INDEX, "recipes/foo.json").expect("locate entry");
        assert_eq!(&SAMPLE_INDEX[bounds.start..bounds.start + 1], "{");
        assert_eq!(&SAMPLE_INDEX[bounds.end - 1..bounds.end], "}");
        let slice = &SAMPLE_INDEX[bounds.start..bounds.end];
        assert!(slice.contains("\"name\": \"foo\""));
    }

    #[test]
    fn apply_one_fix_replaces_existing_sha() {
        let new = "a".repeat(64);
        let fix = ChecksumFix {
            manifest_url: "recipes/foo.json".to_string(),
            old: Some("deadbeef".to_string()),
            new: new.clone(),
        };
        let out = apply_one_fix(SAMPLE_INDEX, &fix).expect("apply fix");
        assert!(out.contains(&format!("\"sha256\": \"{new}\"")));
        assert!(!out.contains("deadbeef"));
    }

    #[test]
    fn apply_one_fix_inserts_missing_sha() {
        let without_sha = "{\n  \"version\": 1,\n  \"recipes\": [\n    {\n      \"name\": \"foo\",\n      \"version\": \"0.1.0\",\n      \"description\": \"x\",\n      \"manifest_url\": \"recipes/foo.json\",\n      \"targets\": [\"just\"]\n    }\n  ]\n}\n";
        let new = "b".repeat(64);
        let fix = ChecksumFix {
            manifest_url: "recipes/foo.json".to_string(),
            old: None,
            new: new.clone(),
        };
        let out = apply_one_fix(without_sha, &fix).expect("apply fix");
        assert!(
            out.contains(&format!("\"sha256\": \"{new}\"")),
            "expected sha256 line to be inserted, got: {out}"
        );
        let parsed: serde_json::Value = serde_json::from_str(&out).expect("still valid JSON");
        assert_eq!(
            parsed["recipes"][0]["sha256"],
            serde_json::Value::String(new)
        );
    }

    #[test]
    fn check_manifest_bytes_flags_empty_targets() {
        let bytes = br#"{
          "name": "foo",
          "version": "0.1.0",
          "description": "ok",
          "targets": {}
        }"#;
        let findings = check_manifest_bytes("foo", bytes);
        assert!(
            findings
                .iter()
                .any(|f| f.severity == Severity::Error && f.message.contains("targets"))
        );
    }

    #[test]
    fn check_manifest_bytes_flags_invalid_json() {
        let findings = check_manifest_bytes("x", b"not json");
        assert!(findings.iter().any(|f| f.message.contains("schema error")));
    }
}
