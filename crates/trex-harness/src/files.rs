use std::fmt;

use trex_sandbox::{OpenShell, Output, Sandbox};

// the files the user sees are the sandbox's home, where the agent works
pub const ROOT: &str = "/sandbox";
pub const MAX_LISTED_FILES: usize = 5000;
pub const MAX_FILE_BYTES: u64 = 10 * 1024 * 1024;

// dependencies, caches and toolchains the agent installs; listing them would bury the user's files
const SKIPPED_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    ".cache",
    ".local",
    ".npm",
    ".pnpm-store",
    ".cargo",
    ".rustup",
    ".mamba",
    "go",
    "__pycache__",
    ".venv",
    "venv",
    "target",
    ".next",
];

const MISSING_EXIT_CODE: i32 = 3;
const TOO_LARGE_EXIT_CODE: i32 = 4;
const EXISTS_EXIT_CODE: i32 = 5;

#[derive(Debug)]
pub enum FileError {
    InvalidPath(String),
    NotFound,
    TooLarge,
    Exists,
    Failed(anyhow::Error),
}

impl fmt::Display for FileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPath(reason) => write!(f, "{reason}"),
            Self::NotFound => write!(f, "no such file"),
            Self::TooLarge => write!(f, "the file is larger than {MAX_FILE_BYTES} bytes"),
            Self::Exists => write!(f, "a file already exists there"),
            Self::Failed(error) => write!(f, "{error:#}"),
        }
    }
}

impl From<anyhow::Error> for FileError {
    fn from(error: anyhow::Error) -> Self {
        Self::Failed(error)
    }
}

pub struct SandboxFile {
    // relative to ROOT
    pub path: String,
    pub size: u64,
    pub modified_at: i64,
}

pub struct Listing {
    pub files: Vec<SandboxFile>,
    pub truncated: bool,
}

// relative paths only, so a request can't reach outside the sandbox's home
pub fn absolute(path: &str) -> Result<String, FileError> {
    let invalid = |reason: &str| Err(FileError::InvalidPath(format!("{path:?} {reason}")));
    if path.is_empty() {
        return invalid("is empty");
    }
    if path.starts_with('/') || path.contains('\0') {
        return invalid("must be relative to the sandbox's home");
    }
    if path
        .split('/')
        .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return invalid("has an empty, . or .. segment");
    }
    Ok(format!("{ROOT}/{path}"))
}

pub async fn list(openshell: &OpenShell, sandbox: &Sandbox) -> anyhow::Result<Listing> {
    let mut argv: Vec<String> = ["find", ROOT, "-mindepth", "1", "-type", "d", "("]
        .map(String::from)
        .to_vec();
    for (index, dir) in SKIPPED_DIRS.iter().enumerate() {
        if index > 0 {
            argv.push("-o".into());
        }
        argv.extend(["-name".into(), (*dir).into()]);
    }
    argv.extend(
        [
            ")",
            "-prune",
            "-o",
            "-type",
            "f",
            "-printf",
            "%P\\0%s\\0%T@\\0",
        ]
        .map(String::from),
    );
    let output = openshell.output(sandbox, argv, Vec::new()).await?;
    // find reports unreadable folders but still lists the rest
    if output.exit_code != Some(0) && output.stdout.is_empty() {
        anyhow::bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
    }
    let fields: Vec<&[u8]> = output.stdout.split(|byte| *byte == 0).collect();
    let mut files: Vec<SandboxFile> = fields
        .as_chunks::<3>()
        .0
        .iter()
        .filter_map(|entry| {
            let path = String::from_utf8(entry[0].to_vec()).ok()?;
            let size = std::str::from_utf8(entry[1]).ok()?.parse().ok()?;
            let modified_at = std::str::from_utf8(entry[2]).ok()?.parse::<f64>().ok()? as i64;
            Some(SandboxFile {
                path,
                size,
                modified_at,
            })
        })
        .collect();
    files.sort_by(|a, b| a.path.cmp(&b.path));
    let truncated = files.len() > MAX_LISTED_FILES;
    files.truncate(MAX_LISTED_FILES);
    Ok(Listing { files, truncated })
}

pub async fn read(
    openshell: &OpenShell,
    sandbox: &Sandbox,
    path: &str,
) -> Result<Vec<u8>, FileError> {
    let script = format!(
        r#"[ -f "$1" ] || exit {MISSING_EXIT_CODE}; [ "$(stat -c %s -- "$1")" -le {MAX_FILE_BYTES} ] || exit {TOO_LARGE_EXIT_CODE}; cat -- "$1""#
    );
    let output = run(openshell, sandbox, &script, &[&absolute(path)?], Vec::new()).await?;
    Ok(output.stdout)
}

