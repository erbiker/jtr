use anyhow::{Result, anyhow, bail};

/// Sentinel-delimited block embedded in the user's justfile/Taskfile.
///
/// Format:
///
/// ```text
/// # >>> jtr:<name>@<version> >>>
/// # source: <homepage_or_index>
/// # do not edit manually; use `jtr update <name>` or `jtr remove <name>`
/// <snippet>
/// # <<< jtr:<name> <<<
/// ```
pub struct ManagedBlock {
    pub name: String,
    pub version: String,
    /// Recipe text between the sentinels. Read by future `jtr update`/`jtr show` commands.
    #[allow(dead_code)]
    pub body: String,
}

pub fn open_marker(name: &str) -> String {
    format!("# >>> jtr:{name}", name = name)
}

pub fn close_marker(name: &str) -> String {
    format!("# <<< jtr:{name} <<<", name = name)
}

pub fn render(name: &str, version: &str, source: Option<&str>, snippet: &str) -> String {
    let source_line = source
        .map(|s| format!("# source: {s}\n", s = s))
        .unwrap_or_default();
    let trimmed = snippet.trim_end_matches('\n');
    format!(
        "# >>> jtr:{name}@{version} >>>\n\
         {source_line}\
         # do not edit manually; use `jtr update {name}` or `jtr remove {name}`\n\
         {body}\n\
         # <<< jtr:{name} <<<\n",
        name = name,
        version = version,
        source_line = source_line,
        body = trimmed,
    )
}

/// Find all installed blocks in the document. Returns `(name, version, body)` triples.
pub fn parse_all(doc: &str) -> Vec<ManagedBlock> {
    let mut out = Vec::new();
    let mut iter = doc.lines().enumerate().peekable();

    while let Some((_, line)) = iter.next() {
        if let Some((name, version)) = parse_open(line) {
            let close = close_marker(&name);
            let mut body_lines: Vec<String> = Vec::new();
            let mut found_close = false;
            for (_, inner) in iter.by_ref() {
                if inner.trim() == close.trim() {
                    found_close = true;
                    break;
                }
                body_lines.push(inner.to_string());
            }
            if found_close {
                let body = strip_internal_header(&body_lines).join("\n");
                out.push(ManagedBlock {
                    name,
                    version,
                    body,
                });
            }
        }
    }

    out
}

/// Remove the block named `<name>`. Returns the new document and whether anything was removed.
///
/// After removal, runs of 2+ consecutive blank lines are collapsed to a single blank line,
/// and trailing blank lines are stripped. This keeps the file tidy across install/remove cycles
/// without accidentally fusing previously-separated regions together.
pub fn remove(doc: &str, name: &str) -> (String, bool) {
    let open_prefix = open_marker(name);
    let close = close_marker(name);

    let lines: Vec<&str> = doc.lines().collect();
    let mut without_block: Vec<&str> = Vec::with_capacity(lines.len());
    let mut in_block = false;
    let mut removed = false;

    for line in &lines {
        if !in_block && line.trim_start().starts_with(&open_prefix) {
            in_block = true;
            removed = true;
            continue;
        }
        if in_block {
            if line.trim() == close.trim() {
                in_block = false;
            }
            continue;
        }
        without_block.push(line);
    }

    let mut collapsed: Vec<&str> = Vec::with_capacity(without_block.len());
    let mut last_was_blank = false;
    for line in without_block {
        let is_blank = line.trim().is_empty();
        if is_blank && last_was_blank {
            continue;
        }
        collapsed.push(line);
        last_was_blank = is_blank;
    }

    while collapsed.last().is_some_and(|l| l.trim().is_empty()) {
        collapsed.pop();
    }

    let mut joined = collapsed.join("\n");
    if !joined.is_empty() && doc.ends_with('\n') && !joined.ends_with('\n') {
        joined.push('\n');
    }
    (joined, removed)
}

/// Append a rendered block to the document, ensuring a blank line separates it from prior content.
pub fn append(doc: &str, rendered: &str) -> String {
    if doc.is_empty() {
        return rendered.to_string();
    }

    let needs_blank = !doc.ends_with("\n\n");
    let mut out = doc.to_string();
    if !out.ends_with('\n') {
        out.push('\n');
    }
    if needs_blank {
        out.push('\n');
    }
    out.push_str(rendered);
    out
}

