//! Analysis cache for repeated runs over the same bytes and parameters.
//!
//! A cache entry is a directory holding `document.json` plus its
//! `document.json.artifacts/` tree. Artifact paths are rewritten on the way in
//! and out so a cached document can be copied to a different output path
//! without dangling images.
//!
//! The key is the media SHA-256, the effective `AnalysisConfig` hash, the
//! annotation flags, and the generator version. Hashing is required: a cache
//! keyed by path would serve stale data after an in-place edit.

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Serialize;
use sha2::{Digest, Sha256};
use sonolytica_core::{AnalysisConfig, Document, GENERATOR_VERSION, SCHEMA_VERSION, Warning};

/// The post-analysis flags that change the document but not `AnalysisConfig`.
#[derive(Debug, Clone, Serialize)]
pub struct Annotations {
    pub ocr: bool,
    pub faces: bool,
    pub objects: bool,
    pub pose: bool,
    pub caption: bool,
    pub no_frames: bool,
    pub max_frames: usize,
    pub frame_stride: Option<f32>,
}

impl Annotations {
    fn sha256(&self) -> String {
        let bytes = serde_json::to_vec(self).unwrap_or_default();
        let digest = Sha256::digest(&bytes);
        format!("{digest:x}")
    }
}

/// Bump when the analysis pipeline changes but the crate version does not.
const CACHE_VERSION: &str = "1";

pub struct Cache {
    dir: PathBuf,
}

impl Cache {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Number of cached analyses and their total size in bytes.
    pub fn stats(&self) -> (usize, u64) {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return (0, 0);
        };
        let mut count = 0;
        let mut bytes = 0;
        for entry in entries.flatten() {
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                count += 1;
                bytes += tree_size(&entry.path());
            }
        }
        (count, bytes)
    }

    /// Stable key for one analysis. `media_sha` is the file content digest.
    pub fn key(
        &self,
        media_sha: &str,
        config: &AnalysisConfig,
        annotations: &Annotations,
    ) -> String {
        let mut hasher = Sha256::new();
        hasher.update(media_sha.as_bytes());
        hasher.update(b"\0");
        hasher.update(config.params_sha256().as_bytes());
        hasher.update(b"\0");
        hasher.update(annotations.sha256().as_bytes());
        hasher.update(b"\0");
        hasher.update(GENERATOR_VERSION.as_bytes());
        hasher.update(b"\0");
        hasher.update(SCHEMA_VERSION.as_bytes());
        hasher.update(b"\0");
        hasher.update(CACHE_VERSION.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    /// Load a cached document into `out`, copying its artifacts next to it.
    ///
    /// Returns `None` when the entry is missing or unreadable; a corrupt cache
    /// is a miss, never an error that blocks a fresh analysis.
    pub fn load(&self, key: &str, out: &Path, media: &Path) -> Option<Document> {
        let entry = self.dir.join(key);
        let source = entry.join("document.json");
        let text = fs::read_to_string(&source).ok()?;
        let mut document = Document::from_json(&text).ok()?;

        let new_name = out.file_name()?.to_string_lossy().into_owned();
        let new_prefix = format!("{new_name}.artifacts");
        let old_prefix = "document.json.artifacts/";

        let source_artifacts = entry.join(old_prefix);
        let target_artifacts = out
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .join(&new_prefix);
        if source_artifacts.is_dir() && target_artifacts != source_artifacts {
            let _ = fs::remove_dir_all(&target_artifacts);
            copy_tree(&source_artifacts, &target_artifacts).ok()?;
        }

        for artifact in &mut document.artifacts {
            if let Some(rest) = artifact.path.strip_prefix(old_prefix) {
                artifact.path = format!("{new_prefix}/{rest}");
            }
        }
        document.media.path = media.to_string_lossy().into_owned();
        document.warnings.push(Warning::new(
            "analysis_cached",
            format!(
                "this document is a cached analysis of the same bytes and parameters \
                 (created {} ms since the epoch); it was not re-run",
                document.created_unix_ms
            ),
        ));
        Some(document)
    }

    /// Store `document` and its artifacts. The document must already be
    /// written at `out` so relative artifact paths resolve.
    pub fn store(&self, key: &str, out: &Path, document: &Document) -> Result<()> {
        let Some(name) = out.file_name() else {
            bail!("cache needs a file output path");
        };
        let prefix = format!("{}.artifacts/", name.to_string_lossy());
        let target = self.dir.join(key);
        if target.exists() {
            return Ok(());
        }

        let temp = self.dir.join(format!("{key}.{}.tmp", std::process::id()));
        let _ = fs::remove_dir_all(&temp);
        fs::create_dir_all(&temp)
            .with_context(|| format!("could not create {}", temp.display()))?;

        let mut cached = document.clone();
        for artifact in &mut cached.artifacts {
            let Some(rest) = artifact.path.strip_prefix(&prefix).map(str::to_string) else {
                continue;
            };
            if !safe_relative(&rest) {
                let _ = fs::remove_dir_all(&temp);
                bail!(
                    "refusing an artifact path that escapes the document: {}",
                    artifact.path
                );
            }
            artifact.path = format!("document.json.artifacts/{rest}");
            let source = out
                .parent()
                .unwrap_or_else(|| Path::new(""))
                .join(&prefix)
                .join(&rest);
            let destination = temp.join("document.json.artifacts").join(&rest);
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&source, &destination)
                .with_context(|| format!("could not cache artifact {}", source.display()))?;
        }

        fs::write(temp.join("document.json"), cached.to_json_pretty()?)
            .with_context(|| format!("could not write {}", temp.join("document.json").display()))?;
        fs::create_dir_all(&self.dir)?;
        match fs::rename(&temp, &target) {
            Ok(()) => Ok(()),
            Err(_) if target.exists() => {
                let _ = fs::remove_dir_all(&temp);
                Ok(())
            }
            Err(error) => {
                let _ = fs::remove_dir_all(&temp);
                Err(error.into())
            }
        }
    }
}

