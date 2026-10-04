pub mod agent;
pub mod event;
pub mod library;
pub mod model;
pub mod tool;

#[cfg(test)]
mod test_support {
    use std::path::Path;

    use trex_sandbox::{OpenShell, Sandbox};
    use uuid::Uuid;

    // a fresh user per test keeps live tests isolated; callers delete the workspace when done
    pub async fn sandbox_for_new_user() -> (OpenShell, Uuid, Sandbox) {
        let tls_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../certs/openshell");
        let openshell = OpenShell::connect("https://127.0.0.1:17670", &tls_dir)
            .await
            .unwrap();
        let user = Uuid::now_v7();
        let workspace = openshell.ensure_workspace(user).await.unwrap();
        let sandbox = openshell.create(&workspace, None).await.unwrap();
        (openshell, user, sandbox)
    }
}
