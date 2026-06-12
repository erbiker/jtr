//! End-to-end CLI integration tests. These invoke the compiled `jtr` binary against the
//! bundled sample index in `jtr-index/`, exercising the same code paths a real user hits.
//!
//! When adding a new command or changing CLI behavior, add a test here. See TESTING.md.

use assert_cmd::Command;
use predicates::str;
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for b in digest {
        write!(&mut out, "{:02x}", b).unwrap();
    }
    out
}

fn sample_index_url() -> String {
    format!("file://{}/jtr-index/index.json", env!("CARGO_MANIFEST_DIR"))
}

/// Build a single-recipe temp index that publishes `postgres-dev` at the given version,
/// with a snippet whose content is detectable from a test (uses the version as a marker
/// in an `@echo` line). Returns the `file://` URL the CLI can consume.
fn write_postgres_dev_index(dir: &Path, version: &str) -> String {
    let recipes_dir = dir.join("recipes");
    fs::create_dir_all(&recipes_dir).unwrap();

    let manifest = format!(
        r#"{{
  "name": "postgres-dev",
  "version": "{version}",
  "description": "test fixture postgres-dev",
  "homepage": "https://example.invalid/postgres-dev",
  "shells_out_to": ["docker"],
  "targets": {{
    "just": {{
      "snippet": "postgres-up:\n    @echo marker-{version}\n"
    }}
  }}
}}"#
    );
    fs::write(recipes_dir.join("postgres-dev.json"), &manifest).unwrap();
    let sha = sha256_hex(manifest.as_bytes());

    let index = format!(
        r#"{{
  "version": 1,
  "recipes": [
    {{
      "name": "postgres-dev",
      "version": "{version}",
      "description": "test fixture postgres-dev",
      "manifest_url": "recipes/postgres-dev.json",
      "targets": ["just"],
      "sha256": "{sha}"
    }}
  ]
}}"#
    );
    fs::write(dir.join("index.json"), index).unwrap();

    format!("file://{}/index.json", dir.display())
}

/// Build a single-recipe temp index for doctor tests. The recipe's `shells_out_to`
/// is configurable so callers can exercise both the "all tools present" and "tool
/// missing" branches without depending on whatever happens to be on the runner's PATH.
fn write_doctor_index(dir: &Path, name: &str, version: &str, shells_out_to: &[&str]) -> String {
    let recipes_dir = dir.join("recipes");
    fs::create_dir_all(&recipes_dir).unwrap();

    let tools = shells_out_to
        .iter()
        .map(|t| format!("\"{}\"", t))
        .collect::<Vec<_>>()
        .join(", ");
    let manifest = format!(
        r#"{{
  "name": "{name}",
  "version": "{version}",
  "description": "doctor fixture",
  "shells_out_to": [{tools}],
  "targets": {{
    "just": {{
      "snippet": "{name}-noop:\n    @echo ok\n"
    }}
  }}
}}"#
    );
    let manifest_file = format!("{name}.json");
    fs::write(recipes_dir.join(&manifest_file), &manifest).unwrap();
    let sha = sha256_hex(manifest.as_bytes());

    let index = format!(
        r#"{{
  "version": 1,
  "recipes": [
    {{
      "name": "{name}",
      "version": "{version}",
      "description": "doctor fixture",
      "manifest_url": "recipes/{manifest_file}",
      "targets": ["just"],
      "sha256": "{sha}"
    }}
  ]
}}"#
    );
    fs::write(dir.join("index.json"), index).unwrap();

    format!("file://{}/index.json", dir.display())
}

fn write_empty_index(dir: &Path) -> String {
    let index = r#"{"version": 1, "recipes": []}"#;
    fs::write(dir.join("index.json"), index).unwrap();
    format!("file://{}/index.json", dir.display())
}

/// Build a single-recipe temp index used as a "tap" fixture. The recipe ships
/// with a snippet that puts a recognizable marker in its `@echo` line so a
/// test can assert which tap the installed block came from.
fn write_tap_index(dir: &Path, recipe: &str, version: &str, marker: &str) -> String {
    let recipes_dir = dir.join("recipes");
    fs::create_dir_all(&recipes_dir).unwrap();

    let manifest = format!(
        r#"{{
  "name": "{recipe}",
  "version": "{version}",
  "description": "fixture for the {marker} tap",
  "shells_out_to": [],
  "targets": {{
    "just": {{
      "snippet": "{recipe}:\n    @echo {marker}\n"
    }}
  }}
}}"#
    );
    fs::write(recipes_dir.join(format!("{recipe}.json")), &manifest).unwrap();
    let sha = sha256_hex(manifest.as_bytes());

    let index = format!(
        r#"{{
  "version": 1,
  "recipes": [
    {{
      "name": "{recipe}",
      "version": "{version}",
      "description": "fixture for the {marker} tap",
      "manifest_url": "recipes/{recipe}.json",
      "targets": ["just"],
      "sha256": "{sha}"
    }}
  ]
}}"#
    );
    fs::write(dir.join("index.json"), index).unwrap();

    format!("file://{}/index.json", dir.display())
}

fn project_with_justfile(initial: &str) -> TempDir {
    let dir = TempDir::new().expect("create temp dir");
    fs::write(dir.path().join("justfile"), initial).expect("write justfile");
    dir
}

fn jtr() -> Command {
    Command::cargo_bin("jtr").expect("locate jtr binary")
}

#[test]
fn search_lists_all_seed_recipes() {
    jtr()
        .env("JTR_INDEX_URL", sample_index_url())
        .arg("search")
        .assert()
        .success()
        .stdout(str::contains("postgres-dev"))
        .stdout(str::contains("redis-dev"))
        .stdout(str::contains("rust-lint-format"))
        .stdout(str::contains("node-lint-format"))
        .stdout(str::contains("clean"));
}

#[test]
fn search_filters_by_query() {
    let assert = jtr()
        .env("JTR_INDEX_URL", sample_index_url())
        .args(["search", "postgres"])
        .assert()
        .success()
        .stdout(str::contains("postgres-dev"));

    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(
        !stdout.contains("redis-dev"),
        "redis-dev should not match 'postgres'"
    );
}

#[test]
fn install_appends_managed_block() {
    let project = project_with_justfile("default:\n    @echo hi\n");

    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", sample_index_url())
        .args(["install", "postgres-dev"])
        .assert()
        .success()
        .stdout(str::contains("installed"));

    let result = fs::read_to_string(project.path().join("justfile")).unwrap();
    assert!(result.contains("# >>> jtr:postgres-dev@0.1.0 >>>"));
    assert!(result.contains("# <<< jtr:postgres-dev <<<"));
    assert!(result.contains("postgres-up:"));
    // The original content is preserved.
    assert!(result.contains("default:\n    @echo hi"));
}

#[test]
fn install_unknown_recipe_errors_cleanly() {
    let project = project_with_justfile("default:\n    @echo hi\n");

    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", sample_index_url())
        .args(["install", "does-not-exist"])
        .assert()
        .failure()
        .stderr(str::contains("not found"));
}

#[test]
fn install_is_idempotent_for_same_version() {
    let project = project_with_justfile("default:\n    @echo hi\n");

    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", sample_index_url())
        .args(["install", "postgres-dev"])
        .assert()
        .success();

    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", sample_index_url())
        .args(["install", "postgres-dev"])
        .assert()
        .success()
        .stdout(str::contains("already at version"));

    let result = fs::read_to_string(project.path().join("justfile")).unwrap();
    // Use the `@` boundary (mirrors the production fix for #12) so this
    // assertion doesn't false-positive if a sibling `postgres-dev-…` recipe
    // is ever added to the sample index.
    let occurrences = result.matches("# >>> jtr:postgres-dev@").count();
    assert_eq!(
        occurrences, 1,
        "expected exactly one postgres-dev block; got {occurrences}"
    );
}

#[test]
fn list_shows_installed_recipes() {
    let project = project_with_justfile("default:\n    @echo hi\n");
    let index = sample_index_url();

    for recipe in ["postgres-dev", "rust-lint-format"] {
        jtr()
            .current_dir(project.path())
            .env("JTR_INDEX_URL", &index)
            .args(["install", recipe])
            .assert()
            .success();
    }

    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", &index)
        .arg("list")
        .assert()
        .success()
        .stdout(str::contains("postgres-dev"))
        .stdout(str::contains("rust-lint-format"))
        .stdout(str::contains("@0.1.0"));
}

#[test]
fn remove_strips_block_and_preserves_surrounding_content() {
    let project = project_with_justfile("default:\n    @echo hi\n");
    let index = sample_index_url();

    for recipe in ["postgres-dev", "rust-lint-format"] {
        jtr()
            .current_dir(project.path())
            .env("JTR_INDEX_URL", &index)
            .args(["install", recipe])
            .assert()
            .success();
    }

    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", &index)
        .args(["remove", "postgres-dev"])
        .assert()
        .success()
        .stdout(str::contains("removed"));

    let result = fs::read_to_string(project.path().join("justfile")).unwrap();
    assert!(!result.contains("postgres-dev"));
    assert!(result.contains("rust-lint-format"));
    assert!(result.contains("default:\n    @echo hi"));
    // Exactly one blank line between the surviving regions, not three.
    assert!(result.contains("    @echo hi\n\n# >>> jtr:rust-lint-format"));
    assert!(!result.contains("\n\n\n"));
}

#[test]
fn remove_nonexistent_block_is_a_noop() {
    let project = project_with_justfile("default:\n    @echo hi\n");

    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", sample_index_url())
        .args(["remove", "postgres-dev"])
        .assert()
        .success()
        .stdout(str::contains("no managed block"));

    let result = fs::read_to_string(project.path().join("justfile")).unwrap();
    assert_eq!(result, "default:\n    @echo hi\n");
}

#[test]
fn remove_with_prefix_overlapping_name_leaves_sibling_intact() {
    // Regression for #12: removing `foo` used to prefix-match `foo-bar`'s
    // open marker (`# >>> jtr:foo` is a prefix of `# >>> jtr:foo-bar@...`)
    // and silently strip the sibling block too. The fix adds a trailing `@`
    // to `open_marker` so it's a proper boundary.
    let registry_dir = TempDir::new().unwrap();
    let index = write_dep_index(
        registry_dir.path(),
        &[("foo", "0.1.0", &[]), ("foo-bar", "0.2.0", &[])],
    );
    let project = project_with_justfile("default:\n    @echo hi\n");

    for recipe in ["foo-bar", "foo"] {
        jtr()
            .current_dir(project.path())
            .env("JTR_INDEX_URL", &index)
            .args(["install", recipe])
            .assert()
            .success();
    }

    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", &index)
        .args(["remove", "foo"])
        .assert()
        .success();

    let body = fs::read_to_string(project.path().join("justfile")).unwrap();
    assert!(!body.contains("# >>> jtr:foo@"), "foo should be removed");
    assert!(
        body.contains("# >>> jtr:foo-bar@0.2.0 >>>"),
        "foo-bar should survive the remove of foo:\n{body}"
    );
    assert!(
        body.contains("foo-bar-noop:"),
        "foo-bar body should survive"
    );
    assert!(body.contains("# <<< jtr:foo-bar <<<"));
}

#[test]
fn install_without_justfile_errors_with_hint() {
    let project = TempDir::new().unwrap();

    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", sample_index_url())
        .args(["install", "postgres-dev"])
        .assert()
        .failure()
        .stderr(str::contains("no justfile or Taskfile"));
}

#[test]
fn update_swaps_block_to_new_version() {
    let project = project_with_justfile("default:\n    @echo hi\n");
    let registry_dir = TempDir::new().unwrap();

    let v1_index = write_postgres_dev_index(registry_dir.path(), "0.1.0");
    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", &v1_index)
        .args(["install", "postgres-dev"])
        .assert()
        .success();

    let before = fs::read_to_string(project.path().join("justfile")).unwrap();
    assert!(before.contains("# >>> jtr:postgres-dev@0.1.0 >>>"));
    assert!(before.contains("marker-0.1.0"));

    let v2_index = write_postgres_dev_index(registry_dir.path(), "0.2.0");
    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", &v2_index)
        .args(["update", "postgres-dev"])
        .assert()
        .success()
        .stdout(str::contains("updated"))
        .stdout(str::contains("0.1.0"))
        .stdout(str::contains("0.2.0"));

    let after = fs::read_to_string(project.path().join("justfile")).unwrap();
    assert!(after.contains("# >>> jtr:postgres-dev@0.2.0 >>>"));
    assert!(!after.contains("@0.1.0"));
    assert!(after.contains("marker-0.2.0"));
    assert!(!after.contains("marker-0.1.0"));
}

#[test]
fn update_is_a_noop_when_already_at_latest() {
    let project = project_with_justfile("default:\n    @echo hi\n");

    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", sample_index_url())
        .args(["install", "postgres-dev"])
        .assert()
        .success();
    let before = fs::read_to_string(project.path().join("justfile")).unwrap();

    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", sample_index_url())
        .args(["update", "postgres-dev"])
        .assert()
        .success()
        .stdout(str::contains("already at version 0.1.0"));

    let after = fs::read_to_string(project.path().join("justfile")).unwrap();
    assert_eq!(
        before, after,
        "file should be byte-identical after a no-op update"
    );
}

#[test]
fn update_with_no_arg_updates_all_installed_recipes() {
    let project = project_with_justfile("default:\n    @echo hi\n");
    let index = sample_index_url();

    for recipe in ["postgres-dev", "rust-lint-format"] {
        jtr()
            .current_dir(project.path())
            .env("JTR_INDEX_URL", &index)
            .args(["install", recipe])
            .assert()
            .success();
    }

    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", &index)
        .arg("update")
        .assert()
        .success()
        .stdout(str::contains("postgres-dev"))
        .stdout(str::contains("rust-lint-format"));
}

#[test]
fn update_uninstalled_recipe_errors_with_hint() {
    let project = project_with_justfile("default:\n    @echo hi\n");

    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", sample_index_url())
        .args(["update", "postgres-dev"])
        .assert()
        .failure()
        .stderr(str::contains("not installed"))
        .stderr(str::contains("jtr install"));
}

#[test]
fn update_with_empty_project_is_friendly_noop() {
    let project = project_with_justfile("default:\n    @echo hi\n");

    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", sample_index_url())
        .arg("update")
        .assert()
        .success()
        .stdout(str::contains("no jtr-managed recipes installed"));
}

#[test]
fn update_refreshes_block_when_user_has_hand_edited_it() {
    let project = project_with_justfile("default:\n    @echo hi\n");

    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", sample_index_url())
        .args(["install", "postgres-dev"])
        .assert()
        .success();

    let installed = fs::read_to_string(project.path().join("justfile")).unwrap();
    let tampered = installed.replace("postgres-up:", "postgres-up: # USER EDIT");
    assert_ne!(installed, tampered, "tamper must actually change content");
    fs::write(project.path().join("justfile"), &tampered).unwrap();

    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", sample_index_url())
        .args(["update", "postgres-dev"])
        .assert()
        .success()
        .stdout(str::contains("refreshed"));

    let after = fs::read_to_string(project.path().join("justfile")).unwrap();
    assert!(
        !after.contains("USER EDIT"),
        "manual edit should be reverted"
    );
    assert_eq!(
        after, installed,
        "result should match the canonical install"
    );
}

#[test]
fn init_creates_justfile_in_empty_dir() {
    let project = TempDir::new().unwrap();

    jtr()
        .current_dir(project.path())
        .arg("init")
        .assert()
        .success()
        .stdout(str::contains("created"))
        .stdout(str::contains("justfile"));

    let body = fs::read_to_string(project.path().join("justfile")).unwrap();
    assert!(
        body.contains("default:"),
        "scaffolded justfile body: {body}"
    );
    assert!(
        body.contains("just --list"),
        "scaffolded justfile body: {body}"
    );
}