/// Install or replace: if a block by this name already exists, swap it in place; otherwise append.
pub fn upsert(doc: &str, name: &str, rendered: &str) -> Result<String> {
    let (without, _) = remove(doc, name);
    let merged = append(&without, rendered);

    // Sanity check: the round-trip parse should now contain exactly one block for `name`.
    let blocks = parse_all(&merged);
    let count = blocks.iter().filter(|b| b.name == name).count();
    if count != 1 {
        bail!(
            "internal error: expected exactly one '{}' block after upsert, found {}",
            name,
            count
        );
    }
    Ok(merged)
}

fn parse_open(line: &str) -> Option<(String, String)> {
    // Match `# >>> jtr:<name>@<version> >>>`
    let line = line.trim_start();
    let rest = line.strip_prefix("# >>> jtr:")?;
    let rest = rest.strip_suffix(" >>>")?;
    let (name, version) = rest.split_once('@')?;
    if name.is_empty() || version.is_empty() {
        return None;
    }
    Some((name.to_string(), version.to_string()))
}

fn strip_internal_header(lines: &[String]) -> Vec<String> {
    // Drop leading "# source:" and "# do not edit" lines that we wrote ourselves so callers
    // see just the recipe body.
    lines
        .iter()
        .skip_while(|l| {
            let t = l.trim_start();
            t.starts_with("# source:") || t.starts_with("# do not edit")
        })
        .cloned()
        .collect()
}

/// Convenience: confirm a string looks like a valid recipe name.
pub fn validate_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(anyhow!("recipe name cannot be empty"));
    }
    let ok = name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '/');
    if !ok {
        return Err(anyhow!(
            "invalid recipe name '{}': only [a-z0-9_/-] allowed",
            name
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_and_parse_roundtrip() {
        let block = render(
            "postgres-dev",
            "0.1.0",
            Some("https://example.com"),
            "pg-up:\n    docker run ...\n",
        );
        let doc = format!("default:\n    @echo hi\n\n{}", block);
        let parsed = parse_all(&doc);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].name, "postgres-dev");
        assert_eq!(parsed[0].version, "0.1.0");
        assert!(parsed[0].body.contains("pg-up:"));
        assert!(parsed[0].body.contains("docker run"));
    }

    #[test]
    fn remove_strips_block_and_collapses_blanks() {
        let block = render("foo", "0.1.0", None, "bar:\n    @echo hi\n");
        let doc = format!("first:\n    @echo a\n\n{}\nlast:\n    @echo b\n", block);
        let (out, removed) = remove(&doc, "foo");
        assert!(removed);
        assert!(!out.contains("jtr:foo"));
        assert!(out.contains("first:"));
        assert!(out.contains("last:"));
        // Exactly one blank line between the two surviving regions.
        assert!(out.contains("    @echo a\n\nlast:"));
        assert!(!out.contains("\n\n\n"));
    }

    #[test]
    fn remove_trailing_block_does_not_leave_blank_tail() {
        let block = render("foo", "0.1.0", None, "bar:\n    @echo hi");
        let doc = format!("default:\n    @echo a\n\n{}", block);
        let (out, _) = remove(&doc, "foo");
        assert!(out.ends_with("@echo a\n"));
    }

    #[test]
    fn upsert_replaces_existing_block() {
        let v1 = render("foo", "0.1.0", None, "old-body");
        let v2 = render("foo", "0.2.0", None, "new-body");
        let doc = format!("default:\n    @echo hi\n\n{}", v1);
        let updated = upsert(&doc, "foo", &v2).unwrap();
        assert!(updated.contains("new-body"));
        assert!(!updated.contains("old-body"));
        assert!(updated.contains("@0.2.0"));
    }

    #[test]
    fn append_to_empty_doc() {
        let block = render("foo", "0.1.0", None, "bar:\n    @echo");
        let out = append("", &block);
        assert_eq!(out, block);
    }

    #[test]
    fn validate_name_rejects_bad_input() {
        assert!(validate_name("").is_err());
        assert!(validate_name("hello world").is_err());
        assert!(validate_name("hello-world").is_ok());
        assert!(validate_name("user/repo").is_ok());
    }
}
