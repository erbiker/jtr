//! HTTP integration tests. Stands up a tiny localhost HTTP server that serves the
//! bundled sample index, then runs the real CLI against it. The shared server records
//! every request URL so we can assert against the network traffic the local disk cache
//! is supposed to suppress.
//!
//! All jtr invocations point `JTR_CACHE_DIR` at a per-test tempdir so they never touch
//! the developer's real cache.

use assert_cmd::Command;
use predicates::str;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use tempfile::TempDir;
use tiny_http::{Header, Response, Server};

type RequestLog = Arc<Mutex<Vec<String>>>;

fn jtr() -> Command {
    Command::cargo_bin("jtr").expect("locate jtr binary")
}

/// Spawn an HTTP server on an ephemeral localhost port that serves files from
/// `root_dir`. Returns the URL of the root index file and a shared log of every
/// URL path the server received.
///
/// The server thread is intentionally leaked: it blocks on `incoming_requests()`
/// and is reaped when the test process exits. Multiple tests each spawn their
/// own server on its own port, so this is fine.
fn spawn_server(root_dir: PathBuf) -> (String, RequestLog) {
    let server = Server::http("127.0.0.1:0").expect("bind ephemeral port");
    let port = match server.server_addr() {
        tiny_http::ListenAddr::IP(addr) => addr.port(),
        other => panic!("expected TCP listener, got {other:?}"),
    };
    let log: RequestLog = Arc::new(Mutex::new(Vec::new()));
    let log_for_thread = log.clone();
    std::thread::spawn(move || {
        for req in server.incoming_requests() {
            log_for_thread
                .lock()
                .expect("request log mutex poisoned")
                .push(req.url().to_string());
            serve_file(&root_dir, req);
        }
    });
    (format!("http://127.0.0.1:{port}/index.json"), log)
}

fn serve_file(root: &Path, req: tiny_http::Request) {
    let rel = req.url().trim_start_matches('/');
    let path = root.join(rel);
    match fs::read(&path) {
        Ok(bytes) => {
            let header: Header = "Content-Type: application/json"
                .parse()
                .expect("static header parses");
            let _ = req.respond(Response::from_data(bytes).with_header(header));
        }
        Err(_) => {
            let _ = req.respond(Response::from_string("not found").with_status_code(404));
        }
    }
}

fn bundled_index_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("jtr-index")
}

fn log_len(log: &RequestLog) -> usize {
    log.lock().expect("request log mutex poisoned").len()
}

#[test]
fn install_works_against_real_http_server() {
    let (index_url, log) = spawn_server(bundled_index_root());

    let project = TempDir::new().expect("create temp project");
    let cache = TempDir::new().expect("create cache dir");
    fs::write(project.path().join("justfile"), "default:\n    @echo hi\n").unwrap();

    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", &index_url)
        .env("JTR_CACHE_DIR", cache.path())
        .args(["install", "postgres-dev"])
        .assert()
        .success()
        .stdout(str::contains("installed"));

    let body = fs::read_to_string(project.path().join("justfile")).unwrap();
    assert!(body.contains("# >>> jtr:postgres-dev@0.1.0 >>>"));
    assert!(body.contains("postgres-up:"));

    // Verifies the full HTTP code path: the index was fetched, *and* the relative
    // `manifest_url` was correctly resolved against the absolute base URL.
    let urls = log.lock().expect("request log mutex poisoned").clone();
    assert!(
        urls.iter().any(|u| u == "/index.json"),
        "expected /index.json fetch, saw: {urls:?}"
    );
    assert!(
        urls.iter().any(|u| u == "/recipes/postgres-dev.json"),
        "expected /recipes/postgres-dev.json fetch (proves relative URL resolution), saw: {urls:?}"
    );
}

#[test]
fn cache_suppresses_network_on_warm_invocation() {
    // Cold install fetches index + manifest; second `jtr search` would normally
    // re-fetch the index. With the cache warm, the second call must touch zero
    // network — assert by diffing the server's request log across calls.
    let (index_url, log) = spawn_server(bundled_index_root());
    let project = TempDir::new().unwrap();
    let cache = TempDir::new().unwrap();
    fs::write(project.path().join("justfile"), "default:\n    @echo hi\n").unwrap();

    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", &index_url)
        .env("JTR_CACHE_DIR", cache.path())
        .args(["install", "postgres-dev"])
        .assert()
        .success();

    let calls_after_install = log_len(&log);
    assert!(
        calls_after_install >= 2,
        "cold install should hit at least index + manifest, saw {calls_after_install}"
    );

    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", &index_url)
        .env("JTR_CACHE_DIR", cache.path())
        .args(["search"])
        .assert()
        .success();

    let calls_after_search = log_len(&log);
    assert_eq!(
        calls_after_search,
        calls_after_install,
        "warm-cache `jtr search` must use zero network; saw {} new request(s)",
        calls_after_search - calls_after_install
    );
}

