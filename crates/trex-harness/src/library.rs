use anyhow::{Context, bail};
use trex_sandbox::{OpenShell, Sandbox};
use trex_store::library::Library;
use uuid::Uuid;

pub async fn copy_to_sandbox(
    library: &Library,
    user: Uuid,
    openshell: &OpenShell,
    sandbox: &Sandbox,
    dest: &str,
) -> anyhow::Result<usize> {
    let files = library.list(user).await?;

    let mut archive = tar::Builder::new(Vec::new());
    for file in &files {
        let content = library.get(user, &file.path).await?;
        let mut header = tar::Header::new_gnu();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_mtime(file.modified_at.max(0) as u64);
        archive
            .append_data(&mut header, &file.path, content.as_slice())
            .with_context(|| format!("failed to archive {}", file.path))?;
    }
    let archive = archive.into_inner().context("failed to finish archive")?;

    // dest is passed as $1 so it is never interpreted by the shell
    let script = r#"mkdir -p -- "$1" && tar -xf - --no-same-owner -C "$1""#;
    let argv = ["sh", "-c", script, "sh", dest].map(String::from).to_vec();
    let output = openshell.output(sandbox, argv, archive).await?;
    if output.exit_code != Some(0) {
        bail!(
            "failed to unpack library: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    tracing::debug!(%user, sandbox = sandbox.name, files = files.len(), "copied library to sandbox");
    Ok(files.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::sandbox_for_new_user;

    // needs the openshell gateway tunnel and <workspace>/certs/openshell: cargo test -- --ignored
    #[tokio::test]
    #[ignore]
    async fn copies_library_into_sandbox() {
        let (openshell, user, sandbox) = sandbox_for_new_user().await;
        let library = Library::in_memory();
        let large = vec![b'x'; 3 * 1024 * 1024];
        library
            .put(user, "notes/todo.md", b"ship trex\n".to_vec())
            .await
            .unwrap();
        library.put(user, "data/large.bin", large).await.unwrap();

        let copied =
            copy_to_sandbox(&library, user, &openshell, &sandbox, "/sandbox/library").await;
        let check = openshell
            .output(
                &sandbox,
                [
                    "sh",
                    "-c",
                    "cat /sandbox/library/notes/todo.md && wc -c < /sandbox/library/data/large.bin",
                ]
                .map(String::from)
                .to_vec(),
                Vec::new(),
            )
            .await
            .unwrap();

        openshell.delete_workspace(user).await.unwrap();

        assert_eq!(copied.unwrap(), 2);
        assert_eq!(
            String::from_utf8(check.stdout).unwrap(),
            format!("ship trex\n{}\n", 3 * 1024 * 1024)
        );
    }
}