#[test]
fn init_with_target_task_creates_taskfile() {
    let project = TempDir::new().unwrap();

    jtr()
        .current_dir(project.path())
        .args(["init", "--target", "task"])
        .assert()
        .success()
        .stdout(str::contains("Taskfile.yml"));

    let body = fs::read_to_string(project.path().join("Taskfile.yml")).unwrap();
    assert!(
        body.contains("version: '3'"),
        "scaffolded Taskfile body: {body}"
    );
    assert!(
        body.contains("default:"),
        "scaffolded Taskfile body: {body}"
    );
    assert!(
        body.contains("task --list"),
        "scaffolded Taskfile body: {body}"
    );
}

#[test]
fn init_refuses_to_overwrite_existing_justfile() {
    let project = TempDir::new().unwrap();
    fs::write(
        project.path().join("justfile"),
        "my-recipe:\n    @echo mine\n",
    )
    .unwrap();

    jtr()
        .current_dir(project.path())
        .arg("init")
        .assert()
        .failure()
        .stderr(str::contains("already exists"))
        .stderr(str::contains("refusing to overwrite"));

    let preserved = fs::read_to_string(project.path().join("justfile")).unwrap();
    assert_eq!(
        preserved, "my-recipe:\n    @echo mine\n",
        "existing justfile must not be touched"
    );
}

#[test]
fn init_install_against_freshly_scaffolded_justfile() {
    // The whole point of `jtr init` is letting a fresh project use `jtr install`
    // immediately afterward. Make sure the round trip actually works end-to-end.
    let project = TempDir::new().unwrap();

    jtr()
        .current_dir(project.path())
        .arg("init")
        .assert()
        .success();

    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", sample_index_url())
        .args(["install", "postgres-dev"])
        .assert()
        .success();

    let body = fs::read_to_string(project.path().join("justfile")).unwrap();
    assert!(body.contains("# >>> jtr:postgres-dev@0.1.0 >>>"));
    assert!(
        body.contains("default:"),
        "scaffold-template must survive install"
    );
}

#[test]
fn doctor_is_a_friendly_noop_when_nothing_is_installed() {
    let project = project_with_justfile("default:\n    @echo hi\n");

    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", sample_index_url())
        .arg("doctor")
        .assert()
        .success()
        .stdout(str::contains("no jtr-managed recipes"));
}

#[test]
fn doctor_passes_when_installed_recipe_is_current_and_tools_present() {
    let project = project_with_justfile("default:\n    @echo hi\n");
    let registry_dir = TempDir::new().unwrap();
    // `sh` is guaranteed present on every Unix CI runner.
    let index = write_doctor_index(registry_dir.path(), "shtool", "0.1.0", &["sh"]);

    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", &index)
        .args(["install", "shtool"])
        .assert()
        .success();

    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", &index)
        .arg("doctor")
        .assert()
        .success()
        .stdout(str::contains("up to date"))
        .stdout(str::contains("all checks passed"));
}

#[test]
fn doctor_reports_orphaned_block_and_fails() {
    let project = project_with_justfile("default:\n    @echo hi\n");
    let registry_dir = TempDir::new().unwrap();
    let install_index = write_doctor_index(registry_dir.path(), "shtool", "0.1.0", &["sh"]);

    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", &install_index)
        .args(["install", "shtool"])
        .assert()
        .success();

    // Switch to an index that no longer ships `shtool`.
    let empty_dir = TempDir::new().unwrap();
    let empty_index = write_empty_index(empty_dir.path());

    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", &empty_index)
        .arg("doctor")
        .assert()
        .failure()
        .stdout(str::contains("no longer in the registry"))
        .stdout(str::contains("shtool"));
}

#[test]
fn doctor_detects_version_drift_and_fails() {
    let project = project_with_justfile("default:\n    @echo hi\n");
    let registry_dir = TempDir::new().unwrap();

    let v1 = write_doctor_index(registry_dir.path(), "shtool", "0.1.0", &["sh"]);
    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", &v1)
        .args(["install", "shtool"])
        .assert()
        .success();

    // Bump the published version without updating the installed block.
    let v2 = write_doctor_index(registry_dir.path(), "shtool", "0.2.0", &["sh"]);

    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", &v2)
        .arg("doctor")
        .assert()
        .failure()
        .stdout(str::contains("newer version available"))
        .stdout(str::contains("0.2.0"));
}

#[test]
fn doctor_reports_missing_tool_and_fails() {
    let project = project_with_justfile("default:\n    @echo hi\n");
    let registry_dir = TempDir::new().unwrap();
    let index = write_doctor_index(
        registry_dir.path(),
        "needs-fake-tool",
        "0.1.0",
        &["definitely-not-a-real-tool-xyz123"],
    );

    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", &index)
        .args(["install", "needs-fake-tool"])
        .assert()
        .success();

    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", &index)
        .arg("doctor")
        .assert()
        .failure()
        .stdout(str::contains("definitely-not-a-real-tool-xyz123"))
        .stdout(str::contains("not found in PATH"));
}

#[test]
fn tap_add_list_remove_roundtrip() {
    let config_dir = TempDir::new().unwrap();

    jtr()
        .env("JTR_CONFIG_DIR", config_dir.path())
        .args(["tap", "list"])
        .assert()
        .success()
        .stdout(str::contains("no taps configured"));

    jtr()
        .env("JTR_CONFIG_DIR", config_dir.path())
        .args([
            "tap",
            "add",
            "alice/recipes",
            "--url",
            "file:///tmp/fake-index.json",
        ])
        .assert()
        .success()
        .stdout(str::contains("added tap"))
        .stdout(str::contains("alice/recipes"));

    let taps_file = fs::read_to_string(config_dir.path().join("taps.toml")).unwrap();
    assert!(taps_file.contains("alice/recipes"));
    assert!(taps_file.contains("file:///tmp/fake-index.json"));

    jtr()
        .env("JTR_CONFIG_DIR", config_dir.path())
        .args(["tap", "list"])
        .assert()
        .success()
        .stdout(str::contains("alice/recipes"));

    // Re-adding the same tap+URL is idempotent, not an error.
    jtr()
        .env("JTR_CONFIG_DIR", config_dir.path())
        .args([
            "tap",
            "add",
            "alice/recipes",
            "--url",
            "file:///tmp/fake-index.json",
        ])
        .assert()
        .success()
        .stdout(str::contains("already configured"));

    jtr()
        .env("JTR_CONFIG_DIR", config_dir.path())
        .args(["tap", "remove", "alice/recipes"])
        .assert()
        .success()
        .stdout(str::contains("removed tap"));

    jtr()
        .env("JTR_CONFIG_DIR", config_dir.path())
        .args(["tap", "list"])
        .assert()
        .success()
        .stdout(str::contains("no taps configured"));
}

#[test]
fn tap_add_rejects_bad_names() {
    let config_dir = TempDir::new().unwrap();

    jtr()
        .env("JTR_CONFIG_DIR", config_dir.path())
        .args(["tap", "add", "not-a-user-repo-pair"])
        .assert()
        .failure()
        .stderr(str::contains("user/repo"));

    jtr()
        .env("JTR_CONFIG_DIR", config_dir.path())
        .args(["tap", "add", "too/many/slashes"])
        .assert()
        .failure()
        .stderr(str::contains("user/repo"));
}

#[test]
fn tap_remove_unknown_errors() {
    let config_dir = TempDir::new().unwrap();

    jtr()
        .env("JTR_CONFIG_DIR", config_dir.path())
        .args(["tap", "remove", "ghost/recipes"])
        .assert()
        .failure()
        .stderr(str::contains("not configured"));
}

#[test]
fn tap_add_branch_override_uses_branch_in_url() {
    let config_dir = TempDir::new().unwrap();

    jtr()
        .env("JTR_CONFIG_DIR", config_dir.path())
        .args(["tap", "add", "alice/recipes@release/v1"])
        .assert()
        .success()
        .stdout(str::contains("added tap"))
        // The stored tap name stays `user/repo`; the branch only shapes the URL.
        .stdout(str::contains("alice/recipes"))
        .stdout(str::contains(
            "raw.githubusercontent.com/alice/recipes/release/v1/index.json",
        ));

    let taps_file = fs::read_to_string(config_dir.path().join("taps.toml")).unwrap();
    // Name persisted without the @branch suffix — otherwise `jtr install
    // alice/recipes/<recipe>` would no longer resolve against this tap.
    assert!(taps_file.contains("\"alice/recipes\""));
    assert!(!taps_file.contains("@release"));
    assert!(taps_file.contains("/release/v1/index.json"));
}

#[test]
fn tap_add_rejects_empty_and_malformed_branch() {
    let config_dir = TempDir::new().unwrap();

    jtr()
        .env("JTR_CONFIG_DIR", config_dir.path())
        .args(["tap", "add", "alice/recipes@"])
        .assert()
        .failure()
        .stderr(str::contains("branch is empty"));

    jtr()
        .env("JTR_CONFIG_DIR", config_dir.path())
        .args(["tap", "add", "alice/recipes@bad branch"])
        .assert()
        .failure()
        .stderr(str::contains("outside"));
}

#[test]
fn tap_add_url_overrides_branch_with_warning() {
    let config_dir = TempDir::new().unwrap();

    jtr()
        .env("JTR_CONFIG_DIR", config_dir.path())
        .args([
            "tap",
            "add",
            "alice/recipes@feature-x",
            "--url",
            "file:///tmp/explicit-index.json",
        ])
        .assert()
        .success()
        .stderr(str::contains("--url takes precedence"))
        .stdout(str::contains("file:///tmp/explicit-index.json"));

    let taps_file = fs::read_to_string(config_dir.path().join("taps.toml")).unwrap();
    assert!(taps_file.contains("file:///tmp/explicit-index.json"));
    assert!(!taps_file.contains("feature-x"));
}

#[test]
fn tap_remove_blocked_by_installed_block_then_force() {
    let config_dir = TempDir::new().unwrap();
    let tap_dir = TempDir::new().unwrap();
    let tap_index = write_tap_index(tap_dir.path(), "fancy-build", "0.1.0", "tap-marker");
    let project = project_with_justfile("default:\n    @echo hi\n");

    jtr()
        .env("JTR_CONFIG_DIR", config_dir.path())
        .args(["tap", "add", "alice/recipes", "--url", &tap_index])
        .assert()
        .success();

    jtr()
        .current_dir(project.path())
        .env("JTR_CONFIG_DIR", config_dir.path())
        .env("JTR_INDEX_URL", sample_index_url())
        .args(["install", "alice/recipes/fancy-build"])
        .assert()
        .success();

    // From the project dir, removing the tap is blocked by the installed block.
    jtr()
        .current_dir(project.path())
        .env("JTR_CONFIG_DIR", config_dir.path())
        .args(["tap", "remove", "alice/recipes"])
        .assert()
        .failure()
        .stderr(str::contains("alice/recipes/fancy-build"))
        .stderr(str::contains("--force"));

    // The blocked remove didn't persist — the tap is still configured.
    jtr()
        .env("JTR_CONFIG_DIR", config_dir.path())
        .args(["tap", "list"])
        .assert()
        .success()
        .stdout(str::contains("alice/recipes"));

    // --force drops the tap and orphans the block.
    jtr()
        .current_dir(project.path())
        .env("JTR_CONFIG_DIR", config_dir.path())
        .args(["tap", "remove", "alice/recipes", "--force"])
        .assert()
        .success()
        .stdout(str::contains("removed tap"));

    // Tap gone; the managed block remains in the justfile (now orphaned).
    let body = fs::read_to_string(project.path().join("justfile")).unwrap();
    assert!(body.contains("# >>> jtr:alice/recipes/fancy-build@"));
}

#[test]
fn tap_remove_guard_ignores_sibling_tap_blocks() {
    // A block from `alice/recipes-extra` must not block removing `alice/recipes`
    // — the trailing-slash boundary in block_belongs_to_tap is what guarantees it.
    let config_dir = TempDir::new().unwrap();
    let tap_dir = TempDir::new().unwrap();
    let tap_index = write_tap_index(tap_dir.path(), "fancy-build", "0.1.0", "extra-marker");
    let project = project_with_justfile("default:\n    @echo hi\n");

    jtr()
        .env("JTR_CONFIG_DIR", config_dir.path())
        .args(["tap", "add", "alice/recipes-extra", "--url", &tap_index])
        .assert()
        .success();
    // Configure the lookalike tap too, so removing it is a legal operation.
    jtr()
        .env("JTR_CONFIG_DIR", config_dir.path())
        .args(["tap", "add", "alice/recipes", "--url", &tap_index])
        .assert()
        .success();

    jtr()
        .current_dir(project.path())
        .env("JTR_CONFIG_DIR", config_dir.path())
        .env("JTR_INDEX_URL", sample_index_url())
        .args(["install", "alice/recipes-extra/fancy-build"])
        .assert()
        .success();

    // Removing `alice/recipes` (no blocks) is not blocked by the
    // `alice/recipes-extra/...` block.
    jtr()
        .current_dir(project.path())
        .env("JTR_CONFIG_DIR", config_dir.path())
        .args(["tap", "remove", "alice/recipes"])
        .assert()
        .success()
        .stdout(str::contains("removed tap"));
}

#[test]
fn tap_add_probe_reports_recipe_count() {
    let config_dir = TempDir::new().unwrap();
    let tap_dir = TempDir::new().unwrap();
    let tap_index = write_tap_index(tap_dir.path(), "fancy-build", "0.1.0", "probe-marker");

    jtr()
        .env("JTR_CONFIG_DIR", config_dir.path())
        .args([
            "tap",
            "add",
            "alice/recipes",
            "--probe",
            "--url",
            &tap_index,
        ])
        .assert()
        .success()
        .stdout(str::contains("reachable, 1 recipe"))
        .stdout(str::contains("added tap"));
}

#[test]
fn tap_add_probe_failure_does_not_persist() {
    let config_dir = TempDir::new().unwrap();

    jtr()
        .env("JTR_CONFIG_DIR", config_dir.path())
        .args([
            "tap",
            "add",
            "ghost/recipes",
            "--probe",
            "--url",
            "file:///definitely/nonexistent/index.json",
        ])
        .assert()
        .failure()
        .stderr(str::contains("probe"));

    // Probe ran before persisting, so nothing was written.
    jtr()
        .env("JTR_CONFIG_DIR", config_dir.path())
        .args(["tap", "list"])
        .assert()
        .success()
        .stdout(str::contains("no taps configured"));
}

#[test]
fn search_spans_curated_and_tap_with_source_labels() {
    let config_dir = TempDir::new().unwrap();
    let tap_dir = TempDir::new().unwrap();
    let tap_index = write_tap_index(tap_dir.path(), "tap-only-recipe", "0.1.0", "from-alice");

    jtr()
        .env("JTR_CONFIG_DIR", config_dir.path())
        .args(["tap", "add", "alice/recipes", "--url", &tap_index])
        .assert()
        .success();

    let assert = jtr()
        .env("JTR_CONFIG_DIR", config_dir.path())
        .env("JTR_INDEX_URL", sample_index_url())
        .arg("search")
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();

    // Curated recipes are still listed by bare name.
    assert!(
        stdout.contains("postgres-dev"),
        "expected curated postgres-dev in search output:\n{stdout}"
    );
    // Tap recipes appear with the `tap-name/recipe` display form so users can paste them into install.
    assert!(
        stdout.contains("alice/recipes/tap-only-recipe"),
        "expected tap-prefixed recipe name in search output:\n{stdout}"
    );
    // The source label column gives the provenance.
    assert!(
        stdout.contains("curated"),
        "expected 'curated' label in search output:\n{stdout}"
    );
    assert!(
        stdout.contains("alice/recipes"),
        "expected tap label in search output:\n{stdout}"
    );
}

