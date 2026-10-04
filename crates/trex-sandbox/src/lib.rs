use std::{collections::HashMap, fmt::Write, fs, path::Path, time::Duration};

use anyhow::{Context, bail};
use futures::stream;
use openshell_sdk::{
    DeleteOptions, EdgeAuthInterceptor, ListOptions, OpenShellClient, SandboxSpec, SdkError,
    raw::proto::{self, exec_sandbox_event::Payload, exec_sandbox_input},
};
use sha2::{Digest, Sha256};
use tonic::{
    Streaming,
    transport::{Certificate, ClientTlsConfig, Endpoint, Identity},
};
use uuid::Uuid;

// the gateway rejects grpc messages over 1 MiB, so stdin is streamed in smaller chunks
const STDIN_CHUNK_BYTES: usize = 256 * 1024;
const USER_LABEL: &str = "trex-user";
const READY_TIMEOUT: Duration = Duration::from_secs(120);
const DELETE_TIMEOUT: Duration = Duration::from_secs(60);

// openshell trusts trex's mtls identity as platform admin, so trex is what keeps users apart:
// every sandbox handle carries the workspace of the user it was created for
pub struct Sandbox {
    pub workspace: String,
    pub name: String,
}

pub struct OpenShell {
    client: OpenShellClient,
}

pub enum ExecEvent {
    Stdout(Vec<u8>),
    Stderr(Vec<u8>),
    Exit(i32),
}

pub struct ExecStream(Streaming<proto::ExecSandboxEvent>);

pub struct Output {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_code: Option<i32>,
}

impl OpenShell {
    // the sdk has no mtls support, so we build the channel ourselves
    pub async fn connect(endpoint: &str, tls_dir: &Path) -> anyhow::Result<Self> {
        let read = |file: &str| {
            let path = tls_dir.join(file);
            fs::read(&path).with_context(|| format!("failed to read {}", path.display()))
        };

        let host = endpoint
            .parse::<tonic::codegen::http::Uri>()
            .context("invalid openshell endpoint")?
            .host()
            .context("openshell endpoint has no host")?
            .to_owned();

        let tls = ClientTlsConfig::new()
            .ca_certificate(Certificate::from_pem(read("ca.crt")?))
            .identity(Identity::from_pem(read("tls.crt")?, read("tls.key")?))
            .domain_name(host);

        let channel = Endpoint::from_shared(endpoint.to_owned())?
            .connect_timeout(Duration::from_secs(10))
            .http2_keep_alive_interval(Duration::from_secs(10))
            .keep_alive_while_idle(true)
            .tls_config(tls)?
            .connect()
            .await
            .with_context(|| format!("failed to connect to openshell gateway at {endpoint}"))?;

        Ok(Self {
            client: OpenShellClient::from_parts(channel, EdgeAuthInterceptor::noop()),
        })
    }

    pub async fn version(&self) -> anyhow::Result<String> {
        Ok(self.client.health().await?.version)
    }

    pub async fn ensure_workspace(&self, user: Uuid) -> anyhow::Result<String> {
        let name = workspace_name(user);
        let workspace = match self.client.get_workspace(&name).await {
            Ok(workspace) => workspace,
            Err(SdkError::NotFound { .. }) => {
                let labels = HashMap::from([(USER_LABEL.to_owned(), user.to_string())]);
                match self.client.create_workspace(&name, labels).await {
                    Ok(workspace) => workspace,
                    Err(SdkError::AlreadyExists { .. }) => self.client.get_workspace(&name).await?,
                    Err(error) => return Err(error).context("failed to create workspace"),
                }
            }
            Err(error) => return Err(error).context("failed to get workspace"),
        };

        // names are a truncated hash, so a collision must never hand one user another's workspace
        if workspace.labels.get(USER_LABEL) != Some(&user.to_string()) {
            bail!("workspace {name} does not belong to user {user}");
        }
        Ok(name)
    }

    // openshell refuses to delete a workspace that still contains sandboxes
    pub async fn delete_workspace(&self, user: Uuid) -> anyhow::Result<()> {
        let name = workspace_name(user);
        let scoped = self.client.workspace(&name);
        let sandboxes = match scoped.list_all_sandboxes(ListOptions::default()).await {
            Ok(sandboxes) => sandboxes,
            Err(SdkError::NotFound { .. }) => return Ok(()),
            Err(error) => return Err(error).context("failed to list sandboxes"),
        };
        for sandbox in sandboxes {
            scoped
                .delete_sandbox(
                    &sandbox.name,
                    DeleteOptions {
                        allow_missing: true,
                    },
                )
                .await?;
            scoped
                .wait_deleted(&sandbox.name, DELETE_TIMEOUT, Some(&sandbox.id))
                .await?;
        }
        self.client
            .delete_workspace(
                &name,
                DeleteOptions {
                    allow_missing: true,
                },
            )
            .await?;
        Ok(())
    }