fn safe_relative(path: &str) -> bool {
    let path = Path::new(path);
    path.components()
        .all(|component| matches!(component, Component::Normal(_) | Component::CurDir))
}

fn tree_size(path: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(path) else {
        return 0;
    };
    let mut total = 0;
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            total += tree_size(&entry.path());
        } else if let Ok(metadata) = entry.metadata() {
            total += metadata.len();
        }
    }
    total
}

fn copy_tree(source: &Path, target: &Path) -> io::Result<()> {
    fs::create_dir_all(target)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let path = entry.path();
        let destination = target.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&path, &destination)?;
        } else {
            fs::copy(&path, &destination)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sonolytica_core::{Document, MediaInfo, TimeRange};

    fn temp_dir(tag: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("sonolytica-cache-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn config() -> AnalysisConfig {
        AnalysisConfig::default()
    }

    fn annotations() -> Annotations {
        Annotations {
            ocr: false,
            faces: false,
            objects: false,
            pose: false,
            caption: false,
            no_frames: false,
            max_frames: 40,
            frame_stride: None,
        }
    }

    #[test]
    fn key_is_stable_and_sensitive() {
        let cache = Cache::new(PathBuf::from("/tmp/cache"));
        let first = cache.key("abc", &config(), &annotations());
        assert_eq!(first, cache.key("abc", &config(), &annotations()));
        assert_ne!(first, cache.key("abd", &config(), &annotations()));

        let mut other = annotations();
        other.pose = true;
        assert_ne!(first, cache.key("abc", &config(), &other));

        let mut changed = config();
        changed.align_words = true;
        assert_ne!(first, cache.key("abc", &changed, &annotations()));
    }

    #[test]
    fn store_and_load_rewrites_paths_and_media() {
        let root = temp_dir("roundtrip");
        let out = root.join("first.json");
        let artifacts = root.join("first.json.artifacts/frames");
        fs::create_dir_all(&artifacts).unwrap();
        fs::write(artifacts.join("frame_0000.jpg"), b"jpeg").unwrap();

        let mut document = Document::new(
            MediaInfo {
                path: "/old/place.mp4".into(),
                sha256: Some("abc".into()),
                size_bytes: None,
                container: None,
                duration: None,
                bit_rate: None,
                streams: Vec::new(),
            },
            AnalysisConfig::default(),
        );
        document.artifacts.push(sonolytica_core::Artifact {
            kind: "keyframe".into(),
            path: "first.json.artifacts/frames/frame_0000.jpg".into(),
            range: TimeRange::from_secs(0.0, 0.0),
            sha256: None,
            width: None,
            height: None,
        });

        let cache = Cache::new(root.join("cache"));
        cache.store("key", &out, &document).unwrap();

        let new_out = root.join("second.json");
        let loaded = cache
            .load("key", &new_out, Path::new("/new/place.mp4"))
            .unwrap();
        assert_eq!(loaded.media.path, "/new/place.mp4");
        assert_eq!(
            loaded.artifacts[0].path,
            "second.json.artifacts/frames/frame_0000.jpg"
        );
        assert!(
            root.join("second.json.artifacts/frames/frame_0000.jpg")
                .is_file()
        );
        assert!(
            loaded
                .warnings
                .iter()
                .any(|warning| warning.code == "analysis_cached")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_missing_entry_is_a_miss() {
        let root = temp_dir("miss");
        let cache = Cache::new(root.join("cache"));
        assert!(
            cache
                .load("nope", &root.join("out.json"), Path::new("/media"))
                .is_none()
        );
        let _ = fs::remove_dir_all(root);
    }
}