#[test]
fn install_from_tap_writes_prefixed_block() {
    let config_dir = TempDir::new().unwrap();
    let tap_dir = TempDir::new().unwrap();
    let tap_index = write_tap_index(tap_dir.path(), "fancy-build", "0.1.0", "tap-marker");
    let project = project_with_justfile("default:\n    @echo hi\n");

    jtr()
        .env("JTR_CONFIG_DIR", config_dir.path())
        .args(["tap", "add", "alice/recipes", "--url", &tap_index])
        .assert()
        .success();

    jtr()
        .current_dir(project.path())
        .env("JTR_CONFIG_DIR", config_dir.path())
        .env("JTR_INDEX_URL", sample_index_url())
        .args(["install", "alice/recipes/fancy-build"])
        .assert()
        .success()
        .stdout(str::contains("installed"))
        .stdout(str::contains("alice/recipes/fancy-build"));

    let result = fs::read_to_string(project.path().join("justfile")).unwrap();
    assert!(result.contains("# >>> jtr:alice/recipes/fancy-build@0.1.0 >>>"));
    assert!(result.contains("# <<< jtr:alice/recipes/fancy-build <<<"));
    assert!(result.contains("tap-marker"));
    assert!(
        result.contains("default:\n    @echo hi"),
        "original content preserved"
    );

    // list should surface the tap-prefixed block name.
    jtr()
        .current_dir(project.path())
        .arg("list")
        .assert()
        .success()
        .stdout(str::contains("alice/recipes/fancy-build"));

    // Removing the tap block uses its full prefixed name.
    jtr()
        .current_dir(project.path())
        .args(["remove", "alice/recipes/fancy-build"])
        .assert()
        .success();

    let after_remove = fs::read_to_string(project.path().join("justfile")).unwrap();
    assert!(!after_remove.contains("alice/recipes/fancy-build"));
}

#[test]
fn install_tap_recipe_when_tap_is_not_configured_errors() {
    let config_dir = TempDir::new().unwrap();
    let project = project_with_justfile("default:\n    @echo hi\n");

    jtr()
        .current_dir(project.path())
        .env("JTR_CONFIG_DIR", config_dir.path())
        .env("JTR_INDEX_URL", sample_index_url())
        .args(["install", "ghost/repo/anything"])
        .assert()
        .failure()
        .stderr(str::contains("not found"));
}

#[test]
fn update_refreshes_tap_recipe_to_new_version() {
    let config_dir = TempDir::new().unwrap();
    let tap_dir = TempDir::new().unwrap();
    let project = project_with_justfile("default:\n    @echo hi\n");

    let v1 = write_tap_index(tap_dir.path(), "thing", "0.1.0", "v1-marker");
    jtr()
        .env("JTR_CONFIG_DIR", config_dir.path())
        .args(["tap", "add", "alice/recipes", "--url", &v1])
        .assert()
        .success();

    jtr()
        .current_dir(project.path())
        .env("JTR_CONFIG_DIR", config_dir.path())
        .env("JTR_INDEX_URL", sample_index_url())
        .args(["install", "alice/recipes/thing"])
        .assert()
        .success();

    // Republish the same tap index at a newer version.
    let _v2 = write_tap_index(tap_dir.path(), "thing", "0.2.0", "v2-marker");

    jtr()
        .current_dir(project.path())
        .env("JTR_CONFIG_DIR", config_dir.path())
        .env("JTR_INDEX_URL", sample_index_url())
        .args(["update", "alice/recipes/thing"])
        .assert()
        .success()
        .stdout(str::contains("updated"))
        .stdout(str::contains("0.1.0"))
        .stdout(str::contains("0.2.0"));

    let after = fs::read_to_string(project.path().join("justfile")).unwrap();
    assert!(after.contains("# >>> jtr:alice/recipes/thing@0.2.0 >>>"));
    assert!(after.contains("v2-marker"));
    assert!(!after.contains("v1-marker"));
}

#[test]
fn doctor_treats_tap_block_as_first_class() {
    let config_dir = TempDir::new().unwrap();
    let tap_dir = TempDir::new().unwrap();
    let project = project_with_justfile("default:\n    @echo hi\n");

    let tap_index = write_tap_index(tap_dir.path(), "thing", "0.1.0", "tap-marker");
    jtr()
        .env("JTR_CONFIG_DIR", config_dir.path())
        .args(["tap", "add", "alice/recipes", "--url", &tap_index])
        .assert()
        .success();

    jtr()
        .current_dir(project.path())
        .env("JTR_CONFIG_DIR", config_dir.path())
        .env("JTR_INDEX_URL", sample_index_url())
        .args(["install", "alice/recipes/thing"])
        .assert()
        .success();

    // doctor passes — the tap is reachable and the version matches.
    jtr()
        .current_dir(project.path())
        .env("JTR_CONFIG_DIR", config_dir.path())
        .env("JTR_INDEX_URL", sample_index_url())
        .arg("doctor")
        .assert()
        .success()
        .stdout(str::contains("up to date"))
        .stdout(str::contains("alice/recipes/thing"))
        .stdout(str::contains("all checks passed"));
}

/// Build a multi-recipe temp index where each recipe can declare dependencies on
/// other recipes in the same index. `recipes` is a slice of `(name, version, deps)`.
fn write_dep_index(dir: &Path, recipes: &[(&str, &str, &[&str])]) -> String {
    let recipes_dir = dir.join("recipes");
    fs::create_dir_all(&recipes_dir).unwrap();

    let mut index_entries = Vec::new();
    for (name, version, deps) in recipes {
        let deps_json = deps
            .iter()
            .map(|d| format!("\"{}\"", d))
            .collect::<Vec<_>>()
            .join(", ");
        let manifest = format!(
            r#"{{
  "name": "{name}",
  "version": "{version}",
  "description": "dep fixture {name}",
  "dependencies": [{deps_json}],
  "targets": {{
    "just": {{
      "snippet": "{name}-noop:\n    @echo {name}\n"
    }}
  }}
}}"#
        );
        fs::write(recipes_dir.join(format!("{name}.json")), &manifest).unwrap();
        let sha = sha256_hex(manifest.as_bytes());
        index_entries.push(format!(
            r#"    {{
      "name": "{name}",
      "version": "{version}",
      "description": "dep fixture {name}",
      "manifest_url": "recipes/{name}.json",
      "targets": ["just"],
      "sha256": "{sha}"
    }}"#
        ));
    }

    let index = format!(
        r#"{{
  "version": 1,
  "recipes": [
{}
  ]
}}"#,
        index_entries.join(",\n")
    );
    fs::write(dir.join("index.json"), index).unwrap();
    format!("file://{}/index.json", dir.display())
}

#[test]
fn install_pulls_in_transitive_dependencies() {
    // A depends on B; installing A must install B first and then A.
    let registry_dir = TempDir::new().unwrap();
    let index = write_dep_index(
        registry_dir.path(),
        &[("a", "0.1.0", &["b"]), ("b", "0.1.0", &[])],
    );
    let project = project_with_justfile("default:\n    @echo hi\n");

    let assert = jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", &index)
        .args(["install", "a"])
        .assert()
        .success()
        .stdout(str::contains("installed").count(2))
        .stdout(str::contains("(dependency)"));

    let body = fs::read_to_string(project.path().join("justfile")).unwrap();
    assert!(body.contains("# >>> jtr:b@0.1.0 >>>"));
    assert!(body.contains("# >>> jtr:a@0.1.0 >>>"));
    // The dependency block must appear before the dependent's block.
    let pos_b = body.find("# >>> jtr:b@").unwrap();
    let pos_a = body.find("# >>> jtr:a@").unwrap();
    assert!(
        pos_b < pos_a,
        "dependency b should be installed before dependent a"
    );
    // The dependent's depends-on line is recorded in the block header.
    assert!(body.contains("# depends-on: b"));

    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(
        stdout.contains("(dependency)"),
        "transitive install should be labeled (dependency):\n{stdout}"
    );
}

#[test]
fn install_resolves_diamond_dependencies_without_duplicating() {
    // A → B, A → C, B → D, C → D — install A and D must appear exactly once.
    let registry_dir = TempDir::new().unwrap();
    let index = write_dep_index(
        registry_dir.path(),
        &[
            ("a", "0.1.0", &["b", "c"]),
            ("b", "0.1.0", &["d"]),
            ("c", "0.1.0", &["d"]),
            ("d", "0.1.0", &[]),
        ],
    );
    let project = project_with_justfile("default:\n    @echo hi\n");

    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", &index)
        .args(["install", "a"])
        .assert()
        .success();

    let body = fs::read_to_string(project.path().join("justfile")).unwrap();
    let occurrences = body.matches("# >>> jtr:d@").count();
    assert_eq!(occurrences, 1, "d should be installed exactly once");
    for n in ["a", "b", "c", "d"] {
        assert!(
            body.contains(&format!("# >>> jtr:{n}@0.1.0 >>>")),
            "{n} missing from {body}"
        );
    }
}

#[test]
fn install_errors_on_cycle_with_both_endpoints_in_message() {
    // A → B → A is a cycle. The error must name both endpoints so the user can fix it.
    let registry_dir = TempDir::new().unwrap();
    let index = write_dep_index(
        registry_dir.path(),
        &[("a", "0.1.0", &["b"]), ("b", "0.1.0", &["a"])],
    );
    let project = project_with_justfile("default:\n    @echo hi\n");

    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", &index)
        .args(["install", "a"])
        .assert()
        .failure()
        .stderr(str::contains("cycle"))
        .stderr(str::contains("a"))
        .stderr(str::contains("b"));
}

#[test]
fn install_errors_on_self_loop() {
    let registry_dir = TempDir::new().unwrap();
    let index = write_dep_index(registry_dir.path(), &[("looper", "0.1.0", &["looper"])]);
    let project = project_with_justfile("default:\n    @echo hi\n");

    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", &index)
        .args(["install", "looper"])
        .assert()
        .failure()
        .stderr(str::contains("cycle"))
        .stderr(str::contains("looper"));
}

#[test]
fn install_dep_already_present_is_idempotent() {
    // If the user has manually installed B, then installs A which depends on B, B
    // must not be duplicated and the install should still succeed.
    let registry_dir = TempDir::new().unwrap();
    let index = write_dep_index(
        registry_dir.path(),
        &[("a", "0.1.0", &["b"]), ("b", "0.1.0", &[])],
    );
    let project = project_with_justfile("default:\n    @echo hi\n");

    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", &index)
        .args(["install", "b"])
        .assert()
        .success();

    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", &index)
        .args(["install", "a"])
        .assert()
        .success();

    let body = fs::read_to_string(project.path().join("justfile")).unwrap();
    let b_count = body.matches("# >>> jtr:b@").count();
    assert_eq!(
        b_count, 1,
        "b should appear exactly once after a was installed"
    );
    assert!(body.contains("# >>> jtr:a@"));
}

#[test]
fn install_tap_recipe_with_curated_dependency_resolves_cross_source() {
    // A tap recipe declares a dep on a curated recipe — resolution must cross source boundaries.
    let registry_dir = TempDir::new().unwrap();
    let curated = write_dep_index(registry_dir.path(), &[("base", "0.1.0", &[])]);

    let tap_dir = TempDir::new().unwrap();
    let tap_index = write_dep_index(tap_dir.path(), &[("downstream", "0.1.0", &["base"])]);

    let config_dir = TempDir::new().unwrap();
    jtr()
        .env("JTR_CONFIG_DIR", config_dir.path())
        .args(["tap", "add", "alice/recipes", "--url", &tap_index])
        .assert()
        .success();

    let project = project_with_justfile("default:\n    @echo hi\n");
    jtr()
        .current_dir(project.path())
        .env("JTR_CONFIG_DIR", config_dir.path())
        .env("JTR_INDEX_URL", &curated)
        .args(["install", "alice/recipes/downstream"])
        .assert()
        .success();

    let body = fs::read_to_string(project.path().join("justfile")).unwrap();
    // Curated dep is written by its bare name; tap dependent uses its tap-prefixed block name.
    assert!(body.contains("# >>> jtr:base@0.1.0 >>>"));
    assert!(body.contains("# >>> jtr:alice/recipes/downstream@0.1.0 >>>"));
}

#[test]
fn install_errors_when_dep_references_unconfigured_tap() {
    // A curated recipe declares a dep on a tap-qualified name whose tap isn't configured.
    // The error must name the missing tap and point the user at `jtr tap add`.
    let registry_dir = TempDir::new().unwrap();
    let index = write_dep_index(
        registry_dir.path(),
        &[("needs-ext", "0.1.0", &["ghost/repo/missing"])],
    );
    let config_dir = TempDir::new().unwrap();
    let project = project_with_justfile("default:\n    @echo hi\n");

    jtr()
        .current_dir(project.path())
        .env("JTR_CONFIG_DIR", config_dir.path())
        .env("JTR_INDEX_URL", &index)
        .args(["install", "needs-ext"])
        .assert()
        .failure()
        .stderr(str::contains("ghost/repo"))
        .stderr(str::contains("not configured"))
        .stderr(str::contains("jtr tap add"));
}

#[test]
fn remove_blocks_when_dependents_are_installed() {
    let registry_dir = TempDir::new().unwrap();
    let index = write_dep_index(
        registry_dir.path(),
        &[("a", "0.1.0", &["b"]), ("b", "0.1.0", &[])],
    );
    let project = project_with_justfile("default:\n    @echo hi\n");

    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", &index)
        .args(["install", "a"])
        .assert()
        .success();

    jtr()
        .current_dir(project.path())
        .args(["remove", "b"])
        .assert()
        .failure()
        .stderr(str::contains("depend"))
        .stderr(str::contains("a"))
        .stderr(str::contains("--force"));

    // Justfile is unchanged.
    let body = fs::read_to_string(project.path().join("justfile")).unwrap();
    assert!(body.contains("# >>> jtr:b@"));
    assert!(body.contains("# >>> jtr:a@"));
}

#[test]
fn remove_force_proceeds_despite_dependents() {
    let registry_dir = TempDir::new().unwrap();
    let index = write_dep_index(
        registry_dir.path(),
        &[("a", "0.1.0", &["b"]), ("b", "0.1.0", &[])],
    );
    let project = project_with_justfile("default:\n    @echo hi\n");

    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", &index)
        .args(["install", "a"])
        .assert()
        .success();

    jtr()
        .current_dir(project.path())
        .args(["remove", "b", "--force"])
        .assert()
        .success()
        .stdout(str::contains("removed"));

    let body = fs::read_to_string(project.path().join("justfile")).unwrap();
    assert!(
        !body.contains("# >>> jtr:b@"),
        "b should be gone after --force"
    );
    assert!(body.contains("# >>> jtr:a@"), "a should remain");
}

