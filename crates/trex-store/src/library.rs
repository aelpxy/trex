use std::{fmt::Write, fs, path::Path as FsPath, sync::Arc};

use anyhow::Context;
use futures::TryStreamExt;
use object_store::{
    ObjectStore, ObjectStoreExt, aws::AmazonS3Builder, local::LocalFileSystem, memory::InMemory,
    path::Path,
};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub struct S3Config {
    pub endpoint: Option<String>,
    pub region: String,
    pub bucket: String,
    pub access_key_id: String,
    pub secret_access_key: String,
    pub force_path_style: bool,
}

pub struct LibraryFile {
    pub path: String,
    pub size: u64,
    pub modified_at: i64,
}

// each user's files live under users/{id}/library/ in one bucket or directory
pub struct Library {
    store: Arc<dyn ObjectStore>,
}

impl Library {
    pub fn s3(config: S3Config) -> anyhow::Result<Self> {
        let mut builder = AmazonS3Builder::new()
            .with_region(config.region)
            .with_bucket_name(config.bucket)
            .with_access_key_id(config.access_key_id)
            .with_secret_access_key(config.secret_access_key)
            .with_virtual_hosted_style_request(!config.force_path_style);
        if let Some(endpoint) = config.endpoint {
            builder = builder
                .with_allow_http(endpoint.starts_with("http://"))
                .with_endpoint(endpoint);
        }
        let store = builder.build().context("invalid s3 configuration")?;
        Ok(Self {
            store: Arc::new(store),
        })
    }

    pub fn local(dir: &FsPath) -> anyhow::Result<Self> {
        fs::create_dir_all(dir).with_context(|| format!("failed to create {}", dir.display()))?;
        let store = LocalFileSystem::new_with_prefix(dir)
            .with_context(|| format!("invalid library directory {}", dir.display()))?;
        Ok(Self {
            store: Arc::new(store),
        })
    }

    pub fn in_memory() -> Self {
        Self {
            store: Arc::new(InMemory::new()),
        }
    }

    pub async fn put(&self, user: Uuid, path: &str, content: Vec<u8>) -> anyhow::Result<()> {
        let key = key(user, path)?;
        self.store
            .put(&key, content.into())
            .await
            .with_context(|| format!("failed to store {path}"))?;
        Ok(())
    }

    pub async fn get(&self, user: Uuid, path: &str) -> anyhow::Result<Vec<u8>> {
        let key = key(user, path)?;
        let object = self
            .store
            .get(&key)
            .await
            .with_context(|| format!("failed to read {path}"))?;
        let bytes = object
            .bytes()
            .await
            .with_context(|| format!("failed to read {path}"))?;
        Ok(bytes.to_vec())
    }

    pub async fn delete(&self, user: Uuid, path: &str) -> anyhow::Result<()> {
        let key = key(user, path)?;
        self.store
            .delete(&key)
            .await
            .with_context(|| format!("failed to delete {path}"))
    }

    // attachments are content-addressed and kept apart from the files the user browses
    pub async fn put_attachment(&self, user: Uuid, content: Vec<u8>) -> anyhow::Result<String> {
        let mut hash = String::with_capacity(64);
        for byte in Sha256::digest(&content) {
            write!(hash, "{byte:02x}").expect("writing to a string cannot fail");
        }
        self.store
            .put(&attachment_key(user, &hash)?, content.into())
            .await
            .context("failed to store attachment")?;
        Ok(hash)
    }

    pub async fn get_attachment(&self, user: Uuid, hash: &str) -> anyhow::Result<Vec<u8>> {
        let object = self
            .store
            .get(&attachment_key(user, hash)?)
            .await
            .with_context(|| format!("failed to read attachment {hash}"))?;
        let bytes = object
            .bytes()
            .await
            .with_context(|| format!("failed to read attachment {hash}"))?;
        Ok(bytes.to_vec())
    }

    pub async fn list(&self, user: Uuid) -> anyhow::Result<Vec<LibraryFile>> {
        let root = root(user);
        let prefix = format!("{root}/");
        let mut files: Vec<LibraryFile> = self
            .store
            .list(Some(&root))
            .map_ok(|meta| LibraryFile {
                path: meta
                    .location
                    .as_ref()
                    .strip_prefix(&prefix)
                    .unwrap_or(meta.location.as_ref())
                    .to_owned(),
                size: meta.size,
                modified_at: meta.last_modified.timestamp(),
            })
            .try_collect()
            .await
            .context("failed to list library")?;
        files.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(files)
    }
}

#[derive(Debug)]
pub struct InvalidPath(String);

