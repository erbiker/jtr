use anyhow::{Result, anyhow, bail};

use crate::target::Target;

/// Spaces a Task managed block is indented to nest under the top-level `tasks:`
/// map. Fixed (not detected from the file): render and insertion must use the
/// *same* indent or `jtr update`'s `extract_block_text == render_block` no-op
/// check drifts and every update spuriously "refreshes". Mixed indentation
/// across sibling YAML keys is valid, so a fixed 2 is always safe.
const TASK_INDENT: usize = 2;

/// Sentinel-delimited block embedded in the user's justfile/Taskfile.
///
/// Format:
///
/// ```text
/// # >>> jtr:<name>@<version> >>>
/// # source: <homepage_or_index>
/// # pinned: <version>                   (omitted when not pinned)
/// # depends-on: <name>, <name>          (omitted when empty)
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
    /// Names of recipes this block declares as dependencies. Recorded inline so
    /// `jtr remove` can compute reverse-deps without hitting the network.
    pub dependencies: Vec<String>,
    /// When `Some(v)`, the user explicitly asked for this version (`jtr install
    /// foo@v`). `jtr update` then refuses to bump the block, and `jtr doctor`
    /// treats matching the pin as healthy rather than out-of-date.
    pub pinned: Option<String>,
}

pub fn open_marker(name: &str) -> String {
    // The trailing `@` is the boundary against the version suffix in the
    // rendered open line (`# >>> jtr:<name>@<version> >>>`). Without it,
    // `starts_with(open_marker("foo"))` also matches a `foo-bar` block —
    // see [issue #12](https://github.com/erbiker/jtr/issues/12).
    format!("# >>> jtr:{name}@", name = name)
}

pub fn close_marker(name: &str) -> String {
    format!("# <<< jtr:{name} <<<", name = name)
}

pub fn render(
    name: &str,
    version: &str,
    source: Option<&str>,
    pinned: Option<&str>,
    dependencies: &[String],
    snippet: &str,
) -> String {
    let source_line = source
        .map(|s| format!("# source: {s}\n", s = s))
        .unwrap_or_default();
    let pinned_line = pinned
        .map(|v| format!("# pinned: {v}\n"))
        .unwrap_or_default();
    let depends_line = if dependencies.is_empty() {
        String::new()
    } else {
        format!("# depends-on: {}\n", dependencies.join(", "))
    };
    let trimmed = snippet.trim_end_matches('\n');
    format!(
        "# >>> jtr:{name}@{version} >>>\n\
         {source_line}\
         {pinned_line}\
         {depends_line}\
         # do not edit manually; use `jtr update {name}` or `jtr remove {name}`\n\
         {body}\n\
         # <<< jtr:{name} <<<\n",
        name = name,
        version = version,
        source_line = source_line,
        pinned_line = pinned_line,
        depends_line = depends_line,
        body = trimmed,
    )
}

/// Render the managed block in the form appropriate for `target`. For Just this
/// is the canonical column-0 block; for Task the same block indented by
/// [`TASK_INDENT`] so it nests under the `tasks:` map. The result is what both
/// the on-disk block and any re-render must equal, so update/diff no-op
/// detection stays byte-exact across targets.
#[allow(clippy::too_many_arguments)]
pub fn render_block(
    target: Target,
    name: &str,
    version: &str,
    source: Option<&str>,
    pinned: Option<&str>,
    dependencies: &[String],
    snippet: &str,
) -> String {
    let block = render(name, version, source, pinned, dependencies, snippet);
    match target {
        Target::Just => block,
        Target::Task => indent_block(&block, TASK_INDENT),
    }
}