/// Build a multi-version index for one recipe. The first entry in `versions` is
/// the "latest" (top-level fields); every entry is also enumerated under
/// `versions: [...]` so the CLI can resolve pins to any of them. Each manifest's
/// snippet embeds its own version as a marker so tests can assert which one
/// landed on disk.
fn write_multi_version_index(dir: &Path, recipe: &str, versions: &[&str]) -> String {
    assert!(!versions.is_empty(), "need at least one version");
    let recipes_dir = dir.join("recipes");
    fs::create_dir_all(&recipes_dir).unwrap();

    let mut version_entries: Vec<String> = Vec::new();
    let mut latest_manifest_url = String::new();
    let mut latest_sha = String::new();

    for (idx, version) in versions.iter().enumerate() {
        let manifest = format!(
            r#"{{
  "name": "{recipe}",
  "version": "{version}",
  "description": "fixture {recipe} v{version}",
  "shells_out_to": [],
  "targets": {{
    "just": {{
      "snippet": "{recipe}-noop:\n    @echo marker-{version}\n"
    }}
  }}
}}"#
        );
        let path = if idx == 0 {
            // Latest goes at the canonical path so an older CLI (no `versions:`) still works.
            format!("{recipe}.json")
        } else {
            format!("{recipe}/{version}.json")
        };
        let on_disk = recipes_dir.join(&path);
        fs::create_dir_all(on_disk.parent().unwrap()).unwrap();
        fs::write(&on_disk, &manifest).unwrap();
        let sha = sha256_hex(manifest.as_bytes());

        if idx == 0 {
            latest_manifest_url = format!("recipes/{path}");
            latest_sha = sha.clone();
        }

        version_entries.push(format!(
            r#"{{
        "version": "{version}",
        "manifest_url": "recipes/{path}",
        "sha256": "{sha}"
      }}"#
        ));
    }

    let latest_version = versions[0];
    let index = format!(
        r#"{{
  "version": 1,
  "recipes": [
    {{
      "name": "{recipe}",
      "version": "{latest_version}",
      "description": "fixture {recipe}",
      "manifest_url": "{latest_manifest_url}",
      "targets": ["just"],
      "sha256": "{latest_sha}",
      "versions": [
        {}
      ]
    }}
  ]
}}"#,
        version_entries.join(",\n        ")
    );
    fs::write(dir.join("index.json"), index).unwrap();

    format!("file://{}/index.json", dir.display())
}

#[test]
fn install_with_pin_writes_the_requested_version() {
    // Latest is 0.2.0 but the user asks for 0.1.0 — the older snippet (marker-0.1.0)
    // must land on disk, the block must declare the pin, and the install output
    // must call out that the install was pinned.
    let index_dir = TempDir::new().unwrap();
    let index_url = write_multi_version_index(index_dir.path(), "demo", &["0.2.0", "0.1.0"]);
    let project = project_with_justfile("default:\n    @echo hi\n");

    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", &index_url)
        .args(["install", "demo@0.1.0"])
        .assert()
        .success()
        .stdout(str::contains("(pinned)"));

    let body = fs::read_to_string(project.path().join("justfile")).unwrap();
    assert!(
        body.contains("# >>> jtr:demo@0.1.0 >>>"),
        "block header should record the pinned version, got:\n{body}"
    );
    assert!(
        body.contains("# pinned: 0.1.0"),
        "block should record the pin marker, got:\n{body}"
    );
    assert!(
        body.contains("marker-0.1.0"),
        "the 0.1.0 manifest's snippet should be on disk, got:\n{body}"
    );
    assert!(
        !body.contains("marker-0.2.0"),
        "0.2.0 must not have been written, got:\n{body}"
    );
}

#[test]
fn install_with_pin_to_nonexistent_version_lists_available() {
    let index_dir = TempDir::new().unwrap();
    let index_url = write_multi_version_index(index_dir.path(), "demo", &["0.2.0", "0.1.0"]);
    let project = project_with_justfile("default:\n    @echo hi\n");

    let assert = jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", &index_url)
        .args(["install", "demo@9.9.9"])
        .assert()
        .failure();
    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    assert!(
        stderr.contains("no published version '9.9.9'"),
        "error should name the missing version, got: {stderr}"
    );
    assert!(
        stderr.contains("0.1.0") && stderr.contains("0.2.0"),
        "error should list available versions, got: {stderr}"
    );

    // No change to the justfile when the pin can't be satisfied.
    let body = fs::read_to_string(project.path().join("justfile")).unwrap();
    assert_eq!(body, "default:\n    @echo hi\n");
}

#[test]
fn install_with_empty_version_after_at_errors() {
    let project = project_with_justfile("default:\n    @echo hi\n");

    let assert = jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", sample_index_url())
        .args(["install", "postgres-dev@"])
        .assert()
        .failure();
    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    assert!(
        stderr.contains("version is empty"),
        "stderr should explain the empty pin, got: {stderr}"
    );
}

#[test]
fn install_with_pin_on_index_without_versions_array_is_backwards_compatible() {
    // An old-style index that only lists a single version (no `versions: [...]`) must
    // still let users pin to that one version — the CLI falls back to the top-level
    // `version` field when matching the pin.
    let dir = TempDir::new().unwrap();
    let index_url = write_postgres_dev_index(dir.path(), "0.1.0");
    let project = project_with_justfile("default:\n    @echo hi\n");

    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", &index_url)
        .args(["install", "postgres-dev@0.1.0"])
        .assert()
        .success();
    let body = fs::read_to_string(project.path().join("justfile")).unwrap();
    assert!(body.contains("# pinned: 0.1.0"));

    // ...but pinning to a non-published version should still produce a useful error,
    // even when there's only one entry to list.
    let project2 = project_with_justfile("default:\n    @echo hi\n");
    let assert = jtr()
        .current_dir(project2.path())
        .env("JTR_INDEX_URL", &index_url)
        .args(["install", "postgres-dev@9.9.9"])
        .assert()
        .failure();
    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    assert!(
        stderr.contains("0.1.0"),
        "available list should include the lone top-level version, got: {stderr}"
    );
}

#[test]
fn update_skips_pinned_blocks_by_default() {
    let index_dir = TempDir::new().unwrap();
    let index_url = write_multi_version_index(index_dir.path(), "demo", &["0.2.0", "0.1.0"]);
    let project = project_with_justfile("default:\n    @echo hi\n");

    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", &index_url)
        .args(["install", "demo@0.1.0"])
        .assert()
        .success();

    // The pin is in place; `jtr update` (no --unpin) should refuse to bump it.
    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", &index_url)
        .args(["update", "demo"])
        .assert()
        .success()
        .stdout(str::contains("pinned to 0.1.0"))
        .stdout(str::contains("--unpin"));

    let body = fs::read_to_string(project.path().join("justfile")).unwrap();
    assert!(
        body.contains("# pinned: 0.1.0"),
        "pin must still be present after `jtr update`, got:\n{body}"
    );
    assert!(
        body.contains("marker-0.1.0"),
        "0.1.0 snippet must still be on disk, got:\n{body}"
    );
    assert!(
        !body.contains("marker-0.2.0"),
        "update must not have bumped the block, got:\n{body}"
    );
}

#[test]
fn update_unpin_bumps_to_latest_and_drops_marker() {
    let index_dir = TempDir::new().unwrap();
    let index_url = write_multi_version_index(index_dir.path(), "demo", &["0.2.0", "0.1.0"]);
    let project = project_with_justfile("default:\n    @echo hi\n");

    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", &index_url)
        .args(["install", "demo@0.1.0"])
        .assert()
        .success();

    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", &index_url)
        .args(["update", "demo", "--unpin"])
        .assert()
        .success()
        .stdout(str::contains("unpinned"));

    let body = fs::read_to_string(project.path().join("justfile")).unwrap();
    assert!(
        body.contains("# >>> jtr:demo@0.2.0 >>>"),
        "block should be on the new latest, got:\n{body}"
    );
    assert!(
        !body.contains("# pinned:"),
        "pin marker should be stripped after --unpin, got:\n{body}"
    );
    assert!(body.contains("marker-0.2.0"));
}

#[test]
fn bare_install_overrides_an_existing_pin() {
    // The user can also re-pin or unpin by re-running install. A bare `jtr install demo`
    // overwrites the pinned block with the latest, unpinned.
    let index_dir = TempDir::new().unwrap();
    let index_url = write_multi_version_index(index_dir.path(), "demo", &["0.2.0", "0.1.0"]);
    let project = project_with_justfile("default:\n    @echo hi\n");

    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", &index_url)
        .args(["install", "demo@0.1.0"])
        .assert()
        .success();

    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", &index_url)
        .args(["install", "demo"])
        .assert()
        .success();

    let body = fs::read_to_string(project.path().join("justfile")).unwrap();
    assert!(body.contains("# >>> jtr:demo@0.2.0 >>>"));
    assert!(
        !body.contains("# pinned:"),
        "bare install should drop the pin marker, got:\n{body}"
    );
}

#[test]
fn doctor_treats_pinned_blocks_at_their_pin_as_healthy() {
    let index_dir = TempDir::new().unwrap();
    let index_url = write_multi_version_index(index_dir.path(), "demo", &["0.2.0", "0.1.0"]);
    let project = project_with_justfile("default:\n    @echo hi\n");

    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", &index_url)
        .args(["install", "demo@0.1.0"])
        .assert()
        .success();

    let assert = jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", &index_url)
        .args(["doctor"])
        .assert()
        .success()
        .stdout(str::contains("pinned, up to date"))
        .stdout(str::contains("all checks passed"));
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(
        !stdout.contains("newer version available"),
        "doctor must not flag drift on a pinned block at its pinned version, got: {stdout}"
    );
}

#[test]
fn doctor_flags_pinned_version_no_longer_published() {
    // Install at 0.1.0, then drop 0.1.0 from the index entirely — doctor should
    // surface this loudly with the available list.
    let index_dir = TempDir::new().unwrap();
    let index_url = write_multi_version_index(index_dir.path(), "demo", &["0.2.0", "0.1.0"]);
    let project = project_with_justfile("default:\n    @echo hi\n");

    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", &index_url)
        .args(["install", "demo@0.1.0"])
        .assert()
        .success();

    // Rewrite the index so 0.1.0 disappears: only 0.2.0 is now published.
    write_multi_version_index(index_dir.path(), "demo", &["0.2.0"]);

    let assert = jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", &index_url)
        .args(["doctor"])
        .assert()
        .failure();
    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let combined = format!("{stdout}\n{stderr}");
    assert!(
        combined.contains("pinned to 0.1.0 but no longer published"),
        "doctor should call out unpublished pins, got: {combined}"
    );
    assert!(
        combined.contains("0.2.0"),
        "available list should include 0.2.0, got: {combined}"
    );
}

#[test]
fn install_recipe_without_task_target_into_taskfile_errors() {
    // A recipe that declares only a `just` target must fail with a clear
    // "does not support target 'task'" message when installed into a Taskfile.yml,
    // rather than writing an empty/garbage block. Uses a synthetic just-only fixture
    // so the guard is independent of the curated corpus — every curated seed recipe
    // now ships both targets (see `curated_postgres_dev_installs_into_taskfile`).
    let config_dir = TempDir::new().unwrap();
    let index_dir = TempDir::new().unwrap();
    let url = write_postgres_dev_index(index_dir.path(), "0.1.0");
    let project = TempDir::new().unwrap();
    fs::write(project.path().join("Taskfile.yml"), "version: '3'\n").unwrap();

    jtr()
        .current_dir(project.path())
        .env("JTR_CONFIG_DIR", config_dir.path())
        .env("JTR_INDEX_URL", &url)
        .args(["install", "postgres-dev"])
        .assert()
        .failure()
        .stderr(str::contains("does not support target 'task'"));
}

#[test]
fn show_prints_the_block_a_curated_install_would_write() {
    let project = project_with_justfile("default:\n    @echo hi\n");
    let config_dir = TempDir::new().unwrap();

    let assert = jtr()
        .current_dir(project.path())
        .env("JTR_CONFIG_DIR", config_dir.path())
        .env("JTR_INDEX_URL", sample_index_url())
        .args(["show", "postgres-dev"])
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();

    assert!(
        stdout.contains("# >>> jtr:postgres-dev@0.1.0 >>>"),
        "show should print the rendered open marker, got:\n{stdout}"
    );
    assert!(
        stdout.contains("# <<< jtr:postgres-dev <<<"),
        "show should print the rendered close marker, got:\n{stdout}"
    );
    assert!(
        stdout.contains("postgres-up:"),
        "show should include the recipe body, got:\n{stdout}"
    );

    // The project file is not modified.
    let after = fs::read_to_string(project.path().join("justfile")).unwrap();
    assert_eq!(after, "default:\n    @echo hi\n");
}

#[test]
fn show_prints_the_block_a_tap_install_would_write() {
    let config_dir = TempDir::new().unwrap();
    let tap_dir = TempDir::new().unwrap();
    let tap_index = write_tap_index(tap_dir.path(), "fancy-build", "0.1.0", "tap-marker");
    let project = project_with_justfile("default:\n    @echo hi\n");

    jtr()
        .env("JTR_CONFIG_DIR", config_dir.path())
        .args(["tap", "add", "alice/recipes", "--url", &tap_index])
        .assert()
        .success();

    let assert = jtr()
        .current_dir(project.path())
        .env("JTR_CONFIG_DIR", config_dir.path())
        .env("JTR_INDEX_URL", sample_index_url())
        .args(["show", "alice/recipes/fancy-build"])
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();

    assert!(
        stdout.contains("# >>> jtr:alice/recipes/fancy-build@0.1.0 >>>"),
        "tap show should use the prefixed block name, got:\n{stdout}"
    );
    assert!(stdout.contains("tap-marker"), "got:\n{stdout}");

    // The project file is not modified.
    let after = fs::read_to_string(project.path().join("justfile")).unwrap();
    assert_eq!(after, "default:\n    @echo hi\n");
}

#[test]
fn show_with_pin_picks_the_requested_version() {
    let project = project_with_justfile("default:\n    @echo hi\n");
    let index_dir = TempDir::new().unwrap();
    let index_url = write_multi_version_index(index_dir.path(), "demo", &["0.2.0", "0.1.0"]);

    let assert = jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", &index_url)
        .args(["show", "demo@0.1.0"])
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(
        stdout.contains("# >>> jtr:demo@0.1.0 >>>"),
        "show @0.1.0 should render the pinned version, got:\n{stdout}"
    );
    assert!(
        stdout.contains("# pinned: 0.1.0"),
        "show @0.1.0 should record the pin marker, got:\n{stdout}"
    );
    assert!(
        stdout.contains("marker-0.1.0"),
        "show @0.1.0 should include the 0.1.0 snippet, got:\n{stdout}"
    );
    assert!(
        !stdout.contains("marker-0.2.0"),
        "show @0.1.0 should not leak the 0.2.0 snippet, got:\n{stdout}"
    );
}

#[test]
fn show_errors_when_recipe_does_not_exist() {
    let project = project_with_justfile("default:\n    @echo hi\n");

    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", sample_index_url())
        .args(["show", "does-not-exist"])
        .assert()
        .failure()
        .stderr(str::contains("not found"));
}

#[test]
fn show_errors_when_tap_is_not_configured() {
    let project = project_with_justfile("default:\n    @echo hi\n");
    let config_dir = TempDir::new().unwrap();

    jtr()
        .current_dir(project.path())
        .env("JTR_CONFIG_DIR", config_dir.path())
        .env("JTR_INDEX_URL", sample_index_url())
        .args(["show", "ghost/repo/anything"])
        .assert()
        .failure()
        .stderr(str::contains("tap 'ghost/repo' is not configured"));
}

#[test]
fn diff_of_an_unmodified_install_exits_zero_with_no_output() {
    let project = project_with_justfile("default:\n    @echo hi\n");
    let config_dir = TempDir::new().unwrap();

    jtr()
        .current_dir(project.path())
        .env("JTR_CONFIG_DIR", config_dir.path())
        .env("JTR_INDEX_URL", sample_index_url())
        .args(["install", "postgres-dev"])
        .assert()
        .success();

    let assert = jtr()
        .current_dir(project.path())
        .env("JTR_CONFIG_DIR", config_dir.path())
        .env("JTR_INDEX_URL", sample_index_url())
        .args(["diff", "postgres-dev"])
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(
        stdout.is_empty(),
        "no-diff case should produce empty stdout, got: {stdout:?}"
    );
}