pub async fn write(
    openshell: &OpenShell,
    sandbox: &Sandbox,
    path: &str,
    content: Vec<u8>,
) -> Result<(), FileError> {
    let script = r#"mkdir -p -- "$(dirname -- "$1")" && cat > "$1""#;
    run(openshell, sandbox, script, &[&absolute(path)?], content).await?;
    Ok(())
}

pub async fn remove(openshell: &OpenShell, sandbox: &Sandbox, path: &str) -> Result<(), FileError> {
    let script = format!(r#"[ -f "$1" ] || exit {MISSING_EXIT_CODE}; rm -- "$1""#);
    run(openshell, sandbox, &script, &[&absolute(path)?], Vec::new()).await?;
    Ok(())
}

// never overwrites, like the library's move
pub async fn rename(
    openshell: &OpenShell,
    sandbox: &Sandbox,
    from: &str,
    to: &str,
) -> Result<(), FileError> {
    let script = format!(
        r#"[ -f "$1" ] || exit {MISSING_EXIT_CODE}; [ -e "$2" ] && exit {EXISTS_EXIT_CODE}; mkdir -p -- "$(dirname -- "$2")" && mv -- "$1" "$2""#
    );
    run(
        openshell,
        sandbox,
        &script,
        &[&absolute(from)?, &absolute(to)?],
        Vec::new(),
    )
    .await?;
    Ok(())
}

// paths go in as positional arguments so the shell never interprets them
async fn run(
    openshell: &OpenShell,
    sandbox: &Sandbox,
    script: &str,
    args: &[&str],
    stdin: Vec<u8>,
) -> Result<Output, FileError> {
    let mut argv = vec![
        "sh".to_owned(),
        "-c".to_owned(),
        script.to_owned(),
        "sh".to_owned(),
    ];
    argv.extend(args.iter().map(|arg| (*arg).to_owned()));
    let output = openshell.output(sandbox, argv, stdin).await?;
    match output.exit_code {
        Some(0) => Ok(output),
        Some(MISSING_EXIT_CODE) => Err(FileError::NotFound),
        Some(TOO_LARGE_EXIT_CODE) => Err(FileError::TooLarge),
        Some(EXISTS_EXIT_CODE) => Err(FileError::Exists),
        _ => Err(FileError::Failed(anyhow::anyhow!(
            "{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_stay_inside_the_home() {
        assert_eq!(absolute("src/app.tsx").unwrap(), "/sandbox/src/app.tsx");
        assert_eq!(absolute(".env").unwrap(), "/sandbox/.env");
        for bad in ["", "/etc/passwd", "../x", "a/../../x", "a//b", "./a", "a/"] {
            assert!(
                matches!(absolute(bad), Err(FileError::InvalidPath(_))),
                "{bad:?}"
            );
        }
    }

    // needs the openshell gateway tunnel, <workspace>/certs/openshell and the dev image
    #[tokio::test]
    #[ignore]
    async fn manages_files_in_the_sandbox() {
        let (openshell, user, sandbox) = crate::test_support::sandbox_for_new_user().await;
        let result = async {
            write(
                &openshell,
                &sandbox,
                "app/main.py",
                b"print('hi')\n".to_vec(),
            )
            .await?;
            write(
                &openshell,
                &sandbox,
                "node_modules/x/index.js",
                b"x".to_vec(),
            )
            .await?;
            write(&openshell, &sandbox, "notes.md", b"# notes".to_vec()).await?;
            let listed = list(&openshell, &sandbox).await?;
            let read_back = read(&openshell, &sandbox, "app/main.py").await?;
            rename(&openshell, &sandbox, "notes.md", "docs/notes.md").await?;
            let exists = rename(&openshell, &sandbox, "app/main.py", "docs/notes.md").await;
            remove(&openshell, &sandbox, "app/main.py").await?;
            let missing = read(&openshell, &sandbox, "app/main.py").await;
            let after = list(&openshell, &sandbox).await?;
            Ok::<_, FileError>((listed, read_back, exists, missing, after))
        }
        .await;
        openshell.delete_workspace(user).await.unwrap();
        let (listed, read_back, exists, missing, after) = result.unwrap();

        let paths: Vec<_> = listed.files.iter().map(|file| file.path.as_str()).collect();
        assert_eq!(paths, ["app/main.py", "notes.md"]);
        assert_eq!(listed.files[0].size, 12);
        assert!(listed.files[0].modified_at > 0);
        assert_eq!(read_back, b"print('hi')\n");
        assert!(matches!(exists, Err(FileError::Exists)));
        assert!(matches!(missing, Err(FileError::NotFound)));
        let paths: Vec<_> = after.files.iter().map(|file| file.path.as_str()).collect();
        assert_eq!(paths, ["docs/notes.md"]);
    }
}
