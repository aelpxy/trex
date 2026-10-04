pub mod agent;
pub mod event;
pub mod history;
pub mod library;
pub mod model;
pub mod question;
pub mod tool;

#[cfg(test)]
mod test_support {
    use std::path::Path;

    use trex_sandbox::{OpenShell, Policy, Sandbox};
    use uuid::Uuid;

    // built from images/sandbox on the gateway host
    const DEV_IMAGE: &str = "localhost/trex-sandbox:latest";

    // a fresh user per test keeps live tests isolated; callers delete the workspace when done.
    // sandboxes match production: the dev image under the default network policy
    pub async fn sandbox_for_new_user() -> (OpenShell, Uuid, Sandbox) {
        let tls_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../certs/openshell");
        let openshell = OpenShell::connect("https://127.0.0.1:17670", &tls_dir)
            .await
            .unwrap();
        let user = Uuid::now_v7();
        let workspace = openshell.ensure_workspace(user).await.unwrap();
        let policy = Policy::from_yaml(include_str!("../../../sandbox-policy.yaml")).unwrap();
        let sandbox = openshell
            .create(&workspace, Some(DEV_IMAGE.into()), Some(&policy))
            .await
            .unwrap();
        (openshell, user, sandbox)
    }
}