#[test]
fn diff_after_a_version_bump_exits_one_with_a_unified_diff() {
    let project = project_with_justfile("default:\n    @echo hi\n");
    let registry_dir = TempDir::new().unwrap();

    let v1 = write_postgres_dev_index(registry_dir.path(), "0.1.0");
    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", &v1)
        .args(["install", "postgres-dev"])
        .assert()
        .success();

    let v2 = write_postgres_dev_index(registry_dir.path(), "0.2.0");
    let assert = jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", &v2)
        .args(["diff", "postgres-dev"])
        .assert()
        .failure()
        .code(1);
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(
        stdout.contains("--- a/postgres-dev"),
        "diff should print a unified-diff header, got:\n{stdout}"
    );
    assert!(
        stdout.contains("+++ b/postgres-dev"),
        "diff should print a unified-diff header, got:\n{stdout}"
    );
    assert!(
        stdout.contains("-# >>> jtr:postgres-dev@0.1.0 >>>"),
        "diff should show the old version line removed, got:\n{stdout}"
    );
    assert!(
        stdout.contains("+# >>> jtr:postgres-dev@0.2.0 >>>"),
        "diff should show the new version line added, got:\n{stdout}"
    );
    assert!(
        stdout.contains("-    @echo marker-0.1.0"),
        "diff should show the old snippet removed, got:\n{stdout}"
    );
    assert!(
        stdout.contains("+    @echo marker-0.2.0"),
        "diff should show the new snippet added, got:\n{stdout}"
    );
}

#[test]
fn diff_of_uninstalled_recipe_renders_whole_block_as_additions() {
    let project = project_with_justfile("default:\n    @echo hi\n");
    let config_dir = TempDir::new().unwrap();

    let assert = jtr()
        .current_dir(project.path())
        .env("JTR_CONFIG_DIR", config_dir.path())
        .env("JTR_INDEX_URL", sample_index_url())
        .args(["diff", "postgres-dev"])
        .assert()
        .failure()
        .code(1);
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(
        stdout.contains("--- a/postgres-dev (not installed)"),
        "diff should mark the left side as not installed, got:\n{stdout}"
    );
    assert!(
        stdout.contains("+# >>> jtr:postgres-dev@0.1.0 >>>"),
        "diff should render the whole block as additions, got:\n{stdout}"
    );
    assert!(
        stdout.contains("+# <<< jtr:postgres-dev <<<"),
        "diff should render the close marker as an addition, got:\n{stdout}"
    );
}

#[test]
fn diff_of_pinned_block_compares_against_the_pin_not_latest() {
    // Install pinned to 0.1.0 while the registry already has 0.2.0. Then bump
    // the registry to 0.3.0. Plain `jtr diff demo` should still exit 0 because
    // the on-disk block matches what `install demo@0.1.0` would write — pinning
    // is a deliberate freeze, and drift against latest is *not* what the user
    // signed up for. `update --unpin` is the way out, not noisy diff output.
    let project = project_with_justfile("default:\n    @echo hi\n");
    let index_dir = TempDir::new().unwrap();
    let index_url = write_multi_version_index(index_dir.path(), "demo", &["0.2.0", "0.1.0"]);

    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", &index_url)
        .args(["install", "demo@0.1.0"])
        .assert()
        .success();

    write_multi_version_index(index_dir.path(), "demo", &["0.3.0", "0.2.0", "0.1.0"]);

    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", &index_url)
        .args(["diff", "demo"])
        .assert()
        .success();
}

#[test]
fn update_installs_newly_added_transitive_dependency() {
    // Regression for #8. Install `a` at v1 (no deps); then publish `a` at v2 with
    // a new dep on `b`; then `jtr update a` should install `b` alongside the
    // updated `a`, not just rewrite the `# depends-on:` header.
    let registry_dir = TempDir::new().unwrap();
    let index = write_dep_index(registry_dir.path(), &[("a", "0.1.0", &[])]);
    let project = project_with_justfile("default:\n    @echo hi\n");

    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", &index)
        .args(["install", "a"])
        .assert()
        .success();

    let before = fs::read_to_string(project.path().join("justfile")).unwrap();
    assert!(before.contains("# >>> jtr:a@0.1.0 >>>"));
    assert!(!before.contains("# >>> jtr:b@"));
    assert!(!before.contains("# depends-on:"));

    // a v2 declares a new dep on b.
    write_dep_index(
        registry_dir.path(),
        &[("a", "0.2.0", &["b"]), ("b", "0.1.0", &[])],
    );

    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", &index)
        .args(["update", "a"])
        .assert()
        .success()
        .stdout(str::contains("installed"))
        .stdout(str::contains("(dependency)"))
        .stdout(str::contains("updated"))
        .stdout(str::contains("0.2.0"));

    let after = fs::read_to_string(project.path().join("justfile")).unwrap();
    assert!(after.contains("# >>> jtr:a@0.2.0 >>>"), "a should be at v2");
    assert!(
        after.contains("# >>> jtr:b@0.1.0 >>>"),
        "b should be installed by `update a` (regression for #8):\n{after}"
    );
    assert!(after.contains("# depends-on: b"));
    assert!(
        after.contains("b-noop:"),
        "b's recipe body should be present"
    );

    // The dep must appear before the dependent in the file (topological order).
    let pos_b = after.find("# >>> jtr:b@").unwrap();
    let pos_a = after.find("# >>> jtr:a@0.2.0").unwrap();
    assert!(
        pos_b < pos_a,
        "transitive dep `b` should be ordered before its dependent `a`"
    );
}

#[test]
fn update_with_no_arg_installs_newly_added_transitive_dependency() {
    // Same shape as `update_installs_newly_added_transitive_dependency` but uses
    // the no-arg form (`jtr update`) — verifies the multi-block loop also picks
    // up new deps for each block it walks.
    let registry_dir = TempDir::new().unwrap();
    let index = write_dep_index(registry_dir.path(), &[("a", "0.1.0", &[])]);
    let project = project_with_justfile("default:\n    @echo hi\n");

    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", &index)
        .args(["install", "a"])
        .assert()
        .success();

    write_dep_index(
        registry_dir.path(),
        &[("a", "0.2.0", &["b"]), ("b", "0.1.0", &[])],
    );

    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", &index)
        .arg("update")
        .assert()
        .success();

    let after = fs::read_to_string(project.path().join("justfile")).unwrap();
    assert!(after.contains("# >>> jtr:a@0.2.0 >>>"));
    assert!(after.contains("# >>> jtr:b@0.1.0 >>>"));
}

#[test]
fn update_bumps_existing_transitive_dependency_to_latest() {
    // When the dep is *already* installed (unpinned) and a newer version
    // publishes, `update <dependent>` should also bump the dep. This matches
    // install's "always upsert" semantics and is the symmetric expansion called
    // out alongside #8.
    let registry_dir = TempDir::new().unwrap();
    let index = write_dep_index(
        registry_dir.path(),
        &[("a", "0.1.0", &["b"]), ("b", "0.1.0", &[])],
    );
    let project = project_with_justfile("default:\n    @echo hi\n");

    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", &index)
        .args(["install", "a"])
        .assert()
        .success();

    // Bump only b.
    write_dep_index(
        registry_dir.path(),
        &[("a", "0.1.0", &["b"]), ("b", "0.2.0", &[])],
    );

    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", &index)
        .args(["update", "a"])
        .assert()
        .success();

    let after = fs::read_to_string(project.path().join("justfile")).unwrap();
    assert!(
        after.contains("# >>> jtr:b@0.2.0 >>>"),
        "b should have been bumped via `update a`:\n{after}"
    );
    assert!(after.contains("# >>> jtr:a@0.1.0 >>>"));
}

/// Same shape as `write_dep_index` but for one of the recipes ships at multiple
/// versions and is enumerated under `versions: [...]`. Lets a test pin a
/// transitive dep to a specific version. `multi.1[0]` is the published latest.
fn write_dep_index_with_multiversion(
    dir: &Path,
    single: &[(&str, &str, &[&str])],
    multi: (&str, &[&str]),
) -> String {
    let (multi_name, multi_versions) = multi;
    assert!(!multi_versions.is_empty());
    let recipes_dir = dir.join("recipes");
    fs::create_dir_all(&recipes_dir).unwrap();

    let mut index_entries: Vec<String> = Vec::new();

    for (name, version, deps) in single {
        let deps_json = deps
            .iter()
            .map(|d| format!("\"{}\"", d))
            .collect::<Vec<_>>()
            .join(", ");
        let manifest = format!(
            r#"{{
  "name": "{name}",
  "version": "{version}",
  "description": "fixture {name}",
  "dependencies": [{deps_json}],
  "targets": {{
    "just": {{
      "snippet": "{name}-noop:\n    @echo {name}-{version}\n"
    }}
  }}
}}"#
        );
        fs::write(recipes_dir.join(format!("{name}.json")), &manifest).unwrap();
        let sha = sha256_hex(manifest.as_bytes());
        index_entries.push(format!(
            r#"    {{
      "name": "{name}",
      "version": "{version}",
      "description": "fixture {name}",
      "manifest_url": "recipes/{name}.json",
      "targets": ["just"],
      "sha256": "{sha}"
    }}"#
        ));
    }

    let mut version_entries: Vec<String> = Vec::new();
    let mut latest_manifest_url = String::new();
    let mut latest_sha = String::new();
    for (idx, version) in multi_versions.iter().enumerate() {
        let manifest = format!(
            r#"{{
  "name": "{multi_name}",
  "version": "{version}",
  "description": "fixture {multi_name} v{version}",
  "targets": {{
    "just": {{
      "snippet": "{multi_name}-noop:\n    @echo marker-{version}\n"
    }}
  }}
}}"#
        );
        let path = if idx == 0 {
            format!("{multi_name}.json")
        } else {
            format!("{multi_name}/{version}.json")
        };
        let on_disk = recipes_dir.join(&path);
        fs::create_dir_all(on_disk.parent().unwrap()).unwrap();
        fs::write(&on_disk, &manifest).unwrap();
        let sha = sha256_hex(manifest.as_bytes());
        if idx == 0 {
            latest_manifest_url = format!("recipes/{path}");
            latest_sha = sha.clone();
        }
        version_entries.push(format!(
            r#"{{
        "version": "{version}",
        "manifest_url": "recipes/{path}",
        "sha256": "{sha}"
      }}"#
        ));
    }

    let latest_version = multi_versions[0];
    index_entries.push(format!(
        r#"    {{
      "name": "{multi_name}",
      "version": "{latest_version}",
      "description": "fixture {multi_name}",
      "manifest_url": "{latest_manifest_url}",
      "targets": ["just"],
      "sha256": "{latest_sha}",
      "versions": [
        {}
      ]
    }}"#,
        version_entries.join(",\n        ")
    ));

    let index = format!(
        r#"{{
  "version": 1,
  "recipes": [
{}
  ]
}}"#,
        index_entries.join(",\n")
    );
    fs::write(dir.join("index.json"), index).unwrap();
    format!("file://{}/index.json", dir.display())
}

#[test]
fn update_respects_pinned_transitive_dependency() {
    // The blind-spot the advisor flagged on PR for #8: when a transitive dep is
    // already installed *with a pin* (via `jtr install <dep>@<version>`), a
    // subsequent `jtr update <dependent>` must NOT silently bump the dep to
    // latest. The pin propagates through `resolve_install_order` via the
    // installed_pins lookup.
    let registry_dir = TempDir::new().unwrap();
    let index = write_dep_index_with_multiversion(
        registry_dir.path(),
        &[("a", "0.1.0", &["b"])],
        ("b", &["0.2.0", "0.1.0"]),
    );
    let project = project_with_justfile("default:\n    @echo hi\n");

    // Install a — pulls in b@0.2.0 (latest, no pin).
    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", &index)
        .args(["install", "a"])
        .assert()
        .success();

    // Pin b at 0.1.0 deliberately.
    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", &index)
        .args(["install", "b@0.1.0"])
        .assert()
        .success();

    let pre = fs::read_to_string(project.path().join("justfile")).unwrap();
    assert!(pre.contains("# >>> jtr:b@0.1.0 >>>"), "pre-update:\n{pre}");
    assert!(pre.contains("# pinned: 0.1.0"));

    // Now publish a@0.2.0 — still depends on b — and run `update a`.
    let index = write_dep_index_with_multiversion(
        registry_dir.path(),
        &[("a", "0.2.0", &["b"])],
        ("b", &["0.2.0", "0.1.0"]),
    );

    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", &index)
        .args(["update", "a"])
        .assert()
        .success();

    let after = fs::read_to_string(project.path().join("justfile")).unwrap();
    assert!(
        after.contains("# >>> jtr:a@0.2.0 >>>"),
        "a should be bumped"
    );
    assert!(
        after.contains("# >>> jtr:b@0.1.0 >>>"),
        "b must stay at pinned 0.1.0, not bump to 0.2.0:\n{after}"
    );
    assert!(
        after.contains("# pinned: 0.1.0"),
        "pin marker must survive across update of dependent:\n{after}"
    );
    assert!(after.contains("marker-0.1.0"));
    assert!(!after.contains("marker-0.2.0"));
}

#[test]
fn install_respects_existing_pinned_transitive_dependency() {
    // Symmetric to `update_respects_pinned_transitive_dependency`: installing a
    // dependent whose dep is already pinned on disk must not bump the dep.
    let registry_dir = TempDir::new().unwrap();
    let index = write_dep_index_with_multiversion(
        registry_dir.path(),
        &[("a", "0.1.0", &["b"])],
        ("b", &["0.2.0", "0.1.0"]),
    );
    let project = project_with_justfile("default:\n    @echo hi\n");

    // Pin b first.
    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", &index)
        .args(["install", "b@0.1.0"])
        .assert()
        .success();

    // Now install a — its transitive dep b is already on disk pinned at 0.1.0.
    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", &index)
        .args(["install", "a"])
        .assert()
        .success();

    let body = fs::read_to_string(project.path().join("justfile")).unwrap();
    assert!(body.contains("# >>> jtr:a@0.1.0 >>>"));
    assert!(
        body.contains("# >>> jtr:b@0.1.0 >>>"),
        "b must stay at pinned 0.1.0 after `install a`:\n{body}"
    );
    assert!(body.contains("# pinned: 0.1.0"));
    assert!(body.contains("marker-0.1.0"));
    assert!(!body.contains("marker-0.2.0"));
}

// --- jtr scaffold + jtr lint -------------------------------------------------

/// Write an empty tap layout (index.json with no recipes, no `recipes/` dir).
fn empty_tap(dir: &Path) {
    let index = "{\n  \"version\": 1,\n  \"recipes\": []\n}\n";
    fs::write(dir.join("index.json"), index).unwrap();
}

#[test]
fn scaffold_recipe_in_standalone_dir_writes_only_the_manifest() {
    let dir = TempDir::new().unwrap();
    jtr()
        .current_dir(dir.path())
        .args(["scaffold", "recipe", "my-recipe"])
        .assert()
        .success()
        .stdout(str::contains("my-recipe.json"));

    let manifest_path = dir.path().join("my-recipe.json");
    assert!(manifest_path.exists(), "manifest should be at cwd root");
    assert!(
        !dir.path().join("recipes").exists(),
        "no recipes/ dir should be created in standalone mode"
    );
    let body = fs::read_to_string(&manifest_path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&body).expect("scaffold output is JSON");
    assert_eq!(parsed["name"], "my-recipe");
    assert_eq!(parsed["version"], "0.1.0");
    assert!(parsed["targets"]["just"]["snippet"].is_string());
}

