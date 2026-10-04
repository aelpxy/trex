use std::{fs, path::Path, time::Duration};

use anyhow::Context;
use openshell_sdk::{
    DeleteOptions, EdgeAuthInterceptor, OpenShellClient, SandboxSpec,
    raw::proto::{self, exec_sandbox_event::Payload},
};
use tonic::{
    Streaming,
    transport::{Certificate, ClientTlsConfig, Endpoint, Identity},
};

const WORKSPACE: &str = "default";

pub struct OpenShell {
    client: OpenShellClient,
}

pub enum ExecEvent {
    Stdout(Vec<u8>),
    Stderr(Vec<u8>),
    Exit(i32),
}

pub struct ExecStream(Streaming<proto::ExecSandboxEvent>);

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

    pub async fn exec(
        &self,
        sandbox: &str,
        command: Vec<String>,
        workdir: Option<String>,
    ) -> anyhow::Result<ExecStream> {
        let request = proto::ExecSandboxRequest {
            sandbox: sandbox.to_owned(),
            workspace_scope: Some(proto::workspace_selector(WORKSPACE)),
            command,
            workdir: workdir.unwrap_or_default(),
            no_login_shell: true,
            ..Default::default()
        };
        let stream = self
            .client
            .raw_grpc()
            .exec_sandbox(request)
            .await?
            .into_inner();
        Ok(ExecStream(stream))
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
            .exec(&sandbox, command.map(String::from).to_vec(), None)
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

        openshell.delete(&sandbox).await.unwrap();

        assert_eq!(stdout, b"hello\n");
        assert_eq!(stderr, b"oops\n");
        assert_eq!(exit, Some(3));
    }
}