    pub async fn create(&self, workspace: &str, image: Option<String>) -> anyhow::Result<Sandbox> {
        let spec = SandboxSpec {
            image,
            ..Default::default()
        };
        let scoped = self.client.workspace(workspace);
        let sandbox = scoped.create_sandbox(spec).await?;
        scoped.wait_ready(&sandbox.name, READY_TIMEOUT).await?;
        Ok(Sandbox {
            workspace: workspace.to_owned(),
            name: sandbox.name,
        })
    }

    // uses the interactive rpc because dropping its stream kills the remote process,
    // and the end of the input stream closes stdin without ending output
    pub async fn exec(
        &self,
        sandbox: &Sandbox,
        command: Vec<String>,
        stdin: Vec<u8>,
    ) -> anyhow::Result<ExecStream> {
        let start = proto::ExecSandboxRequest {
            sandbox: sandbox.name.clone(),
            workspace_scope: Some(proto::workspace_selector(&sandbox.workspace)),
            command,
            no_login_shell: true,
            ..Default::default()
        };
        let mut input = vec![proto::ExecSandboxInput {
            payload: Some(exec_sandbox_input::Payload::Start(start)),
        }];
        input.extend(
            stdin
                .chunks(STDIN_CHUNK_BYTES)
                .map(|chunk| proto::ExecSandboxInput {
                    payload: Some(exec_sandbox_input::Payload::Stdin(chunk.to_vec())),
                }),
        );

        let stream = self
            .client
            .raw_grpc()
            .exec_sandbox_interactive(stream::iter(input))
            .await?
            .into_inner();
        Ok(ExecStream(stream))
    }

    pub async fn output(
        &self,
        sandbox: &Sandbox,
        command: Vec<String>,
        stdin: Vec<u8>,
    ) -> anyhow::Result<Output> {
        let mut stream = self.exec(sandbox, command, stdin).await?;
        let mut output = Output {
            stdout: Vec::new(),
            stderr: Vec::new(),
            exit_code: None,
        };
        while let Some(event) = stream.next().await? {
            match event {
                ExecEvent::Stdout(data) => output.stdout.extend(data),
                ExecEvent::Stderr(data) => output.stderr.extend(data),
                ExecEvent::Exit(code) => output.exit_code = Some(code),
            }
        }
        Ok(output)
    }

    pub async fn delete(&self, sandbox: &Sandbox) -> anyhow::Result<()> {
        self.client
            .workspace(&sandbox.workspace)
            .delete_sandbox(
                &sandbox.name,
                DeleteOptions {
                    allow_missing: true,
                },
            )
            .await?;
        Ok(())
    }
}

// workspace names are dns labels of at most 19 chars, too short for a uuid, so use 68 bits of its hash
pub fn workspace_name(user: Uuid) -> String {
    let digest = Sha256::digest(user.as_bytes());
    let mut name = String::from("u-");
    for byte in &digest[..9] {
        write!(name, "{byte:02x}").expect("writing to a string cannot fail");
    }
    name.truncate(19);
    name
}