#[test]
fn scaffold_recipe_in_tap_repo_writes_manifest_and_appends_index_entry() {
    let dir = TempDir::new().unwrap();
    empty_tap(dir.path());
    jtr()
        .current_dir(dir.path())
        .args(["scaffold", "recipe", "widget"])
        .assert()
        .success()
        .stdout(str::contains("recipes/widget.json"))
        .stdout(str::contains("appended stub entry"));

    let manifest_path = dir.path().join("recipes/widget.json");
    assert!(manifest_path.exists(), "manifest should be in recipes/");

    let index_text = fs::read_to_string(dir.path().join("index.json")).unwrap();
    let index: serde_json::Value = serde_json::from_str(&index_text).expect("index is JSON");
    let recipes = index["recipes"].as_array().expect("recipes array");
    assert_eq!(recipes.len(), 1);
    assert_eq!(recipes[0]["name"], "widget");
    assert_eq!(recipes[0]["manifest_url"], "recipes/widget.json");
    assert!(
        recipes[0].get("sha256").is_none(),
        "scaffold leaves sha256 absent; `lint --fix` fills it in. got: {recipes:?}"
    );
}

#[test]
fn scaffold_recipe_refuses_to_overwrite_an_existing_manifest() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("foo.json"), "{}").unwrap();
    jtr()
        .current_dir(dir.path())
        .args(["scaffold", "recipe", "foo"])
        .assert()
        .failure()
        .stderr(str::contains("already exists"));
}

#[test]
fn scaffold_recipe_refuses_to_overwrite_an_existing_index_entry() {
    let dir = TempDir::new().unwrap();
    let index = r#"{
  "version": 1,
  "recipes": [
    {
      "name": "widget",
      "version": "0.1.0",
      "description": "x",
      "manifest_url": "recipes/widget.json",
      "targets": ["just"]
    }
  ]
}
"#;
    fs::write(dir.path().join("index.json"), index).unwrap();
    jtr()
        .current_dir(dir.path())
        .args(["scaffold", "recipe", "widget"])
        .assert()
        .failure()
        .stderr(str::contains("already has an entry"));
}

#[test]
fn lint_passes_a_valid_manifest() {
    let dir = TempDir::new().unwrap();
    let manifest = r#"{
  "name": "ok",
  "version": "0.1.0",
  "description": "a fine recipe",
  "shells_out_to": [],
  "targets": {
    "just": {
      "snippet": "ok:\n    @echo ok\n"
    }
  }
}"#;
    let path = dir.path().join("ok.json");
    fs::write(&path, manifest).unwrap();
    jtr()
        .arg("lint")
        .arg(&path)
        .assert()
        .success()
        .stdout(str::contains("passed lint"));
}

#[test]
fn lint_detects_schema_breakage() {
    let dir = TempDir::new().unwrap();
    let broken = r#"{"name": "x"}"#;
    let path = dir.path().join("broken.json");
    fs::write(&path, broken).unwrap();
    jtr().arg("lint").arg(&path).assert().failure();
}

fn just_on_path() -> bool {
    let path = match std::env::var_os("PATH") {
        Some(p) => p,
        None => return false,
    };
    std::env::split_paths(&path).any(|d| d.join("just").is_file())
}

#[test]
fn lint_detects_invalid_just_snippet_when_just_is_on_path() {
    if !just_on_path() {
        // Skip on runners without `just` installed; matches lint's documented
        // graceful-fallback behaviour. The unit test against the mismatch
        // detector covers the schema path regardless.
        eprintln!("skipping: `just` not on PATH");
        return;
    }
    let dir = TempDir::new().unwrap();
    // `: not_a_recipe :` is not a valid `just` recipe header.
    let manifest = r#"{
  "name": "bad",
  "version": "0.1.0",
  "description": "broken snippet",
  "targets": {
    "just": {
      "snippet": "this is not valid just syntax!!!\n@@@\n"
    }
  }
}"#;
    let path = dir.path().join("bad.json");
    fs::write(&path, manifest).unwrap();
    jtr()
        .arg("lint")
        .arg(&path)
        .assert()
        .failure()
        .stdout(str::contains("snippet"));
}

#[test]
fn lint_tap_validates_a_whole_tap_fixture() {
    let dir = TempDir::new().unwrap();
    // Build a minimal but coherent tap: index + recipe with correct sha.
    let manifest = "{\n  \"name\": \"alpha\",\n  \"version\": \"0.1.0\",\n  \"description\": \"alpha recipe\",\n  \"targets\": {\n    \"just\": {\n      \"snippet\": \"alpha:\\n    @echo alpha\\n\"\n    }\n  }\n}";
    let recipes_dir = dir.path().join("recipes");
    fs::create_dir(&recipes_dir).unwrap();
    fs::write(recipes_dir.join("alpha.json"), manifest).unwrap();
    let sha = sha256_hex(manifest.as_bytes());
    let index = format!(
        "{{\n  \"version\": 1,\n  \"recipes\": [\n    {{\n      \"name\": \"alpha\",\n      \"version\": \"0.1.0\",\n      \"description\": \"alpha recipe\",\n      \"manifest_url\": \"recipes/alpha.json\",\n      \"targets\": [\"just\"],\n      \"sha256\": \"{sha}\"\n    }}\n  ]\n}}\n"
    );
    fs::write(dir.path().join("index.json"), &index).unwrap();

    jtr()
        .args(["lint", "--tap"])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(str::contains("passed lint"));
}

#[test]
fn lint_tap_detects_sha_mismatch_and_fix_repairs_it() {
    let dir = TempDir::new().unwrap();
    let manifest = "{\n  \"name\": \"alpha\",\n  \"version\": \"0.1.0\",\n  \"description\": \"alpha recipe\",\n  \"targets\": {\n    \"just\": {\n      \"snippet\": \"alpha:\\n    @echo alpha\\n\"\n    }\n  }\n}";
    let recipes_dir = dir.path().join("recipes");
    fs::create_dir(&recipes_dir).unwrap();
    fs::write(recipes_dir.join("alpha.json"), manifest).unwrap();
    let real_sha = sha256_hex(manifest.as_bytes());
    let bogus_sha = "0".repeat(64);
    let index = format!(
        "{{\n  \"version\": 1,\n  \"recipes\": [\n    {{\n      \"name\": \"alpha\",\n      \"version\": \"0.1.0\",\n      \"description\": \"alpha recipe\",\n      \"manifest_url\": \"recipes/alpha.json\",\n      \"targets\": [\"just\"],\n      \"sha256\": \"{bogus_sha}\"\n    }}\n  ]\n}}\n"
    );
    fs::write(dir.path().join("index.json"), &index).unwrap();

    jtr()
        .args(["lint", "--tap"])
        .arg(dir.path())
        .assert()
        .failure()
        .stdout(str::contains("sha256 mismatch"));

    jtr()
        .args(["lint", "--tap"])
        .arg(dir.path())
        .arg("--fix")
        .assert()
        .success()
        .stdout(str::contains("updated sha256"));

    let after = fs::read_to_string(dir.path().join("index.json")).unwrap();
    assert!(
        after.contains(&format!("\"sha256\": \"{real_sha}\"")),
        "index should contain the correct sha after --fix:\n{after}"
    );
    assert!(
        !after.contains(&bogus_sha),
        "bogus sha must be gone after --fix"
    );
}

#[test]
fn lint_tap_fix_adds_missing_sha_field() {
    let dir = TempDir::new().unwrap();
    let manifest = "{\n  \"name\": \"alpha\",\n  \"version\": \"0.1.0\",\n  \"description\": \"alpha recipe\",\n  \"targets\": {\n    \"just\": {\n      \"snippet\": \"alpha:\\n    @echo alpha\\n\"\n    }\n  }\n}";
    let recipes_dir = dir.path().join("recipes");
    fs::create_dir(&recipes_dir).unwrap();
    fs::write(recipes_dir.join("alpha.json"), manifest).unwrap();
    // Index without any sha256 field for the entry.
    let index = "{\n  \"version\": 1,\n  \"recipes\": [\n    {\n      \"name\": \"alpha\",\n      \"version\": \"0.1.0\",\n      \"description\": \"alpha recipe\",\n      \"manifest_url\": \"recipes/alpha.json\",\n      \"targets\": [\"just\"]\n    }\n  ]\n}\n";
    fs::write(dir.path().join("index.json"), index).unwrap();

    jtr()
        .args(["lint", "--tap"])
        .arg(dir.path())
        .arg("--fix")
        .assert()
        .success()
        .stdout(str::contains("added sha256"));

    let after = fs::read_to_string(dir.path().join("index.json")).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&after).expect("still valid JSON");
    let added_sha = parsed["recipes"][0]["sha256"]
        .as_str()
        .expect("sha256 should have been inserted");
    assert_eq!(added_sha, sha256_hex(manifest.as_bytes()));
}

#[test]
fn lint_fix_without_tap_errors_with_helpful_message() {
    let dir = TempDir::new().unwrap();
    let manifest = r#"{"name": "x", "version": "0.1.0", "description": "y", "targets": {"just": {"snippet": "x:\n    @echo x\n"}}}"#;
    let path = dir.path().join("x.json");
    fs::write(&path, manifest).unwrap();
    jtr()
        .arg("lint")
        .arg(&path)
        .arg("--fix")
        .assert()
        .failure()
        .stderr(str::contains("--fix requires --tap"));
}

#[test]
fn lint_tap_detects_field_drift_between_manifest_and_index() {
    let dir = TempDir::new().unwrap();
    let manifest = "{\n  \"name\": \"alpha\",\n  \"version\": \"0.2.0\",\n  \"description\": \"alpha recipe\",\n  \"targets\": {\n    \"just\": {\n      \"snippet\": \"alpha:\\n    @echo alpha\\n\"\n    }\n  }\n}";
    let recipes_dir = dir.path().join("recipes");
    fs::create_dir(&recipes_dir).unwrap();
    fs::write(recipes_dir.join("alpha.json"), manifest).unwrap();
    let sha = sha256_hex(manifest.as_bytes());
    // Index claims version 0.1.0 while manifest says 0.2.0 — drift.
    let index = format!(
        "{{\n  \"version\": 1,\n  \"recipes\": [\n    {{\n      \"name\": \"alpha\",\n      \"version\": \"0.1.0\",\n      \"description\": \"alpha recipe\",\n      \"manifest_url\": \"recipes/alpha.json\",\n      \"targets\": [\"just\"],\n      \"sha256\": \"{sha}\"\n    }}\n  ]\n}}\n"
    );
    fs::write(dir.path().join("index.json"), &index).unwrap();
    jtr()
        .args(["lint", "--tap"])
        .arg(dir.path())
        .assert()
        .failure()
        .stdout(str::contains("does not match"));
}

#[test]
fn lint_warns_when_shells_out_to_lists_missing_tool() {
    let dir = TempDir::new().unwrap();
    let manifest = r#"{
  "name": "needs-ghost",
  "version": "0.1.0",
  "description": "uses a nonexistent tool",
  "shells_out_to": ["definitely-not-a-real-tool-xyz123"],
  "targets": {
    "just": {
      "snippet": "needs-ghost:\n    @echo hi\n"
    }
  }
}"#;
    let path = dir.path().join("needs-ghost.json");
    fs::write(&path, manifest).unwrap();
    // Exit 0 because PATH-missing is a warning, not an error — but the
    // warning must surface on stdout so authors notice before publishing.
    jtr()
        .arg("lint")
        .arg(&path)
        .assert()
        .success()
        .stdout(str::contains("definitely-not-a-real-tool-xyz123"))
        .stdout(str::contains("PATH"));
}

#[test]
fn scaffold_then_lint_fix_round_trip_succeeds() {
    let dir = TempDir::new().unwrap();
    empty_tap(dir.path());
    jtr()
        .current_dir(dir.path())
        .args(["scaffold", "recipe", "demo"])
        .assert()
        .success();

    // Replace the placeholder description so lint doesn't warn — exercises the
    // "user hand-edits the manifest before publishing" pattern.
    let manifest_path = dir.path().join("recipes/demo.json");
    let original = fs::read_to_string(&manifest_path).unwrap();
    let edited = original.replace("TODO: one-line description", "Demo recipe for testing");
    fs::write(&manifest_path, edited).unwrap();

    jtr()
        .args(["lint", "--tap"])
        .arg(dir.path())
        .arg("--fix")
        .assert()
        .success();

    // After --fix, plain `lint --tap` (no --fix) should be clean.
    jtr()
        .args(["lint", "--tap"])
        .arg(dir.path())
        .assert()
        .success();
}

#[test]
fn info_prints_metadata_for_a_curated_recipe() {
    let config_dir = TempDir::new().unwrap();

    let assert = jtr()
        .env("JTR_CONFIG_DIR", config_dir.path())
        .env("JTR_INDEX_URL", sample_index_url())
        .args(["info", "postgres-dev"])
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();

    // Stable fields only — the bundled sha changes whenever the manifest is
    // edited (`lint --fix` recomputes it), so the exact checksum is asserted in
    // the temp-index JSON test where the test computes it itself.
    assert!(stdout.contains("postgres-dev"), "got:\n{stdout}");
    assert!(
        stdout.contains("Local PostgreSQL development environment"),
        "info should print the description, got:\n{stdout}"
    );
    assert!(
        stdout.contains("curated"),
        "info should label the source, got:\n{stdout}"
    );
    assert!(
        stdout.contains("docker"),
        "info should list the shelled-out binary, got:\n{stdout}"
    );
    assert!(
        stdout.contains("0.1.0"),
        "info should print the version, got:\n{stdout}"
    );
}

#[test]
fn info_works_with_no_project_file_present() {
    // The defining difference from show/diff: `info` describes the recipe, not
    // how it lands in a project file, so it must succeed in a directory with no
    // justfile or Taskfile. Guards against a future refactor re-adding
    // target::resolve to the info path.
    let empty_dir = TempDir::new().unwrap();
    let config_dir = TempDir::new().unwrap();

    jtr()
        .current_dir(empty_dir.path())
        .env("JTR_CONFIG_DIR", config_dir.path())
        .env("JTR_INDEX_URL", sample_index_url())
        .args(["info", "postgres-dev"])
        .assert()
        .success()
        .stdout(str::contains("postgres-dev"));
}

#[test]
fn info_describes_a_tap_recipe() {
    let config_dir = TempDir::new().unwrap();
    let tap_dir = TempDir::new().unwrap();
    let tap_index = write_tap_index(tap_dir.path(), "fancy-build", "0.1.0", "tap-marker");

    jtr()
        .env("JTR_CONFIG_DIR", config_dir.path())
        .args(["tap", "add", "alice/recipes", "--url", &tap_index])
        .assert()
        .success();

    let assert = jtr()
        .env("JTR_CONFIG_DIR", config_dir.path())
        .env("JTR_INDEX_URL", sample_index_url())
        .args(["info", "alice/recipes/fancy-build"])
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();

    assert!(stdout.contains("fancy-build"), "got:\n{stdout}");
    assert!(
        stdout.contains("alice/recipes"),
        "info should label the tap source, got:\n{stdout}"
    );
}

#[test]
fn info_with_pin_shows_the_requested_version() {
    let config_dir = TempDir::new().unwrap();
    let index_dir = TempDir::new().unwrap();
    let index_url = write_multi_version_index(index_dir.path(), "demo", &["0.2.0", "0.1.0"]);

    let assert = jtr()
        .env("JTR_CONFIG_DIR", config_dir.path())
        .env("JTR_INDEX_URL", &index_url)
        .args(["info", "demo@0.1.0"])
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();

    assert!(
        stdout.contains("demo @0.1.0"),
        "info @0.1.0 should head with the pinned version, got:\n{stdout}"
    );
    // The full version history is still listed even when a single version is pinned.
    assert!(
        stdout.contains("0.2.0"),
        "info should still list every published version, got:\n{stdout}"
    );
}

