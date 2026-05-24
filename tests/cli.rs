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
