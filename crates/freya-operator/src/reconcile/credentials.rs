//! Game-operator style admin + API token Secret bootstrap for FreyaStudio.
//!
//! Mirrors satisfactory/dragonwilds/palworld: `generateSecrets` fills missing
//! `admin-password` (and username) without overwriting existing non-empty keys.
//! Also ensures a machine API bearer Secret when `apiTokenSecretRef` is unset.

use freya_crd::{
    FreyaStudio, LocalObjectRef, SecretKeyRef, SECRET_KEY_ADMIN_PASSWORD, SECRET_KEY_ADMIN_USERNAME,
    SECRET_KEY_API_TOKEN,
};
use k8s_openapi::api::core::v1::Secret;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{ObjectMeta, OwnerReference};
use kube::api::{Patch, PatchParams};
use kube::{Api, ResourceExt};
use std::collections::BTreeMap;
use tracing::info;

use super::studio::Error;
use super::{LABEL_COMPONENT, LABEL_INSTANCE, LABEL_MANAGED, LABEL_NAME, MANAGED_BY};
use crate::Ctx;

/// Resolved credential wiring for API/MCP Deployments + status.
#[derive(Clone, Debug)]
pub struct StudioCredentials {
    pub admin_secret_name: Option<String>,
    pub admin_generated: bool,
    pub api_token: Option<SecretKeyRef>,
    pub api_token_generated: bool,
}

pub async fn ensure_studio_credentials(
    ctx: &Ctx,
    ns: &str,
    studio: &FreyaStudio,
    owner: &OwnerReference,
) -> Result<StudioCredentials, Error> {
    let name = studio.name_any();
    let labels = BTreeMap::from([
        (LABEL_MANAGED.to_string(), MANAGED_BY.to_string()),
        (LABEL_NAME.to_string(), "freya".to_string()),
        (LABEL_INSTANCE.to_string(), name.clone()),
        (LABEL_COMPONENT.to_string(), "credentials".to_string()),
    ]);

    let mut out = StudioCredentials {
        admin_secret_name: None,
        // Same meaning as game-operator status.credentialsGenerated: operator-managed bootstrap.
        admin_generated: studio.spec.generate_admin_secrets,
        api_token: studio.spec.api_token_secret_ref.clone(),
        api_token_generated: false,
    };

    let admin_name = admin_secret_name(studio);
    if studio.spec.generate_admin_secrets {
        let _ = ensure_admin_secret(ctx, ns, &admin_name, &labels, owner).await?;
        out.admin_secret_name = Some(admin_name);
    } else if let Some(LocalObjectRef { name: ref n }) = studio.spec.admin_credentials_secret_ref {
        out.admin_secret_name = Some(n.clone());
        out.admin_generated = false;
    }

    if out.api_token.is_none() && studio.spec.generate_admin_secrets {
        let token_name = format!("{name}-api-token");
        let created = ensure_api_token_secret(ctx, ns, &token_name, &labels, owner).await?;
        out.api_token = Some(SecretKeyRef {
            name: token_name,
            key: SECRET_KEY_API_TOKEN.into(),
        });
        out.api_token_generated = created;
    }

    Ok(out)
}

fn admin_secret_name(studio: &FreyaStudio) -> String {
    if let Some(LocalObjectRef { name }) = &studio.spec.admin_credentials_secret_ref {
        return name.clone();
    }
    format!("{}-admin", studio.name_any())
}

async fn ensure_admin_secret(
    ctx: &Ctx,
    ns: &str,
    secret_name: &str,
    labels: &BTreeMap<String, String>,
    owner: &OwnerReference,
) -> Result<bool, Error> {
    let api: Api<Secret> = Api::namespaced(ctx.client.clone(), ns);
    let existing = api.get(secret_name).await.ok();
    let mut data = existing
        .as_ref()
        .and_then(|s| s.data.clone())
        .unwrap_or_default();

    let mut generated = false;
    if !data.contains_key(SECRET_KEY_ADMIN_USERNAME)
        || data
            .get(SECRET_KEY_ADMIN_USERNAME)
            .map(|b| b.0.is_empty())
            .unwrap_or(true)
    {
        data.insert(
            SECRET_KEY_ADMIN_USERNAME.into(),
            k8s_openapi::ByteString(b"admin".to_vec()),
        );
        generated = true;
    }
    if !data.contains_key(SECRET_KEY_ADMIN_PASSWORD)
        || data
            .get(SECRET_KEY_ADMIN_PASSWORD)
            .map(|b| b.0.is_empty())
            .unwrap_or(true)
    {
        let pw = generate_password();
        data.insert(
            SECRET_KEY_ADMIN_PASSWORD.into(),
            k8s_openapi::ByteString(pw.into_bytes()),
        );
        generated = true;
        info!(%secret_name, "generated admin-password for FreyaStudio admin login");
    }

    let secret = Secret {
        metadata: ObjectMeta {
            name: Some(secret_name.to_string()),
            namespace: Some(ns.to_string()),
            labels: Some(labels.clone()),
            owner_references: Some(vec![owner.clone()]),
            ..Default::default()
        },
        type_: Some("Opaque".into()),
        data: Some(data),
        ..Default::default()
    };
    api.patch(
        secret_name,
        &PatchParams::apply(MANAGED_BY).force(),
        &Patch::Apply(&secret),
    )
    .await
    .map_err(Error::Kube)?;
    Ok(generated)
}

async fn ensure_api_token_secret(
    ctx: &Ctx,
    ns: &str,
    secret_name: &str,
    labels: &BTreeMap<String, String>,
    owner: &OwnerReference,
) -> Result<bool, Error> {
    let api: Api<Secret> = Api::namespaced(ctx.client.clone(), ns);
    let existing = api.get(secret_name).await.ok();
    let mut data = existing
        .as_ref()
        .and_then(|s| s.data.clone())
        .unwrap_or_default();

    let mut generated = false;
    if !data.contains_key(SECRET_KEY_API_TOKEN)
        || data
            .get(SECRET_KEY_API_TOKEN)
            .map(|b| b.0.is_empty())
            .unwrap_or(true)
    {
        let token = generate_api_token();
        data.insert(
            SECRET_KEY_API_TOKEN.into(),
            k8s_openapi::ByteString(token.into_bytes()),
        );
        generated = true;
        info!(%secret_name, "generated API bearer token Secret for FreyaStudio");
    }

    let secret = Secret {
        metadata: ObjectMeta {
            name: Some(secret_name.to_string()),
            namespace: Some(ns.to_string()),
            labels: Some(labels.clone()),
            owner_references: Some(vec![owner.clone()]),
            ..Default::default()
        },
        type_: Some("Opaque".into()),
        data: Some(data),
        ..Default::default()
    };
    api.patch(
        secret_name,
        &PatchParams::apply(MANAGED_BY).force(),
        &Patch::Apply(&secret),
    )
    .await
    .map_err(Error::Kube)?;
    Ok(generated)
}

fn generate_password() -> String {
    // 24 hex chars ≈ game-operator 18 random bytes (URL-safe).
    format!("{}", uuid::Uuid::new_v4().simple())
        .chars()
        .take(24)
        .collect()
}

fn generate_api_token() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}
