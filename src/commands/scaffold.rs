use anyhow::{Context, Result, anyhow};
use colored::Colorize;
use std::fs;
use std::path::{Path, PathBuf};

use crate::managed;
use crate::target::Target;

pub fn run(name: &str, target: Target) -> Result<()> {
    managed::validate_name(name)?;

    let cwd = std::env::current_dir().context("could not read current directory")?;
    let layout = detect_layout(&cwd);

    let manifest_path = match &layout {
        Layout::TapRepo { recipes_dir, .. } => recipes_dir.join(format!("{name}.json")),
        Layout::Standalone => cwd.join(format!("{name}.json")),
    };

    if manifest_path.exists() {
        return Err(anyhow!(
            "{} already exists; pick a different name or remove the file first",
            manifest_path.display()
        ));
    }

    let manifest_body = render_manifest(name, target);
    if let Some(parent) = manifest_path.parent()
        && !parent.exists()
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("could not create {}", parent.display()))?;
    }
    fs::write(&manifest_path, &manifest_body)
        .with_context(|| format!("could not write {}", manifest_path.display()))?;

    println!(
        "{} wrote {}",
        "✓".green(),
        manifest_path.display().to_string().bold()
    );

    if let Layout::TapRepo { index_path, .. } = &layout {
        append_index_entry(index_path, name, target)?;
        println!(
            "{} appended stub entry to {} {} run `jtr lint --tap . --fix` to compute the checksum",
            "✓".green(),
            index_path.display().to_string().bold(),
            "—".dimmed()
        );
    } else {
        println!(
            "  {} not inside a tap repo (no index.json in cwd); update your tap's index.json manually if publishing",
            "i".cyan()
        );
    }

    println!(
        "  {} edit the description, snippet, and `shells_out_to` next; then `jtr lint <path>` to verify",
        "i".cyan()
    );
    Ok(())
}

enum Layout {
    /// `cwd` contains `index.json` — treat as a tap repo and write into `recipes/`.
    TapRepo {
        index_path: PathBuf,
        recipes_dir: PathBuf,
    },
    /// No `index.json` in cwd; write a bare manifest into cwd.
    Standalone,
}

fn detect_layout(cwd: &Path) -> Layout {
    let index_path = cwd.join("index.json");
    if index_path.is_file() {
        Layout::TapRepo {
            index_path,
            recipes_dir: cwd.join("recipes"),
        }
    } else {
        Layout::Standalone
    }
}

fn render_manifest(name: &str, target: Target) -> String {
    let snippet = match target {
        Target::Just => format!(
            "# TODO: describe what this recipe does\n{name}:\n    @echo hello from {name}\n",
            name = name
        ),
        Target::Task => format!(
            "{name}:\n  desc: TODO describe what this recipe does\n  cmds:\n    - echo hello from {name}\n",
            name = name
        ),
    };
    let snippet_json = serde_json::Value::String(snippet).to_string();
    format!(
        "{{\n  \"name\": \"{name}\",\n  \"version\": \"0.1.0\",\n  \"description\": \"TODO: one-line description\",\n  \"shells_out_to\": [],\n  \"dependencies\": [],\n  \"targets\": {{\n    \"{target}\": {{\n      \"snippet\": {snippet_json}\n    }}\n  }}\n}}\n",
        name = name,
        target = target.as_str(),
    )
}

fn append_index_entry(index_path: &Path, name: &str, target: Target) -> Result<()> {
    let original = fs::read_to_string(index_path)
        .with_context(|| format!("could not read {}", index_path.display()))?;

    let parsed: serde_json::Value = serde_json::from_str(&original)
        .with_context(|| format!("{} is not valid JSON", index_path.display()))?;
    let recipes = parsed
        .get("recipes")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow!("{} has no `recipes` array", index_path.display()))?;
    if recipes
        .iter()
        .any(|r| r.get("name").and_then(|n| n.as_str()) == Some(name))
    {
        return Err(anyhow!(
            "{} already has an entry for '{}'; pick a different name or remove the entry first",
            index_path.display(),
            name
        ));
    }

    let target_str = target.as_str();
    let entry_block = format!(
        "    {{\n      \"name\": \"{name}\",\n      \"version\": \"0.1.0\",\n      \"description\": \"TODO: one-line description\",\n      \"manifest_url\": \"recipes/{name}.json\",\n      \"targets\": [\"{target}\"]\n    }}",
        name = name,
        target = target_str,
    );

    let updated = splice_into_recipes_array(&original, &entry_block).ok_or_else(|| {
        anyhow!(
            "could not locate the recipes array in {} for splicing; \
             expected a `\"recipes\": [...]` array with the standard formatting",
            index_path.display()
        )
    })?;

    fs::write(index_path, updated)
        .with_context(|| format!("could not write {}", index_path.display()))?;
    Ok(())
}

