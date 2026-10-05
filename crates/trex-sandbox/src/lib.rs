use std::{
    collections::HashMap,
    fmt::Write,
    fs,
    path::Path,
    time::{Duration, Instant},
};

use anyhow::{Context, bail};
use futures::stream;
use openshell_sdk::{
    DeleteOptions, EdgeAuthInterceptor, ListOptions, OpenShellClient, SandboxPhase, SdkError,
    raw::proto::{
        self, exec_sandbox_event::Payload, exec_sandbox_input, tcp_forward_frame, tcp_forward_init,
    },
};
use sha2::{Digest, Sha256};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    sync::mpsc,
};
use tonic::{
    Streaming,
    transport::{Certificate, ClientTlsConfig, Endpoint, Identity},
};
use uuid::Uuid;

// the gateway rejects grpc messages over 1 MiB, so stdin is streamed in smaller chunks
const STDIN_CHUNK_BYTES: usize = 256 * 1024;
// the trex workspace that owns an openshell workspace; older ones carry the label they had when
// trex was keyed by user, with the same uuid
const OWNER_LABEL: &str = "trex-workspace";
const LEGACY_OWNER_LABEL: &str = "trex-user";
const READY_TIMEOUT: Duration = Duration::from_secs(120);
const DELETE_TIMEOUT: Duration = Duration::from_secs(60);
const PHASE_POLL_INTERVAL: Duration = Duration::from_millis(500);
const POLICY_LOAD_TIMEOUT: Duration = Duration::from_secs(30);
const FORWARD_BUFFER_FRAMES: usize = 16;
#[cfg(test)]
const DEV_IMAGE: &str = "localhost/trex-sandbox:latest";

// openshell trusts trex's mtls identity as platform admin, so trex is what keeps tenants apart:
// every sandbox handle carries the openshell workspace of the trex workspace it was created for
pub struct Sandbox {
    pub workspace: String,
    pub name: String,
}

pub struct Policy(proto::SandboxPolicy);

// a network request the sandbox was denied, proposed by openshell as a rule a user can approve
pub struct AccessRequest {
    pub id: String,
    pub review_token: String,
    pub rule_name: String,
    pub endpoints: Vec<String>,
    pub binary: String,
    pub rationale: String,
    pub security_notes: String,
    pub hit_count: i32,
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

    pub async fn ensure_workspace(&self, owner: Uuid) -> anyhow::Result<String> {
        let name = workspace_name(owner);
        let workspace = match self.client.get_workspace(&name).await {
            Ok(workspace) => workspace,
            Err(SdkError::NotFound { .. }) => {
                let labels = HashMap::from([(OWNER_LABEL.to_owned(), owner.to_string())]);
                match self.client.create_workspace(&name, labels).await {
                    Ok(workspace) => workspace,
                    Err(SdkError::AlreadyExists { .. }) => self.client.get_workspace(&name).await?,
                    Err(error) => return Err(error).context("failed to create workspace"),
                }
            }
            Err(error) => return Err(error).context("failed to get workspace"),
        };

        // names are a truncated hash, so a collision must never hand one tenant another's workspace
        let labelled = workspace
            .labels
            .get(OWNER_LABEL)
            .or_else(|| workspace.labels.get(LEGACY_OWNER_LABEL));
        if labelled != Some(&owner.to_string()) {
            bail!("openshell workspace {name} does not belong to workspace {owner}");
        }
        Ok(name)
    }