#[test]
fn info_json_is_machine_readable() {
    let config_dir = TempDir::new().unwrap();
    let registry_dir = TempDir::new().unwrap();
    let index_url = write_postgres_dev_index(registry_dir.path(), "0.1.0");

    let assert = jtr()
        .env("JTR_CONFIG_DIR", config_dir.path())
        .env("JTR_INDEX_URL", &index_url)
        .args(["info", "postgres-dev", "--json"])
        .assert()
        .success();
    let stdout = assert.get_output().stdout.clone();

    // Parse the actual contract, not a substring — this is the machine-readable surface.
    let parsed: serde_json::Value =
        serde_json::from_slice(&stdout).expect("info --json must emit valid JSON on stdout");

    assert_eq!(parsed["name"], "postgres-dev");
    assert_eq!(parsed["version"], "0.1.0");
    assert_eq!(parsed["source"], "curated");
    assert_eq!(parsed["shells_out_to"][0], "docker");
    assert_eq!(parsed["targets"][0], "just");

    // The checksum must be the full 64-char sha — provenance vetting is the
    // whole point, and a truncated hash can't verify what's about to install.
    let manifest_bytes = fs::read(registry_dir.path().join("recipes/postgres-dev.json")).unwrap();
    let expected_sha = sha256_hex(&manifest_bytes);
    assert_eq!(parsed["checksum"], expected_sha);
    assert_eq!(expected_sha.len(), 64);
}

#[test]
fn info_lists_declared_dependencies() {
    // The other fixtures all have empty dependencies; this exercises the
    // populated `depends on` line and the non-empty JSON `dependencies` array —
    // the field "what it depends on" is half the point of `info` for vetting.
    let config_dir = TempDir::new().unwrap();
    let registry_dir = TempDir::new().unwrap();
    let index_url = write_dep_index(
        registry_dir.path(),
        &[("a", "0.1.0", &["b"]), ("b", "0.1.0", &[])],
    );

    let assert = jtr()
        .env("JTR_CONFIG_DIR", config_dir.path())
        .env("JTR_INDEX_URL", &index_url)
        .args(["info", "a"])
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(
        stdout.contains("depends on") && stdout.contains('b'),
        "info should print declared dependencies, got:\n{stdout}"
    );

    let json_out = jtr()
        .env("JTR_CONFIG_DIR", config_dir.path())
        .env("JTR_INDEX_URL", &index_url)
        .args(["info", "a", "--json"])
        .assert()
        .success();
    let parsed: serde_json::Value =
        serde_json::from_slice(&json_out.get_output().stdout).expect("valid JSON");
    assert_eq!(parsed["dependencies"][0], "b");
}

#[test]
fn info_errors_when_recipe_does_not_exist() {
    let config_dir = TempDir::new().unwrap();

    jtr()
        .env("JTR_CONFIG_DIR", config_dir.path())
        .env("JTR_INDEX_URL", sample_index_url())
        .args(["info", "does-not-exist"])
        .assert()
        .failure()
        .stderr(str::contains("not found"));
}

#[test]
fn info_errors_when_tap_is_not_configured() {
    let config_dir = TempDir::new().unwrap();

    jtr()
        .env("JTR_CONFIG_DIR", config_dir.path())
        .env("JTR_INDEX_URL", sample_index_url())
        .args(["info", "ghost/repo/anything"])
        .assert()
        .failure()
        .stderr(str::contains("tap 'ghost/repo' is not configured"));
}

#[test]
fn update_dry_run_with_nothing_to_change_is_empty_and_exits_zero() {
    // Mirrors `jtr diff` of an unmodified install: when every block is already
    // current, --dry-run writes nothing, prints nothing, and exits 0.
    let project = project_with_justfile("default:\n    @echo hi\n");
    let config_dir = TempDir::new().unwrap();

    jtr()
        .current_dir(project.path())
        .env("JTR_CONFIG_DIR", config_dir.path())
        .env("JTR_INDEX_URL", sample_index_url())
        .args(["install", "postgres-dev"])
        .assert()
        .success();
    let before = fs::read_to_string(project.path().join("justfile")).unwrap();

    let assert = jtr()
        .current_dir(project.path())
        .env("JTR_CONFIG_DIR", config_dir.path())
        .env("JTR_INDEX_URL", sample_index_url())
        .args(["update", "--dry-run"])
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(
        stdout.is_empty(),
        "no-op dry-run should produce empty stdout, got: {stdout:?}"
    );

    let after = fs::read_to_string(project.path().join("justfile")).unwrap();
    assert_eq!(before, after, "dry-run must not modify the project file");
}

#[test]
fn update_dry_run_previews_an_update_without_writing() {
    // A newer version is available; --dry-run prints `would update` plus a
    // unified diff (same engine as `jtr diff`), exits 1, and leaves the file
    // untouched.
    let project = project_with_justfile("default:\n    @echo hi\n");
    let registry_dir = TempDir::new().unwrap();
    let config_dir = TempDir::new().unwrap();

    let v1 = write_postgres_dev_index(registry_dir.path(), "0.1.0");
    jtr()
        .current_dir(project.path())
        .env("JTR_CONFIG_DIR", config_dir.path())
        .env("JTR_INDEX_URL", &v1)
        .args(["install", "postgres-dev"])
        .assert()
        .success();
    let before = fs::read_to_string(project.path().join("justfile")).unwrap();

    let v2 = write_postgres_dev_index(registry_dir.path(), "0.2.0");
    let assert = jtr()
        .current_dir(project.path())
        .env("JTR_CONFIG_DIR", config_dir.path())
        .env("JTR_INDEX_URL", &v2)
        .args(["update", "postgres-dev", "--dry-run"])
        .assert()
        .failure()
        .code(1);
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(
        stdout.contains("would update"),
        "dry-run should announce the would-update action, got:\n{stdout}"
    );
    assert!(
        stdout.contains("0.1.0") && stdout.contains("0.2.0"),
        "dry-run should show the version transition, got:\n{stdout}"
    );
    assert!(
        stdout.contains("--- a/postgres-dev") && stdout.contains("+++ b/postgres-dev"),
        "dry-run should print a unified-diff header, got:\n{stdout}"
    );
    assert!(
        stdout.contains("-    @echo marker-0.1.0") && stdout.contains("+    @echo marker-0.2.0"),
        "dry-run diff should show the snippet change, got:\n{stdout}"
    );

    let after = fs::read_to_string(project.path().join("justfile")).unwrap();
    assert_eq!(before, after, "dry-run must not modify the project file");
}

#[test]
fn update_dry_run_lists_a_missing_transitive_dependency() {
    // a@v2 adds a new dep on b. `update a --dry-run` should preview installing b
    // (as a dependency) and updating a, exit 1, and write nothing — so b never
    // lands on disk.
    let registry_dir = TempDir::new().unwrap();
    let config_dir = TempDir::new().unwrap();
    let index = write_dep_index(registry_dir.path(), &[("a", "0.1.0", &[])]);
    let project = project_with_justfile("default:\n    @echo hi\n");

    jtr()
        .current_dir(project.path())
        .env("JTR_CONFIG_DIR", config_dir.path())
        .env("JTR_INDEX_URL", &index)
        .args(["install", "a"])
        .assert()
        .success();
    let before = fs::read_to_string(project.path().join("justfile")).unwrap();

    write_dep_index(
        registry_dir.path(),
        &[("a", "0.2.0", &["b"]), ("b", "0.1.0", &[])],
    );

    let assert = jtr()
        .current_dir(project.path())
        .env("JTR_CONFIG_DIR", config_dir.path())
        .env("JTR_INDEX_URL", &index)
        .args(["update", "a", "--dry-run"])
        .assert()
        .failure()
        .code(1);
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(
        stdout.contains("would install") && stdout.contains("(dependency)"),
        "dry-run should preview installing the new dep b, got:\n{stdout}"
    );
    assert!(
        stdout.contains("would update"),
        "dry-run should preview updating a, got:\n{stdout}"
    );

    let after = fs::read_to_string(project.path().join("justfile")).unwrap();
    assert_eq!(before, after, "dry-run must not modify the project file");
    assert!(
        !after.contains("# >>> jtr:b@"),
        "dry-run must not actually install b, got:\n{after}"
    );
}

#[test]
fn update_dry_run_with_unpin_previews_the_unpin_without_writing() {
    // `--unpin --dry-run` should preview bumping a pinned block to latest and
    // dropping the pin, exit 1, but leave the pin and the old snippet on disk.
    let index_dir = TempDir::new().unwrap();
    let config_dir = TempDir::new().unwrap();
    let index_url = write_multi_version_index(index_dir.path(), "demo", &["0.2.0", "0.1.0"]);
    let project = project_with_justfile("default:\n    @echo hi\n");

    jtr()
        .current_dir(project.path())
        .env("JTR_CONFIG_DIR", config_dir.path())
        .env("JTR_INDEX_URL", &index_url)
        .args(["install", "demo@0.1.0"])
        .assert()
        .success();
    let before = fs::read_to_string(project.path().join("justfile")).unwrap();

    let assert = jtr()
        .current_dir(project.path())
        .env("JTR_CONFIG_DIR", config_dir.path())
        .env("JTR_INDEX_URL", &index_url)
        .args(["update", "demo", "--unpin", "--dry-run"])
        .assert()
        .failure()
        .code(1);
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(
        stdout.contains("would unpin"),
        "dry-run --unpin should announce the would-unpin action, got:\n{stdout}"
    );
    assert!(
        stdout.contains("0.1.0") && stdout.contains("0.2.0"),
        "dry-run --unpin should show the version transition, got:\n{stdout}"
    );

    let after = fs::read_to_string(project.path().join("justfile")).unwrap();
    assert_eq!(before, after, "dry-run must not modify the project file");
    assert!(
        after.contains("# pinned: 0.1.0"),
        "pin must survive a dry-run --unpin, got:\n{after}"
    );
    assert!(
        after.contains("marker-0.1.0") && !after.contains("marker-0.2.0"),
        "dry-run --unpin must not bump the snippet, got:\n{after}"
    );
}

#[test]
fn update_dry_run_shows_pinned_skip_and_exits_zero() {
    // Without --unpin a pinned block is a non-action, but the "pinned — skipping"
    // line is still an enumerated plan outcome, so --dry-run prints it. Since
    // nothing would change, the run exits 0. Guards against silently dropping the
    // skip line from dry-run output.
    let index_dir = TempDir::new().unwrap();
    let config_dir = TempDir::new().unwrap();
    let index_url = write_multi_version_index(index_dir.path(), "demo", &["0.2.0", "0.1.0"]);
    let project = project_with_justfile("default:\n    @echo hi\n");

    jtr()
        .current_dir(project.path())
        .env("JTR_CONFIG_DIR", config_dir.path())
        .env("JTR_INDEX_URL", &index_url)
        .args(["install", "demo@0.1.0"])
        .assert()
        .success();

    jtr()
        .current_dir(project.path())
        .env("JTR_CONFIG_DIR", config_dir.path())
        .env("JTR_INDEX_URL", &index_url)
        .args(["update", "demo", "--dry-run"])
        .assert()
        .success()
        .stdout(str::contains("pinned to 0.1.0"))
        .stdout(str::contains("skipping"));
}

#[test]
fn update_dry_run_with_two_current_blocks_is_empty_and_exits_zero() {
    // The single-block no-op case can't catch the upsert-reorder trap: only a
    // *non-last* block triggers it. With two already-current blocks, dry-run must
    // still be a silent exit-0 no-op — comparing block content, not the whole doc.
    let project = project_with_justfile("default:\n    @echo hi\n");
    let config_dir = TempDir::new().unwrap();

    for recipe in ["postgres-dev", "rust-lint-format"] {
        jtr()
            .current_dir(project.path())
            .env("JTR_CONFIG_DIR", config_dir.path())
            .env("JTR_INDEX_URL", sample_index_url())
            .args(["install", recipe])
            .assert()
            .success();
    }
    let before = fs::read_to_string(project.path().join("justfile")).unwrap();

    let assert = jtr()
        .current_dir(project.path())
        .env("JTR_CONFIG_DIR", config_dir.path())
        .env("JTR_INDEX_URL", sample_index_url())
        .args(["update", "--dry-run"])
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(
        stdout.is_empty(),
        "two-block no-op dry-run should produce empty stdout, got:\n{stdout}"
    );

    let after = fs::read_to_string(project.path().join("justfile")).unwrap();
    assert_eq!(before, after, "dry-run must not modify the project file");
}

#[test]
fn update_no_arg_on_current_blocks_reports_already_current_not_refreshed() {
    // Regression: `upsert` appends the block to the end, so a no-arg update over
    // two current blocks used to move each one and report a spurious "refreshed
    // (reverted manual edits)". With block-content no-op detection it must report
    // "already at version" for both and leave the file byte-identical.
    let project = project_with_justfile("default:\n    @echo hi\n");
    let config_dir = TempDir::new().unwrap();

    for recipe in ["postgres-dev", "rust-lint-format"] {
        jtr()
            .current_dir(project.path())
            .env("JTR_CONFIG_DIR", config_dir.path())
            .env("JTR_INDEX_URL", sample_index_url())
            .args(["install", recipe])
            .assert()
            .success();
    }
    let before = fs::read_to_string(project.path().join("justfile")).unwrap();

    let assert = jtr()
        .current_dir(project.path())
        .env("JTR_CONFIG_DIR", config_dir.path())
        .env("JTR_INDEX_URL", sample_index_url())
        .arg("update")
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(
        stdout.contains("already at version"),
        "no-op update should report already-current, got:\n{stdout}"
    );
    assert!(
        !stdout.contains("refreshed"),
        "no-op update must not spuriously refresh unchanged blocks, got:\n{stdout}"
    );

    let after = fs::read_to_string(project.path().join("justfile")).unwrap();
    assert_eq!(
        before, after,
        "a genuine no-op update must leave the file byte-identical"
    );
}

// --- Task (YAML) write target ---------------------------------------------
//
// These mirror the justfile suite against Taskfile.yml fixtures: install nests
// the block under `tasks:`, remove/update/doctor/dependencies behave identically,
// and a user's other tasks (including literal block scalars) survive byte-for-byte.
// They never shell out to `task`, so they pass on runners without it installed.

fn project_with_taskfile(initial: &str) -> TempDir {
    let dir = TempDir::new().expect("create temp dir");
    fs::write(dir.path().join("Taskfile.yml"), initial).expect("write Taskfile.yml");
    dir
}

/// A starter Taskfile with one user-authored task, used as the install target.
const TASKFILE_WITH_DEFAULT: &str =
    "version: '3'\n\ntasks:\n  default:\n    desc: List tasks\n    cmds:\n      - task --list\n";

/// Single-recipe index publishing `<recipe>` with a `task` target. The snippet
/// carries `marker-<version>` in its `desc:` so a test can detect which version
/// landed and assert that update swapped it.
fn write_task_index(dir: &Path, recipe: &str, version: &str) -> String {
    let recipes_dir = dir.join("recipes");
    fs::create_dir_all(&recipes_dir).unwrap();

    let manifest = format!(
        r#"{{
  "name": "{recipe}",
  "version": "{version}",
  "description": "task fixture {recipe}",
  "shells_out_to": [],
  "targets": {{
    "task": {{
      "snippet": "{recipe}-up:\n  desc: marker-{version}\n  cmds:\n    - echo {recipe}\n"
    }}
  }}
}}"#
    );
    fs::write(recipes_dir.join(format!("{recipe}.json")), &manifest).unwrap();
    let sha = sha256_hex(manifest.as_bytes());

    let index = format!(
        r#"{{
  "version": 1,
  "recipes": [
    {{
      "name": "{recipe}",
      "version": "{version}",
      "description": "task fixture {recipe}",
      "manifest_url": "recipes/{recipe}.json",
      "targets": ["task"],
      "sha256": "{sha}"
    }}
  ]
}}"#
    );
    fs::write(dir.join("index.json"), index).unwrap();
    format!("file://{}/index.json", dir.display())
}

