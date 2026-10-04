use std::{fs, path::Path, time::Duration};

use anyhow::Context;
use futures::stream;
use openshell_sdk::{
    DeleteOptions, EdgeAuthInterceptor, OpenShellClient, SandboxSpec,
    raw::proto::{self, exec_sandbox_event::Payload, exec_sandbox_input},
};
use tonic::{
    Streaming,
    transport::{Certificate, ClientTlsConfig, Endpoint, Identity},
};

const WORKSPACE: &str = "default";
// the gateway rejects grpc messages over 1 MiB, so stdin is streamed in smaller chunks
const STDIN_CHUNK_BYTES: usize = 256 * 1024;

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

    pub async fn create(&self, image: Option<String>) -> anyhow::Result<String> {
        let spec = SandboxSpec {
            image,
            ..Default::default()
        };
        let sandbox = self.client.create_sandbox(spec).await?;
        self.client
            .wait_ready(&sandbox.name, Duration::from_secs(120))
            .await?;
        Ok(sandbox.name)
    }

    // uses the interactive rpc because dropping its stream kills the remote process,
    // and the end of the input stream closes stdin without ending output
    pub async fn exec(
        &self,
        sandbox: &str,
        command: Vec<String>,
        stdin: Vec<u8>,
    ) -> anyhow::Result<ExecStream> {
        let start = proto::ExecSandboxRequest {
            sandbox: sandbox.to_owned(),
            workspace_scope: Some(proto::workspace_selector(WORKSPACE)),
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
        sandbox: &str,
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

    pub async fn delete(&self, sandbox: &str) -> anyhow::Result<()> {
        self.client
            .delete_sandbox(
                sandbox,
                DeleteOptions {
                    allow_missing: true,
                },
            )
            .await?;
        Ok(())
    }
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

        let sandbox = openshell.create(None).await.unwrap();

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

        openshell.delete(&sandbox).await.unwrap();

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
}
