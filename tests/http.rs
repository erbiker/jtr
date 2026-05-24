//! HTTP integration tests. Stands up a tiny localhost HTTP server that serves the
//! bundled sample index, then runs the real CLI against it. This is the only test that
//! exercises the production HTTP fetch path — every other integration test uses `file://`.
//!
//! The server serves manifests verbatim (no rewriting), so once checksum verification
//! lands the test continues to pass — the served bytes match the index's declared hash.

use assert_cmd::Command;
use predicates::str;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tempfile::TempDir;
use tiny_http::{Header, Response, Server};

type RequestLog = Arc<Mutex<Vec<String>>>;

fn jtr() -> Command {
    Command::cargo_bin("jtr").expect("locate jtr binary")
}

/// Spawn an HTTP server on an ephemeral localhost port that serves files from the
/// repository's `jtr-index/` directory. Returns the URL of the root index file and a
/// shared log of every URL path the server received.
///
/// The server thread is intentionally leaked: it blocks on `incoming_requests()` and
/// is reaped when the test process exits. Fine for a single-test use case; revisit
/// if a second HTTP test is added.
fn spawn_index_server() -> (String, RequestLog) {
    let index_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("jtr-index");
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
            serve_file(&index_root, req);
        }
    });
    (format!("http://127.0.0.1:{port}/index.json"), log)
}

fn serve_file(root: &std::path::Path, req: tiny_http::Request) {
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

#[test]
fn install_works_against_real_http_server() {
    let (index_url, log) = spawn_index_server();

    let project = TempDir::new().expect("create temp project");
    fs::write(project.path().join("justfile"), "default:\n    @echo hi\n").unwrap();

    jtr()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", &index_url)
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
