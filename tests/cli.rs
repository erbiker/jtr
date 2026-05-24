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
    let occurrences = result.matches("# >>> jtr:postgres-dev").count();
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
fn install_into_taskfile_errors_because_no_seed_supports_task_yet() {
    // Seed recipes only declare a `just` target today. Installing into a Taskfile.yml
    // should fail with a clear "does not support target 'task'" message. When the first
    // task-supporting recipe lands, add a sibling test for the "task target not yet
    // implemented" code path.
    let project = TempDir::new().unwrap();
    fs::write(project.path().join("Taskfile.yml"), "version: '3'\n").unwrap();

    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", sample_index_url())
        .args(["install", "postgres-dev"])
        .assert()
        .failure()
        .stderr(str::contains("does not support target 'task'"));
}