/// Splice `entry` as the new last item of the `"recipes": [...]` array, preserving
/// the file's existing formatting. Returns `None` if the array can't be located —
/// the caller surfaces a clear error rather than silently rewriting.
fn splice_into_recipes_array(source: &str, entry: &str) -> Option<String> {
    let recipes_key = source.find("\"recipes\":")?;
    let array_open = source[recipes_key..].find('[')?;
    let array_open_abs = recipes_key + array_open;
    let array_close_abs = find_matching_bracket(source.as_bytes(), array_open_abs)?;

    let array_body = &source[array_open_abs + 1..array_close_abs];
    let trimmed = array_body.trim_end_matches([' ', '\t']);
    let trailing_ws_len = array_body.len() - trimmed.len();
    let array_body_end_abs = array_close_abs - trailing_ws_len;

    let body_trim = array_body.trim();
    let mut spliced = String::with_capacity(source.len() + entry.len() + 4);
    spliced.push_str(&source[..array_open_abs + 1]);
    if body_trim.is_empty() {
        spliced.push('\n');
        spliced.push_str(entry);
        spliced.push('\n');
        spliced.push_str("  ");
    } else {
        let before_close = &source[array_open_abs + 1..array_body_end_abs];
        let trimmed_end = before_close.trim_end_matches(['\n']);
        spliced.push_str(trimmed_end);
        spliced.push_str(",\n");
        spliced.push_str(entry);
        spliced.push('\n');
        spliced.push_str("  ");
    }
    spliced.push_str(&source[array_close_abs..]);
    Some(spliced)
}

fn find_matching_bracket(bytes: &[u8], open_idx: usize) -> Option<usize> {
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape = false;
    for (i, &b) in bytes.iter().enumerate().skip(open_idx) {
        if escape {
            escape = false;
            continue;
        }
        if b == b'\\' && in_string {
            escape = true;
            continue;
        }
        if b == b'"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        if b == b'[' {
            depth += 1;
        } else if b == b']' {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splice_appends_to_populated_array() {
        let original = "{\n  \"version\": 1,\n  \"recipes\": [\n    {\n      \"name\": \"foo\"\n    }\n  ]\n}\n";
        let entry = "    {\n      \"name\": \"bar\"\n    }";
        let spliced = splice_into_recipes_array(original, entry).unwrap();
        assert!(spliced.contains("\"foo\""), "foo entry should survive");
        assert!(spliced.contains("\"bar\""), "bar entry should be inserted");
        let foo_pos = spliced.find("\"foo\"").unwrap();
        let bar_pos = spliced.find("\"bar\"").unwrap();
        assert!(foo_pos < bar_pos, "bar should appear after foo");
        assert!(
            spliced.contains("    }\n  ]"),
            "closing bracket indent preserved"
        );
    }

    #[test]
    fn splice_inserts_into_empty_array() {
        let original = "{\n  \"version\": 1,\n  \"recipes\": []\n}\n";
        let entry = "    {\n      \"name\": \"foo\"\n    }";
        let spliced = splice_into_recipes_array(original, entry).unwrap();
        assert!(spliced.contains("\"foo\""));
        assert!(spliced.contains("  ]"));
    }

    #[test]
    fn render_manifest_for_just_target_has_snippet_placeholder() {
        let body = render_manifest("foo", Target::Just);
        let parsed: serde_json::Value = serde_json::from_str(&body).expect("valid JSON");
        assert_eq!(parsed["name"], "foo");
        assert_eq!(parsed["version"], "0.1.0");
        let snippet = parsed["targets"]["just"]["snippet"].as_str().unwrap();
        assert!(snippet.contains("foo:"));
        assert!(snippet.contains("hello from foo"));
    }

    #[test]
    fn render_manifest_for_task_target_emits_task_syntax() {
        let body = render_manifest("bar", Target::Task);
        let parsed: serde_json::Value = serde_json::from_str(&body).expect("valid JSON");
        let snippet = parsed["targets"]["task"]["snippet"].as_str().unwrap();
        assert!(snippet.contains("bar:"));
        assert!(snippet.contains("cmds:"));
    }
}