impl std::fmt::Display for InvalidPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for InvalidPath {}

pub fn is_invalid_path(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| cause.is::<InvalidPath>())
}

pub fn is_not_found(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        matches!(
            cause.downcast_ref::<object_store::Error>(),
            Some(object_store::Error::NotFound { .. })
        )
    })
}

fn attachment_key(user: Uuid, hash: &str) -> anyhow::Result<Path> {
    if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(InvalidPath(format!("invalid attachment {hash:?}")).into());
    }
    Ok(Path::from_iter([
        "users",
        &user.to_string(),
        "attachments",
        hash,
    ]))
}

fn root(user: Uuid) -> Path {
    Path::from_iter(["users", &user.to_string(), "library"])
}

// library paths come from users and models, so anything that could escape the user's prefix is rejected
fn key(user: Uuid, path: &str) -> anyhow::Result<Path> {
    if path.is_empty() || path.len() > 1024 {
        return Err(InvalidPath("library path must be 1 to 1024 bytes".into()).into());
    }
    let mut segments = Vec::new();
    for segment in path.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." || segment.contains('\\') {
            return Err(InvalidPath(format!("invalid library path {path:?}")).into());
        }
        segments.push(segment);
    }
    let mut key = root(user);
    for segment in segments {
        key = key.join(segment);
    }
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stores_attachments_by_content() {
        let library = Library::in_memory();
        let (alice, mallory) = (Uuid::now_v7(), Uuid::now_v7());
        let hash = library
            .put_attachment(alice, b"pixels".to_vec())
            .await
            .unwrap();
        assert_eq!(hash.len(), 64);
        assert_eq!(
            library
                .put_attachment(alice, b"pixels".to_vec())
                .await
                .unwrap(),
            hash
        );
        assert_eq!(
            library.get_attachment(alice, &hash).await.unwrap(),
            b"pixels"
        );
        assert!(library.get_attachment(mallory, &hash).await.is_err());
        assert!(library.get_attachment(alice, "../library/x").await.is_err());
        assert!(
            library.list(alice).await.unwrap().is_empty(),
            "attachments stay out of the library"
        );
    }

    #[test]
    fn rejects_escaping_paths() {
        let user = Uuid::now_v7();
        for path in [
            "",
            "/etc/passwd",
            "../other",
            "a/../../b",
            "a//b",
            "./a",
            "a\\b",
            "a/",
        ] {
            assert!(key(user, path).is_err(), "{path:?} should be rejected");
        }
        assert_eq!(
            key(user, "notes/todo.md").unwrap().as_ref(),
            format!("users/{user}/library/notes/todo.md")
        );
    }

    #[tokio::test]
    async fn local_library_round_trips() {
        let dir = std::env::temp_dir().join(format!("trex-library-{}", Uuid::now_v7()));
        let library = Library::local(&dir).unwrap();
        let user = Uuid::now_v7();

        library
            .put(user, "docs/a.md", b"hello".to_vec())
            .await
            .unwrap();
        let listed = library.list(user).await.unwrap();
        let content = library.get(user, "docs/a.md").await.unwrap();
        let on_disk = std::fs::read(dir.join(format!("users/{user}/library/docs/a.md")));
        std::fs::remove_dir_all(&dir).unwrap();

        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].path, "docs/a.md");
        assert_eq!(content, b"hello");
        assert_eq!(on_disk.unwrap(), b"hello");
    }

    #[tokio::test]
    async fn stores_files_per_user() {
        let library = Library::in_memory();
        let (alice, bob) = (Uuid::now_v7(), Uuid::now_v7());

        library.put(alice, "b.txt", b"two".to_vec()).await.unwrap();
        library
            .put(alice, "docs/a.md", b"one".to_vec())
            .await
            .unwrap();
        library.put(bob, "b.txt", b"bob".to_vec()).await.unwrap();

        let files = library.list(alice).await.unwrap();
        let paths: Vec<_> = files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["b.txt", "docs/a.md"]);
        assert_eq!(files[0].size, 3);
        assert_eq!(library.get(alice, "b.txt").await.unwrap(), b"two");
        assert_eq!(library.get(bob, "b.txt").await.unwrap(), b"bob");

        library.delete(alice, "b.txt").await.unwrap();
        assert!(is_not_found(
            &library.get(alice, "b.txt").await.unwrap_err()
        ));
        let escaped = library.get(alice, "../x").await.unwrap_err();
        assert!(is_invalid_path(&escaped) && !is_not_found(&escaped));
        assert_eq!(library.list(alice).await.unwrap().len(), 1);
        assert_eq!(library.list(bob).await.unwrap().len(), 1);
    }
}
