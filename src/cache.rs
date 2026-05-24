use anyhow::{Context, Result, anyhow};
use directories::ProjectDirs;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::index::sha256_hex;

/// Time-to-live for cached `index.json` bodies. Matches DEEP_DIVE §3.
const INDEX_TTL: Duration = Duration::from_secs(60 * 60);

/// On-disk cache layout:
///
/// ```text
/// <root>/
/// ├── indices/<sha256-of-url>.json    # URL-keyed, mtime is the freshness clock
/// └── manifests/<sha256-of-content>.json
/// ```
///
/// Manifests are content-addressed by their declared SHA-256, so they're
/// immutable — no TTL, no invalidation. A rotated checksum in the parent
/// index simply points lookups at a different filename.
///
/// Cache I/O is best-effort: read misses are silent, write failures emit a
/// warning and the command keeps going. The cache is a perf optimization,
/// not a correctness boundary.
#[derive(Debug, Clone)]
pub struct Cache {
    root: PathBuf,
}

impl Cache {
    /// Returns `Ok(None)` when no cache directory can be determined (rare —
    /// only on platforms where `directories` can't pick a default). Returns
    /// `Err` only when an env-override path is invalid in a way the user
    /// must fix.
    pub fn open() -> Result<Option<Self>> {
        let root = match std::env::var_os("JTR_CACHE_DIR") {
            Some(custom) => PathBuf::from(custom),
            None => match ProjectDirs::from("dev", "jtr", "jtr") {
                Some(dirs) => dirs.cache_dir().to_path_buf(),
                None => return Ok(None),
            },
        };
        Ok(Some(Self { root }))
    }

    #[cfg(test)]
    fn with_root(root: PathBuf) -> Self {
        Self { root }
    }

    fn indices_dir(&self) -> PathBuf {
        self.root.join("indices")
    }

    fn manifests_dir(&self) -> PathBuf {
        self.root.join("manifests")
    }

    fn index_path(&self, url: &str) -> PathBuf {
        self.indices_dir()
            .join(format!("{}.json", sha256_hex(url.as_bytes())))
    }

    fn manifest_path(&self, sha256: &str) -> PathBuf {
        self.manifests_dir().join(format!("{sha256}.json"))
    }

    /// Read the cached index body for `url`, returning `None` if absent or
    /// older than [`INDEX_TTL`]. Read errors are treated as cache misses.
    pub fn read_index(&self, url: &str) -> Option<String> {
        let path = self.index_path(url);
        let meta = fs::metadata(&path).ok()?;
        let modified = meta.modified().ok()?;
        if SystemTime::now()
            .duration_since(modified)
            .map(|age| age > INDEX_TTL)
            .unwrap_or(true)
        {
            return None;
        }
        fs::read_to_string(&path).ok()
    }

    /// Write `body` as the cached index for `url`. Atomic on the cache
    /// filesystem; failures emit a warning and return `Err` for the caller
    /// to log-and-eat.
    pub fn write_index(&self, url: &str, body: &str) -> Result<()> {
        atomic_write(&self.indices_dir(), &self.index_path(url), body.as_bytes())
    }

    /// Read the cached manifest body for `expected_sha256`. Defensively
    /// re-verifies the on-disk bytes against the expected hash so a corrupted
    /// cache (e.g. partial write that escaped the atomic rename, manual edit)
    /// can't produce a checksum-mismatched manifest at the caller. Returns
    /// `None` on any mismatch or read error.
    pub fn read_manifest(&self, expected_sha256: &str) -> Option<String> {
        let path = self.manifest_path(expected_sha256);
        let body = fs::read_to_string(&path).ok()?;
        if sha256_hex(body.as_bytes()) != expected_sha256 {
            let _ = fs::remove_file(&path);
            return None;
        }
        Some(body)
    }

    /// Write `body` indexed by `expected_sha256`. Caller is responsible for
    /// having already verified the hash; this method just persists the bytes.
    pub fn write_manifest(&self, expected_sha256: &str, body: &str) -> Result<()> {
        atomic_write(
            &self.manifests_dir(),
            &self.manifest_path(expected_sha256),
            body.as_bytes(),
        )
    }
}