impl ExecStream {
    pub async fn next(&mut self) -> anyhow::Result<Option<ExecEvent>> {
        while let Some(event) = self.0.message().await? {
            match event.payload {
                Some(Payload::Stdout(chunk)) => return Ok(Some(ExecEvent::Stdout(chunk.data))),
                Some(Payload::Stderr(chunk)) => return Ok(Some(ExecEvent::Stderr(chunk.data))),
                Some(Payload::Exit(exit)) => return Ok(Some(ExecEvent::Exit(exit.exit_code))),
                None => {}
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // needs the gateway tunnel on 127.0.0.1:17670 and certs in <workspace>/certs/openshell: cargo test -- --ignored
    #[tokio::test]
    #[ignore]
    async fn sandbox_lifecycle() {
        let tls_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../certs/openshell");
        let openshell = OpenShell::connect("https://127.0.0.1:17670", &tls_dir)
            .await
            .unwrap();

        let user = Uuid::now_v7();
        let workspace = openshell.ensure_workspace(user).await.unwrap();
        let sandbox = openshell.create(&workspace, None).await.unwrap();

        let command = ["sh", "-c", "echo hello && echo oops >&2 && exit 3"];
        let mut stream = openshell
            .exec(&sandbox, command.map(String::from).to_vec(), Vec::new())
            .await
            .unwrap();

        let (mut stdout, mut stderr, mut exit) = (Vec::new(), Vec::new(), None);
        while let Some(event) = stream.next().await.unwrap() {
            match event {
                ExecEvent::Stdout(data) => stdout.extend(data),
                ExecEvent::Stderr(data) => stderr.extend(data),
                ExecEvent::Exit(code) => exit = Some(code),
            }
        }

        let echoed = openshell
            .output(&sandbox, vec!["cat".into()], b"from stdin".to_vec())
            .await
            .unwrap();

        let script = "echo started; sleep 3; touch /tmp/survived";
        let mut cancelled = openshell
            .exec(
                &sandbox,
                ["sh", "-c", script].map(String::from).to_vec(),
                Vec::new(),
            )
            .await
            .unwrap();
        let first = cancelled.next().await.unwrap();
        drop(cancelled);
        tokio::time::sleep(Duration::from_secs(5)).await;
        let survived = openshell
            .output(
                &sandbox,
                ["test", "-e", "/tmp/survived"].map(String::from).to_vec(),
                Vec::new(),
            )
            .await
            .unwrap();

        openshell.delete_workspace(user).await.unwrap();

        assert!(matches!(first, Some(ExecEvent::Stdout(data)) if data == b"started\n"));
        assert_eq!(
            survived.exit_code,
            Some(1),
            "dropping the exec stream must kill the process"
        );
        assert_eq!(stdout, b"hello\n");
        assert_eq!(stderr, b"oops\n");
        assert_eq!(exit, Some(3));
        assert_eq!(echoed.stdout, b"from stdin");
        assert_eq!(echoed.exit_code, Some(0));
    }

    #[test]
    fn workspace_names_fit_openshell_limits() {
        let user = Uuid::now_v7();
        let name = workspace_name(user);
        assert_eq!(name.len(), 19);
        assert!(name.starts_with("u-"));
        assert!(
            name[2..]
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        );
        assert_eq!(workspace_name(user), name);
        assert_ne!(workspace_name(Uuid::now_v7()), name);
    }

    // needs the gateway tunnel on 127.0.0.1:17670 and certs in <workspace>/certs/openshell: cargo test -- --ignored
    #[tokio::test]
    #[ignore]
    async fn users_cannot_reach_each_others_sandboxes() {
        let tls_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../certs/openshell");
        let openshell = OpenShell::connect("https://127.0.0.1:17670", &tls_dir)
            .await
            .unwrap();
        let (alice, bob) = (Uuid::now_v7(), Uuid::now_v7());
        let alice_ws = openshell.ensure_workspace(alice).await.unwrap();
        let bob_ws = openshell.ensure_workspace(bob).await.unwrap();
        let again = openshell.ensure_workspace(alice).await.unwrap();
        let sandbox = openshell.create(&alice_ws, None).await.unwrap();

        let forged = Sandbox {
            workspace: bob_ws.clone(),
            name: sandbox.name.clone(),
        };
        let from_bob = openshell
            .output(&forged, vec!["true".into()], Vec::new())
            .await;
        let from_alice = openshell
            .output(&sandbox, vec!["true".into()], Vec::new())
            .await
            .unwrap();
        let deleted_by_bob = openshell.delete(&forged).await;
        let still_there = openshell
            .output(&sandbox, vec!["true".into()], Vec::new())
            .await
            .unwrap();

        openshell.delete_workspace(alice).await.unwrap();
        openshell.delete_workspace(bob).await.unwrap();
        let recreated = openshell.ensure_workspace(alice).await;
        openshell.delete_workspace(alice).await.unwrap();

        assert_eq!(again, alice_ws);
        assert_ne!(alice_ws, bob_ws);
        assert!(from_bob.is_err(), "bob must not exec in alice's sandbox");
        assert_eq!(from_alice.exit_code, Some(0));
        assert!(
            deleted_by_bob.is_ok(),
            "deleting a missing sandbox is allowed"
        );
        assert_eq!(
            still_there.exit_code,
            Some(0),
            "bob's delete must not touch alice's sandbox"
        );
        assert!(recreated.is_ok());
    }
}