/// Indent every non-blank line by `spaces`. Blank lines are left empty so the
/// output carries no trailing whitespace. A uniform shift preserves relative
/// indentation, so YAML block scalars inside the snippet survive intact.
fn indent_block(text: &str, spaces: usize) -> String {
    let pad = " ".repeat(spaces);
    let trailing_nl = text.ends_with('\n');
    let mut joined = text
        .lines()
        .map(|l| {
            if l.trim().is_empty() {
                String::new()
            } else {
                format!("{pad}{l}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    if trailing_nl {
        joined.push('\n');
    }
    joined
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
                let dependencies = parse_dependencies(&body_lines);
                let pinned = parse_pinned(&body_lines);
                let body = strip_internal_header(&body_lines).join("\n");
                out.push(ManagedBlock {
                    name,
                    version,
                    body,
                    dependencies,
                    pinned,
                });
            }
        }
    }

    out
}

/// Remove the block named `<name>`. Returns the new document and whether
/// anything was removed. The cleanup strategy depends on `target`:
///
/// - **Just** collapses every run of 2+ blank lines to one and strips trailing
///   blanks — safe tidying in a comment-and-recipe file.
/// - **Task** only mends the *removal seam* (a double blank left where the block
///   was) and strips trailing blanks. It never collapses interior blanks
///   globally, because a blank line inside a user's YAML literal block scalar
///   (`cmds: - |`) is significant — global collapse would silently corrupt an
///   unrelated sibling task on any install (every install runs `upsert` →
///   `remove`).
pub fn remove(target: Target, doc: &str, name: &str) -> (String, bool) {
    match target {
        Target::Just => remove_just(doc, name),
        Target::Task => remove_task(doc, name),
    }
}