/// Write `bytes` to `dest` atomically. Creates `parent_dir` if needed and uses
/// a tempfile in the same directory so the final `persist` is a true atomic
/// rename within one filesystem (NamedTempFile::new() picks the OS temp dir
/// which on Linux is often a different fs from `~/.cache/jtr/`).
fn atomic_write(parent_dir: &Path, dest: &Path, bytes: &[u8]) -> Result<()> {
    fs::create_dir_all(parent_dir)
        .with_context(|| format!("could not create {}", parent_dir.display()))?;
    let mut tmp = tempfile::Builder::new()
        .prefix(".jtr-cache-")
        .tempfile_in(parent_dir)
        .with_context(|| format!("could not create tempfile in {}", parent_dir.display()))?;
    tmp.write_all(bytes)
        .with_context(|| format!("could not write tempfile in {}", parent_dir.display()))?;
    tmp.as_file()
        .sync_all()
        .with_context(|| format!("could not fsync tempfile in {}", parent_dir.display()))?;
    tmp.persist(dest)
        .map_err(|e| anyhow!("could not rename into {}: {}", dest.display(), e.error))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn with_cache<F: FnOnce(&Cache)>(f: F) {
        // Bypass `JTR_CACHE_DIR` in unit tests — the env var is process-global
        // and these tests run in parallel, so set/remove on it races and tests
        // can collide on cache filenames (every test that writes "https://example.com/..."
        // hashes to the same path). The integration tests in `tests/http.rs`
        // exercise the env-var path properly via subprocess invocations.
        let dir = TempDir::new().unwrap();
        let cache = Cache::with_root(dir.path().to_path_buf());
        f(&cache);
        drop(dir);
    }

    #[test]
    fn index_roundtrip_hits_within_ttl() {
        with_cache(|cache| {
            let url = "https://example.com/index.json";
            assert!(cache.read_index(url).is_none(), "fresh cache must miss");
            cache
                .write_index(url, r#"{"version":1,"recipes":[]}"#)
                .unwrap();
            let body = cache.read_index(url).expect("warm cache must hit");
            assert!(body.contains("recipes"));
        });
    }

    #[test]
    fn index_misses_after_ttl_via_mtime_backdate() {
        with_cache(|cache| {
            let url = "https://example.com/index.json";
            cache.write_index(url, "{}").unwrap();
            // Backdate the mtime past the TTL. If `set_modified` errors on a
            // weird filesystem, the test is moot — skip the assertion.
            let path = cache.index_path(url);
            let two_hours_ago = SystemTime::now() - Duration::from_secs(60 * 60 * 2);
            if std::fs::File::open(&path)
                .and_then(|f| f.set_modified(two_hours_ago))
                .is_ok()
            {
                assert!(cache.read_index(url).is_none(), "index past TTL must miss");
            }
        });
    }

    #[test]
    fn distinct_urls_cache_independently() {
        // Acceptance criterion: each tap's index is cached separately. URLs
        // hash to distinct filenames by construction, but assert it so a
        // future refactor of the keying scheme doesn't silently regress.
        with_cache(|cache| {
            let curated = "https://example.com/curated/index.json";
            let tap = "https://example.com/alice/index.json";
            cache.write_index(curated, "curated-body").unwrap();
            cache.write_index(tap, "tap-body").unwrap();

            // Backdate ONLY the curated entry past TTL; the tap entry must
            // still hit.
            let two_hours_ago = SystemTime::now() - Duration::from_secs(60 * 60 * 2);
            if std::fs::File::open(cache.index_path(curated))
                .and_then(|f| f.set_modified(two_hours_ago))
                .is_ok()
            {
                assert!(cache.read_index(curated).is_none());
                assert_eq!(cache.read_index(tap).as_deref(), Some("tap-body"));
            }
        });
    }

    #[test]
    fn manifest_roundtrip_is_content_addressed() {
        with_cache(|cache| {
            let body = r#"{"name":"foo","version":"0.1.0","description":"x","targets":{}}"#;
            let sha = sha256_hex(body.as_bytes());
            assert!(cache.read_manifest(&sha).is_none(), "fresh cache must miss");
            cache.write_manifest(&sha, body).unwrap();
            assert_eq!(cache.read_manifest(&sha).as_deref(), Some(body));
        });
    }

    #[test]
    fn manifest_rejects_corrupted_cache_entry() {
        with_cache(|cache| {
            let body = r#"{"name":"foo"}"#;
            let sha = sha256_hex(body.as_bytes());
            cache.write_manifest(&sha, body).unwrap();
            // Tamper with the on-disk bytes; expected hash no longer matches.
            std::fs::write(cache.manifest_path(&sha), b"tampered").unwrap();
            assert!(cache.read_manifest(&sha).is_none());
            // Tampered file is removed so a future fetch can repopulate cleanly.
            assert!(!cache.manifest_path(&sha).exists());
        });
    }

    #[test]
    fn write_creates_parent_dirs() {
        with_cache(|cache| {
            assert!(!cache.indices_dir().exists());
            cache.write_index("https://example.com/x", "{}").unwrap();
            assert!(cache.indices_dir().is_dir());
        });
    }
}