    // openshell refuses to delete a workspace that still contains sandboxes
    pub async fn delete_workspace(&self, owner: Uuid) -> anyhow::Result<()> {
        let name = workspace_name(owner);
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

    // the sdk's sandbox spec has no policy field, so this uses the raw create rpc
    pub async fn create(
        &self,
        workspace: &str,
        image: Option<String>,
        policy: Option<&Policy>,
    ) -> anyhow::Result<Sandbox> {
        let request = proto::CreateSandboxRequest {
            workspace_scope: Some(proto::workspace_selector(workspace)),
            spec: Some(proto::SandboxSpec {
                template: image.map(|image| proto::SandboxTemplate {
                    image,
                    ..Default::default()
                }),
                policy: policy.map(|policy| policy.0.clone()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let response = self
            .client
            .raw_grpc()
            .create_sandbox(request)
            .await
            .context("failed to create sandbox")?
            .into_inner();
        let name = response
            .sandbox
            .and_then(|sandbox| sandbox.metadata)
            .map(|metadata| metadata.name)
            .context("gateway returned no sandbox")?;

        self.client
            .workspace(workspace)
            .wait_ready(&name, READY_TIMEOUT)
            .await?;
        Ok(Sandbox {
            workspace: workspace.to_owned(),
            name,
        })
    }

    // openshell has no push notification for new drafts, so callers poll this
    pub async fn pending_access(&self, sandbox: &Sandbox) -> anyhow::Result<Vec<AccessRequest>> {
        let request = proto::GetDraftPolicyRequest {
            workspace_scope: Some(proto::workspace_selector(&sandbox.workspace)),
            sandbox: sandbox.name.clone(),
            status_filter: "pending".into(),
        };
        let response = self
            .client
            .raw_grpc()
            .get_draft_policy(request)
            .await
            .context("failed to get access requests")?
            .into_inner();

        let requests = response
            .chunks
            .into_iter()
            .map(|chunk| AccessRequest {
                endpoints: chunk
                    .proposed_rule
                    .map(|rule| {
                        rule.endpoints
                            .iter()
                            .map(|endpoint| {
                                format!("{}:{}", endpoint.host, endpoint_ports(endpoint))
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
                id: chunk.id,
                review_token: chunk.review_token,
                rule_name: chunk.rule_name,
                binary: chunk.binary,
                rationale: chunk.rationale,
                security_notes: chunk.security_notes,
                hit_count: chunk.hit_count,
            })
            .collect();
        Ok(requests)
    }

    // the review token pins approval to the exact proposal the user saw
    pub async fn approve_access(
        &self,
        sandbox: &Sandbox,
        request: &AccessRequest,
    ) -> anyhow::Result<()> {
        let approve = proto::ApproveDraftChunkRequest {
            workspace_scope: Some(proto::workspace_selector(&sandbox.workspace)),
            sandbox: sandbox.name.clone(),
            chunk_id: request.id.clone(),
            review_token: request.review_token.clone(),
            ..Default::default()
        };
        let version = self
            .client
            .raw_grpc()
            .approve_draft_chunk(approve)
            .await
            .context("failed to approve access request")?
            .into_inner()
            .policy_version;
        self.wait_policy_loaded(sandbox, version).await
    }

    // the sandbox loads an approved policy asynchronously, so a retry right after approving can
    // still be denied unless we wait for it
    async fn wait_policy_loaded(&self, sandbox: &Sandbox, version: u32) -> anyhow::Result<()> {
        let deadline = Instant::now() + POLICY_LOAD_TIMEOUT;
        loop {
            let request = proto::GetSandboxPolicyStatusRequest {
                workspace_scope: Some(proto::workspace_selector(&sandbox.workspace)),
                sandbox: sandbox.name.clone(),
                version,
                ..Default::default()
            };
            let status = self
                .client
                .raw_grpc()
                .get_sandbox_policy_status(request)
                .await
                .context("failed to check the sandbox policy")?
                .into_inner();
            if status.active_version >= version {
                return Ok(());
            }
            if let Some(revision) = status.revision
                && revision.status == proto::PolicyStatus::Failed as i32
            {
                bail!(
                    "the sandbox rejected the approved policy: {}",
                    revision.load_error
                );
            }
            // a stopped sandbox loads the policy when it starts again, so the approval still stands
            if Instant::now() > deadline {
                return Ok(());
            }
            tokio::time::sleep(PHASE_POLL_INTERVAL).await;
        }
    }

    pub async fn reject_access(
        &self,
        sandbox: &Sandbox,
        request: &AccessRequest,
        reason: &str,
    ) -> anyhow::Result<()> {
        let reject = proto::RejectDraftChunkRequest {
            workspace_scope: Some(proto::workspace_selector(&sandbox.workspace)),
            sandbox: sandbox.name.clone(),
            chunk_id: request.id.clone(),
            reason: reason.to_owned(),
            ..Default::default()
        };
        self.client
            .raw_grpc()
            .reject_draft_chunk(reject)
            .await
            .context("failed to reject access request")?;
        Ok(())
    }

    // uses the interactive rpc because dropping its stream kills the remote process,
    // and the end of the input stream closes stdin without ending output
    pub async fn exec(
        &self,
        sandbox: &Sandbox,
        command: Vec<String>,
        stdin: Vec<u8>,
    ) -> anyhow::Result<ExecStream> {
        match self.exec_once(sandbox, command.clone(), &stdin).await {
            Err(status)
                if status.code() == tonic::Code::FailedPrecondition
                    && status.message().contains("not ready") =>
            {
                // the sandbox can leave Ready mid-run, e.g. stopped while idle or briefly restarting
                self.start(sandbox).await?;
                Ok(self.exec_once(sandbox, command, &stdin).await?)
            }
            result => Ok(result?),
        }
    }

    async fn exec_once(
        &self,
        sandbox: &Sandbox,
        command: Vec<String>,
        stdin: &[u8],
    ) -> Result<ExecStream, tonic::Status> {
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

    // splices a client connection into a loopback port in the sandbox; `initial` is what was
    // already read from the client. returns once the sandbox side closes
    pub async fn forward<S>(
        &self,
        sandbox: &Sandbox,
        port: u16,
        initial: Vec<u8>,
        client: S,
    ) -> anyhow::Result<()>
    where
        S: AsyncRead + AsyncWrite + Send + 'static,
    {
        // the gateway allows only a few connections per token, so each one gets its own
        let token = self
            .client
            .raw_grpc()
            .create_ssh_session(proto::CreateSshSessionRequest {
                sandbox: sandbox.name.clone(),
                workspace_scope: Some(proto::workspace_selector(&sandbox.workspace)),
            })
            .await
            .context("failed to authorize a tunnel into the sandbox")?
            .into_inner()
            .token;
        let result = self
            .forward_with(sandbox, port, &token, initial, client)
            .await;
        let revoked = self
            .client
            .raw_grpc()
            .revoke_ssh_session(proto::RevokeSshSessionRequest {
                token,
                allow_missing: true,
            })
            .await;
        if let Err(error) = revoked {
            tracing::warn!(sandbox = sandbox.name, %error, "failed to revoke a tunnel token");
        }
        result
    }

    async fn forward_with<S>(
        &self,
        sandbox: &Sandbox,
        port: u16,
        token: &str,
        initial: Vec<u8>,
        client: S,
    ) -> anyhow::Result<()>
    where
        S: AsyncRead + AsyncWrite + Send + 'static,
    {
        let (tx, rx) = mpsc::channel::<proto::TcpForwardFrame>(FORWARD_BUFFER_FRAMES);
        let init = proto::TcpForwardInit {
            sandbox: sandbox.name.clone(),
            workspace: sandbox.workspace.clone(),
            authorization_token: token.to_owned(),
            // ipv4 loopback inside a sandbox goes through openshell's network proxy, which resets
            // these connections, so servers are reached over ipv6
            target: Some(tcp_forward_init::Target::Tcp(proto::TcpRelayTarget {
                host: "::1".into(),
                port: u32::from(port),
            })),
            ..Default::default()
        };
        let data = |bytes: Vec<u8>| proto::TcpForwardFrame {
            payload: Some(tcp_forward_frame::Payload::Data(bytes)),
        };
        tx.send(proto::TcpForwardFrame {
            payload: Some(tcp_forward_frame::Payload::Init(init)),
        })
        .await
        .context("forward stream closed")?;
        if !initial.is_empty() {
            tx.send(data(initial))
                .await
                .context("forward stream closed")?;
        }
        let frames = stream::unfold(rx, |mut rx| async {
            rx.recv().await.map(|frame| (frame, rx))
        });
        let mut response = self
            .client
            .raw_grpc()
            .forward_tcp(frames)
            .await
            .context("failed to open a tunnel into the sandbox")?
            .into_inner();

        let (mut read, mut write) = tokio::io::split(client);
        let upload = tokio::spawn(async move {
            let mut buffer = vec![0; STDIN_CHUNK_BYTES];
            loop {
                match read.read(&mut buffer).await {
                    Ok(0) | Err(_) => break,
                    Ok(read) => {
                        if tx.send(data(buffer[..read].to_vec())).await.is_err() {
                            break;
                        }
                    }
                }
            }
        });
        let result = async {
            while let Some(frame) = response.message().await.context("tunnel failed")? {
                if let Some(tcp_forward_frame::Payload::Data(bytes)) = frame.payload {
                    write.write_all(&bytes).await.context("client went away")?;
                }
            }
            write.shutdown().await.context("client went away")
        }
        .await;
        upload.abort();
        result
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

    // frees the sandbox's compute; its files survive until it is started again
    pub async fn stop(&self, sandbox: &Sandbox) -> anyhow::Result<()> {
        self.client
            .workspace(&sandbox.workspace)
            .stop_sandbox(&sandbox.name)
            .await
            .context("failed to stop sandbox")?;
        Ok(())
    }

    pub async fn health(&self, sandbox: &Sandbox) -> anyhow::Result<SandboxHealth> {
        match self
            .client
            .workspace(&sandbox.workspace)
            .get_sandbox(&sandbox.name)
            .await
        {
            Ok(found) if found.phase == SandboxPhase::Error => Ok(SandboxHealth::Broken),
            Ok(_) => Ok(SandboxHealth::Usable),
            Err(SdkError::NotFound { .. }) => Ok(SandboxHealth::Missing),
            Err(error) => Err(error).context("failed to look up sandbox"),
        }
    }

    // brings a stopped sandbox back and waits until it runs commands; a running one is left alone
    pub async fn start(&self, sandbox: &Sandbox) -> anyhow::Result<()> {
        let client = self.client.workspace(&sandbox.workspace);
        let deadline = Instant::now() + READY_TIMEOUT;
        loop {
            let phase = client
                .get_sandbox(&sandbox.name)
                .await
                .context("failed to look up sandbox")?
                .phase;
            match phase {
                SandboxPhase::Ready => return Ok(()),
                SandboxPhase::Stopped => {
                    client
                        .start_sandbox(&sandbox.name)
                        .await
                        .context("failed to start sandbox")?;
                    break;
                }
                SandboxPhase::Provisioning | SandboxPhase::Starting => break,
                // a stop still in progress has to finish before the sandbox can start again
                SandboxPhase::Stopping if Instant::now() < deadline => {
                    tokio::time::sleep(PHASE_POLL_INTERVAL).await;
                }
                phase => bail!("sandbox cannot be started while {phase:?}"),
            }
        }
        client.wait_ready(&sandbox.name, READY_TIMEOUT).await?;
        Ok(())
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxHealth {
    Usable,
    // in the error phase, which the gateway can neither stop nor start
    Broken,
    Missing,
}

impl Policy {
    pub fn from_yaml(yaml: &str) -> anyhow::Result<Self> {
        openshell_policy::parse_sandbox_policy(yaml)
            .map(Self)
            .map_err(|error| anyhow::anyhow!("{error}"))
    }
}

fn endpoint_ports(endpoint: &proto::NetworkEndpoint) -> String {
    let ports: Vec<_> = if endpoint.ports.is_empty() {
        vec![endpoint.port]
    } else {
        endpoint.ports.clone()
    };
    ports
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

// workspace names are dns labels of at most 19 chars, too short for a uuid, so use 68 bits of its hash
pub fn workspace_name(owner: Uuid) -> String {
    let digest = Sha256::digest(owner.as_bytes());
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
        let sandbox = openshell.create(&workspace, None, None).await.unwrap();

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
        let sandbox = openshell.create(&alice_ws, None, None).await.unwrap();

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

    #[test]
    fn default_policy_parses() {
        let yaml = include_str!("../../../sandbox-policy.yaml");
        let policy = Policy::from_yaml(yaml).unwrap();
        assert!(policy.0.network_policies.contains_key("package_registries"));
        assert!(Policy::from_yaml("version: 1\nbogus: true\n").is_err());
    }

    // needs the gateway tunnel on 127.0.0.1:17670, certs in <workspace>/certs/openshell, and internet on the gateway host
    #[tokio::test]
    #[ignore]
    async fn denied_network_access_can_be_approved() {
        let tls_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../certs/openshell");
        let openshell = OpenShell::connect("https://127.0.0.1:17670", &tls_dir)
            .await
            .unwrap();
        let policy = Policy::from_yaml(include_str!("../../../sandbox-policy.yaml")).unwrap();
        let user = Uuid::now_v7();
        let workspace = openshell.ensure_workspace(user).await.unwrap();
        let sandbox = openshell
            .create(&workspace, None, Some(&policy))
            .await
            .unwrap();

        // the default image has no curl, so open a tcp connection with bash itself
        let connect = |host: &str| {
            let script = format!("timeout 15 bash -c 'exec 3<>/dev/tcp/{host}/443'");
            ["bash", "-c", &script].map(String::from).to_vec()
        };
        let allowed = openshell
            .output(&sandbox, connect("pypi.org"), Vec::new())
            .await
            .unwrap();
        let denied = openshell
            .output(&sandbox, connect("example.com"), Vec::new())
            .await
            .unwrap();

        // the supervisor batches denials into proposals roughly every 10 seconds
        let mut request = None;
        for _ in 0..30 {
            let pending = openshell.pending_access(&sandbox).await.unwrap();
            request = pending
                .into_iter()
                .find(|r| r.endpoints.iter().any(|e| e.starts_with("example.com:")));
            if request.is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        let request = request.expect("denied access should become a pending request");
        openshell.approve_access(&sandbox, &request).await.unwrap();

        let mut approved = None;
        for _ in 0..15 {
            let output = openshell
                .output(&sandbox, connect("example.com"), Vec::new())
                .await
                .unwrap();
            approved = output.exit_code;
            if approved == Some(0) {
                break;
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        let still_pending = openshell.pending_access(&sandbox).await.unwrap();

        openshell.delete_workspace(user).await.unwrap();

        assert_eq!(allowed.exit_code, Some(0), "allowlisted host must connect");
        assert_ne!(denied.exit_code, Some(0), "unlisted host must be denied");
        assert!(!request.review_token.is_empty());
        assert_eq!(approved, Some(0), "approved host must connect");
        assert!(still_pending.iter().all(|r| r.id != request.id));
    }

    // needs the openshell gateway tunnel, <workspace>/certs/openshell, and the dev image
    #[tokio::test]
    #[ignore]
    async fn forwards_connections_to_sandbox_ports() {
        let tls_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../certs/openshell");
        let openshell = OpenShell::connect("https://127.0.0.1:17670", &tls_dir)
            .await
            .unwrap();
        let user = Uuid::now_v7();
        let workspace = openshell.ensure_workspace(user).await.unwrap();
        let sandbox = openshell
            .create(&workspace, Some(DEV_IMAGE.into()), None)
            .await
            .unwrap();
        let script = "echo hello-from-the-sandbox > /sandbox/index.html && cd /sandbox && \\
                      (setsid python3 -m http.server 8000 --bind :: >/tmp/server.log 2>&1 < /dev/null &) ; \\
                      for i in $(seq 50); do curl -s localhost:8000 >/dev/null && break; sleep 0.2; done";
        let started = openshell
            .output(
                &sandbox,
                ["bash", "-c", script].map(String::from).to_vec(),
                Vec::new(),
            )
            .await;

        let (client, mut ours) = tokio::io::duplex(64 * 1024);
        let request = b"GET /index.html HTTP/1.0\r\nHost: localhost\r\n\r\n".to_vec();
        let mut response = Vec::new();
        let (forwarded, read) = tokio::time::timeout(Duration::from_secs(60), async {
            tokio::join!(
                openshell.forward(&sandbox, 8000, request, client),
                ours.read_to_end(&mut response)
            )
        })
        .await
        .expect("the forward finishes once the server closes");
        openshell.delete_workspace(user).await.unwrap();

        started.unwrap();
        forwarded.unwrap();
        read.unwrap();
        let response = String::from_utf8_lossy(&response);
        assert!(response.starts_with("HTTP/1.0 200"), "{response}");
        assert!(response.contains("hello-from-the-sandbox"), "{response}");
    }

    // needs the gateway tunnel, certs, internet on the gateway host, and the image from images/sandbox built on it
    #[tokio::test]
    #[ignore]
    async fn stopped_sandboxes_keep_their_files() {
        let tls_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../certs/openshell");
        let openshell = OpenShell::connect("https://127.0.0.1:17670", &tls_dir)
            .await
            .unwrap();
        let policy = Policy::from_yaml(include_str!("../../../sandbox-policy.yaml")).unwrap();
        let user = Uuid::now_v7();
        let workspace = openshell.ensure_workspace(user).await.unwrap();
        let sandbox = openshell
            .create(&workspace, Some(DEV_IMAGE.into()), Some(&policy))
            .await
            .unwrap();
        let run = |script: &str| {
            openshell.output(
                &sandbox,
                ["bash", "-c", script].map(String::from).to_vec(),
                Vec::new(),
            )
        };

        let written = run("mkdir -p /sandbox/project && echo kept > /sandbox/project/note.txt && pip install --quiet six && echo ok").await.unwrap();
        openshell.start(&sandbox).await.unwrap();
        openshell.stop(&sandbox).await.unwrap();
        // an exec on a stopped sandbox starts it first
        let started = Instant::now();
        let while_stopped = run("echo up").await.unwrap();
        let start_time = started.elapsed();
        let read = run("cat /sandbox/project/note.txt && python3 -c 'import six; print(\"six\")'")
            .await
            .unwrap();
        eprintln!("start took {start_time:?}");
        let health_while_running = openshell.health(&sandbox).await.unwrap();

        openshell.delete_workspace(user).await.unwrap();
        assert_eq!(health_while_running, SandboxHealth::Usable);
        assert_eq!(
            openshell.health(&sandbox).await.unwrap(),
            SandboxHealth::Missing,
            "a deleted sandbox is missing"
        );

        assert_eq!(String::from_utf8_lossy(&written.stdout).trim(), "ok");
        assert_eq!(String::from_utf8_lossy(&while_stopped.stdout), "up\n");
        assert_eq!(
            String::from_utf8_lossy(&read.stdout),
            "kept\nsix\n",
            "{}",
            String::from_utf8_lossy(&read.stderr)
        );
    }

    #[tokio::test]
    #[ignore]
    async fn dev_image_tools_work_under_policy() {
        let tls_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../certs/openshell");
        let openshell = OpenShell::connect("https://127.0.0.1:17670", &tls_dir)
            .await
            .unwrap();
        let policy = Policy::from_yaml(include_str!("../../../sandbox-policy.yaml")).unwrap();
        let user = Uuid::now_v7();
        let workspace = openshell.ensure_workspace(user).await.unwrap();
        let sandbox = openshell
            .create(&workspace, Some(DEV_IMAGE.into()), Some(&policy))
            .await
            .unwrap();

        let script = r#"
            set -u
            for tool in git curl python3 pip uv node npm go cargo rustc micromamba rg jq cmake sqlite3; do
                command -v "$tool" >/dev/null && echo "tool:$tool" || echo "missing:$tool"
            done
            cd /tmp && mkdir work && cd work
            pip install --quiet six && python3 -c "import six" && echo "ok:pip"
            uv venv --quiet venv && uv pip install --quiet --python venv six && echo "ok:uv"
            npm install --silent --no-audit --no-fund is-number >/dev/null && node -e "require('is-number')" && echo "ok:npm"
            go mod init example.com/probe >/dev/null 2>&1 && go get github.com/google/uuid >/dev/null 2>&1 && echo "ok:go"
            cargo new --quiet probe && cd probe && cargo add --quiet itoa && cargo fetch --quiet && echo "ok:cargo" && cd ..
            micromamba install --quiet -y -n base -c conda-forge yq >/dev/null && yq --version >/dev/null && echo "ok:micromamba"
            git ls-remote --heads https://github.com/octocat/Hello-World >/dev/null && echo "ok:git"
            curl -s -o /dev/null --max-time 15 https://example.com && echo "reached:example.com" || echo "blocked:example.com"
        "#;
        let output = openshell
            .output(
                &sandbox,
                ["bash", "-c", script].map(String::from).to_vec(),
                Vec::new(),
            )
            .await
            .unwrap();

        openshell.delete_workspace(user).await.unwrap();

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let expected = [
            "ok:pip",
            "ok:uv",
            "ok:npm",
            "ok:go",
            "ok:cargo",
            "ok:micromamba",
            "ok:git",
            "blocked:example.com",
        ];
        assert!(!stdout.contains("missing:"), "missing tools:\n{stdout}");
        for marker in expected {
            assert!(
                stdout.contains(marker),
                "{marker} not found\nstdout:\n{stdout}\nstderr:\n{stderr}"
            );
        }
    }
}