/// Multi-recipe index with `task` targets and a dependency graph, mirroring
/// `write_dep_index` for the Task path.
fn write_task_dep_index(dir: &Path, recipes: &[(&str, &str, &[&str])]) -> String {
    let recipes_dir = dir.join("recipes");
    fs::create_dir_all(&recipes_dir).unwrap();

    let mut index_entries = Vec::new();
    for (name, version, deps) in recipes {
        let deps_json = deps
            .iter()
            .map(|d| format!("\"{}\"", d))
            .collect::<Vec<_>>()
            .join(", ");
        let manifest = format!(
            r#"{{
  "name": "{name}",
  "version": "{version}",
  "description": "task dep fixture {name}",
  "dependencies": [{deps_json}],
  "targets": {{
    "task": {{
      "snippet": "{name}-noop:\n  cmds:\n    - echo {name}\n"
    }}
  }}
}}"#
        );
        fs::write(recipes_dir.join(format!("{name}.json")), &manifest).unwrap();
        let sha = sha256_hex(manifest.as_bytes());
        index_entries.push(format!(
            r#"    {{
      "name": "{name}",
      "version": "{version}",
      "description": "task dep fixture {name}",
      "manifest_url": "recipes/{name}.json",
      "targets": ["task"],
      "sha256": "{sha}"
    }}"#
        ));
    }

    let index = format!(
        r#"{{
  "version": 1,
  "recipes": [
{}
  ]
}}"#,
        index_entries.join(",\n")
    );
    fs::write(dir.join("index.json"), index).unwrap();
    format!("file://{}/index.json", dir.display())
}

#[test]
fn task_install_nests_block_under_tasks_map() {
    let config_dir = TempDir::new().unwrap();
    let index_dir = TempDir::new().unwrap();
    let url = write_task_index(index_dir.path(), "redis", "0.1.0");
    let project = project_with_taskfile(TASKFILE_WITH_DEFAULT);

    jtr()
        .current_dir(project.path())
        .env("JTR_CONFIG_DIR", config_dir.path())
        .env("JTR_INDEX_URL", &url)
        .args(["install", "redis"])
        .assert()
        .success()
        .stdout(str::contains("installed"));

    let result = fs::read_to_string(project.path().join("Taskfile.yml")).unwrap();
    // The user's task is preserved verbatim, and the managed block is nested
    // under tasks: at two-space indent (markers, body, and close all indented).
    assert!(result.contains("tasks:\n  default:\n    desc: List tasks"));
    assert!(result.contains("  # >>> jtr:redis@0.1.0 >>>"));
    assert!(result.contains("  redis-up:\n    desc: marker-0.1.0"));
    assert!(result.contains("  # <<< jtr:redis <<<"));
    // No top-level (column-0) leakage of the recipe's task key.
    assert!(!result.contains("\nredis-up:"));
}

#[test]
fn curated_postgres_dev_installs_into_taskfile() {
    // jtr-index #23: every curated seed recipe now declares a `task` target, so
    // installing one that used to be just-only (postgres-dev) into a Taskfile.yml
    // succeeds and nests the block under `tasks:` instead of erroring. This runs
    // against the real bundled index, so it also guards the index `targets` entry
    // and the manifest's task snippet staying in sync.
    let config_dir = TempDir::new().unwrap();
    let project = project_with_taskfile(TASKFILE_WITH_DEFAULT);

    jtr()
        .current_dir(project.path())
        .env("JTR_CONFIG_DIR", config_dir.path())
        .env("JTR_INDEX_URL", sample_index_url())
        .args(["install", "postgres-dev"])
        .assert()
        .success()
        .stdout(str::contains("installed"));

    let result = fs::read_to_string(project.path().join("Taskfile.yml")).unwrap();
    // The user's task survives and the managed block nests under tasks: at two-space
    // indent (markers, body, and close all indented).
    assert!(result.contains("tasks:\n  default:\n    desc: List tasks"));
    assert!(result.contains("  # >>> jtr:postgres-dev@0.1.0 >>>"));
    assert!(result.contains("  postgres-up:\n    desc:"));
    assert!(result.contains("  # <<< jtr:postgres-dev <<<"));
    // No top-level (column-0) leakage of the recipe's task keys.
    assert!(!result.contains("\npostgres-up:"));
    assert!(!result.contains("\npostgres-down:"));
}

#[test]
fn task_install_then_remove_restores_surrounding_content() {
    let config_dir = TempDir::new().unwrap();
    let index_dir = TempDir::new().unwrap();
    let url = write_task_index(index_dir.path(), "redis", "0.1.0");
    let project = project_with_taskfile(TASKFILE_WITH_DEFAULT);

    for action in ["install", "remove"] {
        jtr()
            .current_dir(project.path())
            .env("JTR_CONFIG_DIR", config_dir.path())
            .env("JTR_INDEX_URL", &url)
            .args([action, "redis"])
            .assert()
            .success();
    }

    let result = fs::read_to_string(project.path().join("Taskfile.yml")).unwrap();
    assert!(!result.contains("jtr:redis@"));
    assert!(!result.contains("redis-up:"));
    // The user's original task survives untouched.
    assert!(result.contains("tasks:\n  default:\n    desc: List tasks"));
    assert!(
        !result.contains("\n\n\n"),
        "remove must not leave a blank gap"
    );
}

#[test]
fn task_update_swaps_block_to_new_version() {
    let config_dir = TempDir::new().unwrap();
    let index_dir = TempDir::new().unwrap();
    write_task_index(index_dir.path(), "redis", "0.1.0");
    let url = format!("file://{}/index.json", index_dir.path().display());
    let project = project_with_taskfile(TASKFILE_WITH_DEFAULT);

    jtr()
        .current_dir(project.path())
        .env("JTR_CONFIG_DIR", config_dir.path())
        .env("JTR_INDEX_URL", &url)
        .args(["install", "redis"])
        .assert()
        .success();

    // Republish 0.2.0 in place (file:// bypasses the cache by design).
    write_task_index(index_dir.path(), "redis", "0.2.0");

    jtr()
        .current_dir(project.path())
        .env("JTR_CONFIG_DIR", config_dir.path())
        .env("JTR_INDEX_URL", &url)
        .args(["update", "redis"])
        .assert()
        .success()
        .stdout(str::contains("updated"));

    let result = fs::read_to_string(project.path().join("Taskfile.yml")).unwrap();
    assert!(result.contains("# >>> jtr:redis@0.2.0 >>>"));
    assert!(result.contains("desc: marker-0.2.0"));
    assert!(!result.contains("marker-0.1.0"));
    // Exactly one block after the bump.
    assert_eq!(result.matches("# >>> jtr:redis@").count(), 1);
}

#[test]
fn task_update_dry_run_is_silent_noop_when_current() {
    let config_dir = TempDir::new().unwrap();
    let index_dir = TempDir::new().unwrap();
    let url = write_task_index(index_dir.path(), "redis", "0.1.0");
    let project = project_with_taskfile(TASKFILE_WITH_DEFAULT);

    jtr()
        .current_dir(project.path())
        .env("JTR_CONFIG_DIR", config_dir.path())
        .env("JTR_INDEX_URL", &url)
        .args(["install", "redis"])
        .assert()
        .success();

    let before = fs::read_to_string(project.path().join("Taskfile.yml")).unwrap();

    // A freshly-installed Task block must read back byte-identical to what would
    // be re-rendered, so --dry-run prints nothing and exits 0. This is the
    // end-to-end guard for the indent-determinism idempotency invariant.
    let assert = jtr()
        .current_dir(project.path())
        .env("JTR_CONFIG_DIR", config_dir.path())
        .env("JTR_INDEX_URL", &url)
        .args(["update", "--dry-run"])
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(
        stdout.trim().is_empty(),
        "current Task block should yield an empty --dry-run, got:\n{stdout}"
    );

    let after = fs::read_to_string(project.path().join("Taskfile.yml")).unwrap();
    assert_eq!(before, after, "--dry-run must not touch the file");
}

#[test]
fn task_doctor_detects_version_drift() {
    let config_dir = TempDir::new().unwrap();
    let index_dir = TempDir::new().unwrap();
    write_task_index(index_dir.path(), "redis", "0.1.0");
    let url = format!("file://{}/index.json", index_dir.path().display());
    let project = project_with_taskfile(TASKFILE_WITH_DEFAULT);

    jtr()
        .current_dir(project.path())
        .env("JTR_CONFIG_DIR", config_dir.path())
        .env("JTR_INDEX_URL", &url)
        .args(["install", "redis"])
        .assert()
        .success();

    write_task_index(index_dir.path(), "redis", "0.2.0");

    jtr()
        .current_dir(project.path())
        .env("JTR_CONFIG_DIR", config_dir.path())
        .env("JTR_INDEX_URL", &url)
        .arg("doctor")
        .assert()
        .failure()
        .stdout(str::contains("newer version available: 0.2.0"));
}

#[test]
fn task_install_preserves_sibling_block_scalar_blanks() {
    // Mixed-content guard: a user task whose command is a literal block scalar
    // with internal blank lines. Installing an unrelated managed block must not
    // collapse those blanks (the justfile path's global blank-collapse would).
    let config_dir = TempDir::new().unwrap();
    let index_dir = TempDir::new().unwrap();
    let url = write_task_index(index_dir.path(), "redis", "0.1.0");
    let initial = "version: '3'\n\ntasks:\n  greet:\n    cmds:\n      - |\n        echo a\n\n\n        echo b\n";
    let project = project_with_taskfile(initial);

    jtr()
        .current_dir(project.path())
        .env("JTR_CONFIG_DIR", config_dir.path())
        .env("JTR_INDEX_URL", &url)
        .args(["install", "redis"])
        .assert()
        .success();

    let result = fs::read_to_string(project.path().join("Taskfile.yml")).unwrap();
    assert!(
        result.contains("        echo a\n\n\n        echo b"),
        "block scalar blank lines must survive install:\n{result}"
    );
    assert!(result.contains("# >>> jtr:redis@0.1.0 >>>"));
}

#[test]
fn task_install_creates_tasks_map_when_absent() {
    let config_dir = TempDir::new().unwrap();
    let index_dir = TempDir::new().unwrap();
    let url = write_task_index(index_dir.path(), "redis", "0.1.0");
    // A Taskfile with no tasks: map at all (only version + includes).
    let initial = "version: '3'\n\nincludes:\n  docker: ./docker.yml\n";
    let project = project_with_taskfile(initial);

    jtr()
        .current_dir(project.path())
        .env("JTR_CONFIG_DIR", config_dir.path())
        .env("JTR_INDEX_URL", &url)
        .args(["install", "redis"])
        .assert()
        .success();

    let result = fs::read_to_string(project.path().join("Taskfile.yml")).unwrap();
    assert!(result.contains("includes:\n  docker: ./docker.yml"));
    assert!(result.contains("tasks:\n  # >>> jtr:redis@0.1.0 >>>"));
    // The block round-trips through list.
    jtr()
        .current_dir(project.path())
        .env("JTR_CONFIG_DIR", config_dir.path())
        .env("JTR_INDEX_URL", &url)
        .arg("list")
        .assert()
        .success()
        .stdout(str::contains("redis"));
}

#[test]
fn task_dependency_roundtrip_installs_both_blocks_nested() {
    let config_dir = TempDir::new().unwrap();
    let index_dir = TempDir::new().unwrap();
    let url = write_task_dep_index(
        index_dir.path(),
        &[("leaf", "0.1.0", &[]), ("root", "0.1.0", &["leaf"])],
    );
    let project = project_with_taskfile(TASKFILE_WITH_DEFAULT);

    jtr()
        .current_dir(project.path())
        .env("JTR_CONFIG_DIR", config_dir.path())
        .env("JTR_INDEX_URL", &url)
        .args(["install", "root"])
        .assert()
        .success();

    let result = fs::read_to_string(project.path().join("Taskfile.yml")).unwrap();
    let leaf_at = result.find("# >>> jtr:leaf@").expect("leaf block present");
    let root_at = result.find("# >>> jtr:root@").expect("root block present");
    assert!(
        leaf_at < root_at,
        "dependency must be ordered before its dependent"
    );
    // Both nested under tasks:, before the markers — no column-0 task keys.
    assert!(result.contains("  # >>> jtr:leaf@0.1.0 >>>"));
    assert!(result.contains("  leaf-noop:"));
    assert!(result.contains("  root-noop:"));
}

#[test]
fn task_install_is_idempotent_for_same_version() {
    // Mirrors `install_is_idempotent_for_same_version` for the Task path. install
    // uses a whole-document no-op check (`new_doc == current_doc`), distinct from
    // update's block-only one, so the Task remove→re-insert round-trip must
    // reproduce the file byte-for-byte for a re-install to be a true no-op.
    let config_dir = TempDir::new().unwrap();
    let index_dir = TempDir::new().unwrap();
    let url = write_task_index(index_dir.path(), "redis", "0.1.0");
    let project = project_with_taskfile(TASKFILE_WITH_DEFAULT);

    jtr()
        .current_dir(project.path())
        .env("JTR_CONFIG_DIR", config_dir.path())
        .env("JTR_INDEX_URL", &url)
        .args(["install", "redis"])
        .assert()
        .success();
    let after_first = fs::read_to_string(project.path().join("Taskfile.yml")).unwrap();

    jtr()
        .current_dir(project.path())
        .env("JTR_CONFIG_DIR", config_dir.path())
        .env("JTR_INDEX_URL", &url)
        .args(["install", "redis"])
        .assert()
        .success()
        .stdout(str::contains("already at version"));
    let after_second = fs::read_to_string(project.path().join("Taskfile.yml")).unwrap();

    assert_eq!(
        after_first, after_second,
        "a second install of the same version must leave the Taskfile byte-identical"
    );
    assert_eq!(after_second.matches("# >>> jtr:redis@").count(), 1);
}

/// Regression for #19: piping multi-line output into a reader that closes the
/// pipe early (`jtr search | head`, `jtr list | grep -q`) must not panic. Rust
/// defaults SIGPIPE to SIG_IGN, so a write to a closed pipe returned EPIPE and
/// `println!` panicked; `main` now resets SIGPIPE to SIG_DFL.
///
/// We close the read end *before* jtr's first write (rather than reading a line
/// first) so the very first write hits a reader-less pipe deterministically —
/// reading first would race jtr dumping its small output into the pipe buffer
/// and exiting cleanly before we close.
#[cfg(unix)]
#[test]
fn writing_to_a_closed_pipe_does_not_panic() {
    use std::io::Read;
    use std::os::unix::process::ExitStatusExt;
    use std::process::{Command as StdCommand, Stdio};

    let config_dir = TempDir::new().unwrap();
    let mut child = StdCommand::new(env!("CARGO_BIN_EXE_jtr"))
        .arg("search")
        .env("JTR_INDEX_URL", sample_index_url())
        .env("JTR_CONFIG_DIR", config_dir.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn jtr");

    drop(child.stdout.take());

    let mut stderr = String::new();
    child
        .stderr
        .take()
        .expect("stderr piped")
        .read_to_string(&mut stderr)
        .expect("read stderr");
    let status = child.wait().expect("wait for jtr");

    assert!(
        !stderr.contains("panicked") && !stderr.contains("Broken pipe"),
        "jtr panicked writing to a closed pipe (status: {status:?}):\n{stderr}"
    );
    // SIGPIPE is 13 on Linux/macOS; SIG_DFL termination should report it.
    assert_eq!(
        status.signal(),
        Some(13),
        "expected termination by SIGPIPE, got {status:?}"
    );
}