#[test]
fn no_cache_flag_forces_a_refetch() {
    let (index_url, log) = spawn_server(bundled_index_root());
    let cache = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    fs::write(project.path().join("justfile"), "default:\n    @echo hi\n").unwrap();

    // Warm the cache.
    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", &index_url)
        .env("JTR_CACHE_DIR", cache.path())
        .args(["search"])
        .assert()
        .success();
    let after_warm = log_len(&log);

    // Bypass the cache: must produce a new index fetch.
    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", &index_url)
        .env("JTR_CACHE_DIR", cache.path())
        .args(["--no-cache", "search"])
        .assert()
        .success();
    let after_bypass = log_len(&log);

    assert!(
        after_bypass > after_warm,
        "--no-cache must trigger at least one new HTTP fetch (was {after_warm}, now {after_bypass})"
    );
}

#[test]
fn cached_index_past_ttl_is_refetched() {
    let (index_url, log) = spawn_server(bundled_index_root());
    let cache = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    fs::write(project.path().join("justfile"), "default:\n    @echo hi\n").unwrap();

    // First fetch populates the cache; record how many requests that took.
    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", &index_url)
        .env("JTR_CACHE_DIR", cache.path())
        .args(["search"])
        .assert()
        .success();
    let after_first = log_len(&log);

    // Backdate every cached index by ~2 hours — past the 1h TTL. If the host
    // filesystem refuses `set_modified` we can't exercise the path, so skip
    // the assertion rather than fail spuriously.
    let two_hours_ago = SystemTime::now() - Duration::from_secs(60 * 60 * 2);
    let indices_dir = cache.path().join("indices");
    let mut backdated_any = false;
    if let Ok(entries) = fs::read_dir(&indices_dir) {
        for entry in entries.flatten() {
            if let Ok(f) = std::fs::File::open(entry.path())
                && f.set_modified(two_hours_ago).is_ok()
            {
                backdated_any = true;
            }
        }
    }
    if !backdated_any {
        return;
    }

    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", &index_url)
        .env("JTR_CACHE_DIR", cache.path())
        .args(["search"])
        .assert()
        .success();
    let after_stale = log_len(&log);

    assert!(
        after_stale > after_first,
        "index past TTL must be refetched (was {after_first}, now {after_stale})"
    );
}

#[test]
fn rotated_manifest_checksum_falls_through_to_a_new_fetch() {
    // Content-addressed manifest cache: when the index advertises a new sha256,
    // lookups go to a different filename and naturally miss. To exercise that
    // path we must first ensure the CLI sees the rotated index — within the 1h
    // TTL the cached index hides any change. We backdate the cached index past
    // TTL so the second invocation re-fetches it, sees the new sha, and then
    // misses the manifest cache (different content address).
    let root = TempDir::new().unwrap();
    let cache = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    fs::write(project.path().join("justfile"), "default:\n    @echo hi\n").unwrap();

    fs::create_dir_all(root.path().join("recipes")).unwrap();
    let manifest_v1 = manifest_body("rotate-me", "0.1.0", "v1");
    write_index_with_manifest(root.path(), "rotate-me", "0.1.0", &manifest_v1);

    let (index_url, log) = spawn_server(root.path().to_path_buf());

    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", &index_url)
        .env("JTR_CACHE_DIR", cache.path())
        .args(["install", "rotate-me"])
        .assert()
        .success();
    let after_v1 = log_len(&log);

    // Maintainer publishes a new manifest body at the SAME url with a new sha.
    let manifest_v2 = manifest_body("rotate-me", "0.2.0", "v2");
    write_index_with_manifest(root.path(), "rotate-me", "0.2.0", &manifest_v2);

    // Backdate cached indices so the CLI re-fetches and sees the rotated sha.
    let two_hours_ago = SystemTime::now() - Duration::from_secs(60 * 60 * 2);
    let mut backdated_any = false;
    if let Ok(entries) = fs::read_dir(cache.path().join("indices")) {
        for entry in entries.flatten() {
            if let Ok(f) = std::fs::File::open(entry.path())
                && f.set_modified(two_hours_ago).is_ok()
            {
                backdated_any = true;
            }
        }
    }
    if !backdated_any {
        return;
    }

    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", &index_url)
        .env("JTR_CACHE_DIR", cache.path())
        .args(["update", "rotate-me"])
        .assert()
        .success();
    let after_v2 = log_len(&log);

    // Two new requests expected: the refreshed index + the v2 manifest.
    assert!(
        after_v2 - after_v1 >= 2,
        "rotated checksum must miss content-addressed cache and refetch both index + manifest (was {after_v1}, now {after_v2})"
    );

    let body = fs::read_to_string(project.path().join("justfile")).unwrap();
    assert!(
        body.contains("# >>> jtr:rotate-me@0.2.0 >>>"),
        "expected v0.2.0 to land on disk after the rotated-manifest update; got:\n{body}"
    );
}

fn manifest_body(name: &str, version: &str, marker: &str) -> String {
    format!(
        r#"{{"name":"{name}","version":"{version}","description":"{marker}","targets":{{"just":{{"snippet":"{name}-{marker}:\n    @echo {marker}\n"}}}}}}"#,
    )
}

fn write_index_with_manifest(root: &Path, name: &str, version: &str, manifest: &str) {
    fs::write(root.join("recipes").join(format!("{name}.json")), manifest).unwrap();
    let sha = sha256_hex(manifest.as_bytes());
    let index = format!(
        r#"{{"version":1,"recipes":[{{"name":"{name}","version":"{version}","description":"d","manifest_url":"recipes/{name}.json","targets":["just"],"sha256":"{sha}"}}]}}"#
    );
    fs::write(root.join("index.json"), index).unwrap();
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}