fn remove_just(doc: &str, name: &str) -> (String, bool) {
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

fn remove_task(doc: &str, name: &str) -> (String, bool) {
    let open_prefix = open_marker(name);
    let close = close_marker(name);

    let lines: Vec<&str> = doc.lines().collect();
    let mut out: Vec<&str> = Vec::with_capacity(lines.len());
    let mut in_block = false;
    let mut removed = false;
    let mut seam: Option<usize> = None;

    for line in &lines {
        if !in_block && line.trim_start().starts_with(&open_prefix) {
            in_block = true;
            removed = true;
            seam = Some(out.len());
            continue;
        }
        if in_block {
            if line.trim() == close.trim() {
                in_block = false;
            }
            continue;
        }
        out.push(line);
    }

    // Mend only the seam: if the block sat between two blank separators, the
    // removal left two adjacent blanks — collapse to one. Interior blanks
    // elsewhere are untouched.
    if let Some(i) = seam
        && i > 0
        && i < out.len()
        && out[i - 1].trim().is_empty()
        && out[i].trim().is_empty()
    {
        out.remove(i);
    }

    while out.last().is_some_and(|l| l.trim().is_empty()) {
        out.pop();
    }

    let mut joined = out.join("\n");
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

/// Install or replace `name`'s block with `rendered`. For Just the block is
/// appended at end-of-file; for Task it is spliced into the top-level `tasks:`
/// map (creating one if absent). `rendered` must already be in the form
/// [`render_block`] produced for the same `target` (indented for Task).
pub fn upsert(target: Target, doc: &str, name: &str, rendered: &str) -> Result<String> {
    let (without, _) = remove(target, doc, name);
    let merged = match target {
        Target::Just => append(&without, rendered),
        Target::Task => insert_into_tasks(&without, rendered)?,
    };

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

/// Splice an already-indented managed block into the end of the top-level
/// `tasks:` map. Pure text surgery — the YAML is never parsed or re-serialized,
/// so the user's other tasks, vars, includes, and comments survive byte-for-byte
/// (the same reason the justfile path edits text rather than an AST). If there is
/// no `tasks:` map, one is created at end-of-file.
fn insert_into_tasks(doc: &str, block: &str) -> Result<String> {
    let lines: Vec<&str> = doc.lines().collect();

    let Some(tasks_idx) = lines.iter().position(|l| is_tasks_key(l)) else {
        let mut out = doc.to_string();
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        if !out.is_empty() && !out.ends_with("\n\n") {
            out.push('\n');
        }
        out.push_str("tasks:\n");
        out.push_str(block);
        return Ok(out);
    };

    // The map body runs until the next column-0 non-blank line (a sibling
    // top-level key) or end-of-file.
    let end = lines
        .iter()
        .enumerate()
        .skip(tasks_idx + 1)
        .find(|(_, l)| !l.trim().is_empty() && !starts_with_space(l))
        .map(|(i, _)| i)
        .unwrap_or(lines.len());

    let mut head: Vec<&str> = lines[..end].to_vec();
    let tail: Vec<&str> = lines[end..].to_vec();
    while head.last().is_some_and(|l| l.trim().is_empty()) {
        head.pop();
    }

    // No blank separator when the map is empty (head ends at the `tasks:` line);
    // otherwise one blank line before our block.
    let map_is_empty = head.len() <= tasks_idx + 1;

    let mut out: Vec<String> = head.iter().map(|s| s.to_string()).collect();
    if !map_is_empty {
        out.push(String::new());
    }
    out.extend(block.lines().map(|s| s.to_string()));
    if !tail.is_empty() {
        out.push(String::new());
        out.extend(tail.iter().map(|s| s.to_string()));
    }

    let mut joined = out.join("\n");
    joined.push('\n');
    Ok(joined)
}

/// A column-0 line introducing the block-style `tasks:` mapping — exactly
/// `tasks:` with nothing but optional whitespace or a trailing `# comment`
/// after it. A scalar/flow value (`tasks: {…}`) is deliberately not matched so
/// we never try to splice by indentation into something that isn't a block map.
fn is_tasks_key(line: &str) -> bool {
    if starts_with_space(line) {
        return false;
    }
    let Some(rest) = line.strip_prefix("tasks:") else {
        return false;
    };
    let rest = rest.trim_start();
    rest.is_empty() || rest.starts_with('#')
}

fn starts_with_space(line: &str) -> bool {
    line.starts_with(' ') || line.starts_with('\t')
}

/// Pull the raw text of the named managed block out of `doc`, including the
/// open and close marker lines. Returns an empty string if the block isn't
/// found. The returned string always ends with `\n` so it diffs cleanly against
/// `render`'s output (which also ends with `\n`). Used by `jtr diff` and
/// `jtr update --dry-run` to show the on-disk block as the "before" side.
pub fn extract_block_text(doc: &str, name: &str) -> String {
    let open_prefix = open_marker(name);
    let close = close_marker(name);
    let close_trimmed = close.trim();

    let lines: Vec<&str> = doc.lines().collect();
    let Some(start) = lines
        .iter()
        .position(|l| l.trim_start().starts_with(&open_prefix))
    else {
        return String::new();
    };
    let Some(rel_end) = lines[start..]
        .iter()
        .position(|l| l.trim() == close_trimmed)
    else {
        return String::new();
    };
    let end = start + rel_end;
    let mut out = lines[start..=end].join("\n");
    out.push('\n');
    out
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
    // Drop leading "# source:", "# pinned:", "# depends-on:", and "# do not edit"
    // lines that we wrote ourselves so callers see just the recipe body.
    lines
        .iter()
        .skip_while(|l| {
            let t = l.trim_start();
            t.starts_with("# source:")
                || t.starts_with("# pinned:")
                || t.starts_with("# depends-on:")
                || t.starts_with("# do not edit")
        })
        .cloned()
        .collect()
}

fn parse_dependencies(lines: &[String]) -> Vec<String> {
    for line in lines {
        let t = line.trim_start();
        if let Some(rest) = t.strip_prefix("# depends-on:") {
            return rest
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
        if !is_header_line(t) {
            break;
        }
    }
    Vec::new()
}

fn parse_pinned(lines: &[String]) -> Option<String> {
    for line in lines {
        let t = line.trim_start();
        if let Some(rest) = t.strip_prefix("# pinned:") {
            let v = rest.trim();
            if v.is_empty() {
                return None;
            }
            return Some(v.to_string());
        }
        if !is_header_line(t) {
            break;
        }
    }
    None
}

fn is_header_line(trimmed: &str) -> bool {
    trimmed.is_empty()
        || trimmed.starts_with("# source:")
        || trimmed.starts_with("# pinned:")
        || trimmed.starts_with("# depends-on:")
        || trimmed.starts_with("# do not edit")
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
    use crate::target::Target;

    #[test]
    fn render_and_parse_roundtrip() {
        let block = render(
            "postgres-dev",
            "0.1.0",
            Some("https://example.com"),
            None,
            &[],
            "pg-up:\n    docker run ...\n",
        );
        let doc = format!("default:\n    @echo hi\n\n{}", block);
        let parsed = parse_all(&doc);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].name, "postgres-dev");
        assert_eq!(parsed[0].version, "0.1.0");
        assert!(parsed[0].body.contains("pg-up:"));
        assert!(parsed[0].body.contains("docker run"));
        assert!(parsed[0].dependencies.is_empty());
    }

    #[test]
    fn render_and_parse_roundtrip_with_dependencies() {
        let deps = vec!["clean".to_string(), "alice/recipes/foo".to_string()];
        let block = render(
            "fancy-build",
            "0.2.0",
            Some("https://example.com"),
            None,
            &deps,
            "fancy:\n    @echo hi\n",
        );
        // The depends-on line is in the rendered output, but stripped from body.
        assert!(block.contains("# depends-on: clean, alice/recipes/foo"));

        let parsed = parse_all(&block);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].dependencies, deps);
        assert!(!parsed[0].body.contains("depends-on"));
    }

    #[test]
    fn render_and_parse_roundtrip_with_pin() {
        let block = render(
            "postgres-dev",
            "0.1.0",
            Some("https://example.com"),
            Some("0.1.0"),
            &["clean".to_string()],
            "pg-up:\n    docker run ...\n",
        );
        assert!(block.contains("# pinned: 0.1.0"));
        // pinned line should appear between source and depends-on.
        let src = block.find("# source:").unwrap();
        let pin = block.find("# pinned:").unwrap();
        let dep = block.find("# depends-on:").unwrap();
        assert!(src < pin && pin < dep);

        let parsed = parse_all(&block);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].pinned.as_deref(), Some("0.1.0"));
        assert_eq!(parsed[0].dependencies, vec!["clean".to_string()]);
        assert!(!parsed[0].body.contains("pinned:"));
        assert!(!parsed[0].body.contains("depends-on"));
    }

    #[test]
    fn render_omits_pinned_line_when_none() {
        let block = render("foo", "0.1.0", None, None, &[], "bar:\n    @echo hi\n");
        assert!(!block.contains("# pinned:"));
    }

    #[test]
    fn render_omits_depends_line_when_empty() {
        let block = render("foo", "0.1.0", None, None, &[], "bar:\n    @echo hi\n");
        assert!(!block.contains("# depends-on:"));
    }

    #[test]
    fn remove_strips_block_and_collapses_blanks() {
        let block = render("foo", "0.1.0", None, None, &[], "bar:\n    @echo hi\n");
        let doc = format!("first:\n    @echo a\n\n{}\nlast:\n    @echo b\n", block);
        let (out, removed) = remove(Target::Just, &doc, "foo");
        assert!(removed);
        assert!(!out.contains("jtr:foo"));
        assert!(out.contains("first:"));
        assert!(out.contains("last:"));
        // Exactly one blank line between the two surviving regions.
        assert!(out.contains("    @echo a\n\nlast:"));
        assert!(!out.contains("\n\n\n"));
    }

    #[test]
    fn remove_does_not_strip_blocks_with_shared_prefix() {
        // Regression for #12: `remove("foo")` used to match `foo-bar`'s open
        // marker via `starts_with("# >>> jtr:foo")`, then consume everything
        // up to (and past) `foo-bar`'s body looking for `foo`'s close.
        let foo = render("foo", "0.1.0", None, None, &[], "foo:\n    @echo a");
        let foo_bar = render("foo-bar", "0.2.0", None, None, &[], "foo-bar:\n    @echo b");
        let doc = format!("default:\n    @echo hi\n\n{}\n{}", foo, foo_bar);

        let (out, removed) = remove(Target::Just, &doc, "foo");
        assert!(removed);
        assert!(!out.contains("# >>> jtr:foo@"));
        assert!(out.contains("# >>> jtr:foo-bar@0.2.0 >>>"));
        assert!(out.contains("foo-bar:\n    @echo b"));
        assert!(out.contains("# <<< jtr:foo-bar <<<"));
    }

    #[test]
    fn remove_trailing_block_does_not_leave_blank_tail() {
        let block = render("foo", "0.1.0", None, None, &[], "bar:\n    @echo hi");
        let doc = format!("default:\n    @echo a\n\n{}", block);
        let (out, _) = remove(Target::Just, &doc, "foo");
        assert!(out.ends_with("@echo a\n"));
    }

    #[test]
    fn upsert_replaces_existing_block() {
        let v1 = render("foo", "0.1.0", None, None, &[], "old-body");
        let v2 = render("foo", "0.2.0", None, None, &[], "new-body");
        let doc = format!("default:\n    @echo hi\n\n{}", v1);
        let updated = upsert(Target::Just, &doc, "foo", &v2).unwrap();
        assert!(updated.contains("new-body"));
        assert!(!updated.contains("old-body"));
        assert!(updated.contains("@0.2.0"));
    }

    #[test]
    fn append_to_empty_doc() {
        let block = render("foo", "0.1.0", None, None, &[], "bar:\n    @echo");
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

    const TASK_SNIPPET: &str =
        "db-up:\n  desc: Start postgres\n  cmds:\n    - docker run --rm postgres";

    #[test]
    fn task_render_block_indents_every_line_by_two() {
        let rendered = render_block(
            Target::Task,
            "postgres-dev",
            "0.1.0",
            Some("https://example.com"),
            None,
            &[],
            TASK_SNIPPET,
        );
        // Every non-blank line gains exactly two leading spaces; markers included.
        for line in rendered.lines() {
            if !line.trim().is_empty() {
                assert!(
                    line.starts_with("  "),
                    "line not indented under tasks:: {line:?}"
                );
            }
        }
        assert!(rendered.contains("  # >>> jtr:postgres-dev@0.1.0 >>>"));
        assert!(rendered.contains("  db-up:"));
        assert!(rendered.contains("    desc: Start postgres"));
        assert!(rendered.contains("      - docker run --rm postgres"));
        assert!(rendered.contains("  # <<< jtr:postgres-dev <<<"));
    }

    // The load-bearing invariant for Task idempotency: the text we write into the
    // tasks: map (render_block) must be byte-identical to what extract_block_text
    // reads back. If it drifts, `jtr update` reports a spurious "would refresh"
    // and rewrites the file on every run.
    #[test]
    fn task_render_insert_extract_roundtrips() {
        let taskfile = "version: '3'\n\ntasks:\n  default:\n    cmds:\n      - task --list\n";
        let rendered = render_block(
            Target::Task,
            "postgres-dev",
            "0.1.0",
            Some("https://example.com"),
            None,
            &["clean".to_string()],
            TASK_SNIPPET,
        );
        let merged = upsert(Target::Task, taskfile, "postgres-dev", &rendered).unwrap();

        let extracted = extract_block_text(&merged, "postgres-dev");
        assert_eq!(extracted, rendered, "on-disk block must equal render_block");

        // And the block round-trips through the generic parser.
        let parsed = parse_all(&merged);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].name, "postgres-dev");
        assert_eq!(parsed[0].version, "0.1.0");
        assert_eq!(parsed[0].dependencies, vec!["clean".to_string()]);
    }

    #[test]
    fn task_insert_into_empty_tasks_map() {
        let taskfile = "version: '3'\n\ntasks:\n";
        let rendered = render_block(Target::Task, "foo", "0.1.0", None, None, &[], TASK_SNIPPET);
        let merged = upsert(Target::Task, taskfile, "foo", &rendered).unwrap();
        // No spurious blank line between `tasks:` and the first managed entry.
        assert!(merged.contains("tasks:\n  # >>> jtr:foo@0.1.0 >>>"));
        assert!(merged.starts_with("version: '3'"));
    }

    #[test]
    fn task_insert_preserves_following_top_level_key() {
        let taskfile =
            "version: '3'\n\ntasks:\n  default:\n    cmds:\n      - echo hi\n\nvars:\n  FOO: bar\n";
        let rendered = render_block(Target::Task, "foo", "0.1.0", None, None, &[], TASK_SNIPPET);
        let merged = upsert(Target::Task, taskfile, "foo", &rendered).unwrap();
        // The block lands inside the tasks map, before the next top-level key.
        let foo_at = merged.find("  # >>> jtr:foo@").unwrap();
        let vars_at = merged.find("\nvars:").unwrap();
        let default_at = merged.find("  default:").unwrap();
        assert!(default_at < foo_at, "block should follow existing tasks");
        assert!(
            foo_at < vars_at,
            "block must stay inside tasks: not after vars:"
        );
        assert!(merged.contains("vars:\n  FOO: bar"));
    }

    #[test]
    fn task_insert_creates_tasks_map_when_absent() {
        let taskfile = "version: '3'\n\nincludes:\n  docker: ./docker.yml\n";
        let rendered = render_block(Target::Task, "foo", "0.1.0", None, None, &[], TASK_SNIPPET);
        let merged = upsert(Target::Task, taskfile, "foo", &rendered).unwrap();
        assert!(merged.contains("includes:\n  docker: ./docker.yml"));
        assert!(merged.contains("tasks:\n  # >>> jtr:foo@0.1.0 >>>"));
        assert_eq!(parse_all(&merged).len(), 1);
    }

    // Direct hit on "preserves the user's other tasks": a sibling task with a
    // literal block scalar whose value contains blank lines. Installing (and then
    // removing) an unrelated managed block must not collapse those interior blanks.
    #[test]
    fn task_install_remove_preserves_sibling_block_scalar() {
        let taskfile = "version: '3'\n\ntasks:\n  greet:\n    cmds:\n      - |\n        echo a\n\n\n        echo b\n";
        let rendered = render_block(Target::Task, "foo", "0.1.0", None, None, &[], TASK_SNIPPET);

        let installed = upsert(Target::Task, taskfile, "foo", &rendered).unwrap();
        assert!(
            installed.contains("        echo a\n\n\n        echo b"),
            "block scalar blanks must survive install"
        );

        let (removed, did) = remove(Target::Task, &installed, "foo");
        assert!(did);
        assert!(!removed.contains("jtr:foo@"));
        assert!(
            removed.contains("        echo a\n\n\n        echo b"),
            "block scalar blanks must survive remove: {removed:?}"
        );
    }

    #[test]
    fn task_remove_mends_seam_to_single_blank() {
        let a = render_block(
            Target::Task,
            "a",
            "0.1.0",
            None,
            None,
            &[],
            "a-task:\n  cmds:\n    - echo a",
        );
        let b = render_block(
            Target::Task,
            "b",
            "0.1.0",
            None,
            None,
            &[],
            "b-task:\n  cmds:\n    - echo b",
        );
        let base = "version: '3'\n\ntasks:\n  default:\n    cmds:\n      - echo hi\n";
        let with_a = upsert(Target::Task, base, "a", &a).unwrap();
        let with_both = upsert(Target::Task, &with_a, "b", &b).unwrap();
        let (after, _) = remove(Target::Task, &with_both, "a");
        assert!(
            !after.contains("\n\n\n"),
            "seam should collapse to one blank: {after:?}"
        );
        assert!(after.contains("# >>> jtr:b@"));
    }

    #[test]
    fn is_tasks_key_matches_only_block_style_top_level() {
        assert!(is_tasks_key("tasks:"));
        assert!(is_tasks_key("tasks:   "));
        assert!(is_tasks_key("tasks: # the recipes"));
        assert!(!is_tasks_key("  tasks:"));
        assert!(!is_tasks_key("tasks: {}"));
        assert!(!is_tasks_key("tasks_extra:"));
        assert!(!is_tasks_key("version: '3'"));
    }
}
