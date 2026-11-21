pub mod asset;
pub mod generation;
pub mod studio;
pub mod worker;

pub const LABEL_MANAGED: &str = "app.kubernetes.io/managed-by";
pub const LABEL_NAME: &str = "app.kubernetes.io/name";
pub const LABEL_INSTANCE: &str = "app.kubernetes.io/instance";
pub const LABEL_COMPONENT: &str = "app.kubernetes.io/component";
pub const MANAGED_BY: &str = "freya-operator";

pub fn default_api_image() -> (String, String) {
    (
        "harbor.dataknife.net/library/freya-api".into(),
        "0.1.0".into(),
    )
}

pub fn default_ui_image() -> (String, String) {
    (
        "harbor.dataknife.net/library/freya-web".into(),
        "0.1.0".into(),
    )
}

pub fn default_worker_image() -> (String, String) {
    (
        "harbor.dataknife.net/library/freya-diffusion-worker".into(),
        "0.1.0".into(),
    )
}
