//! Reconcile FreyaStudio → API/UI Deployments, Services, gallery PVC, optional Ingress.

use crate::reconcile::{
    default_api_image, default_ui_image, LABEL_COMPONENT, LABEL_INSTANCE, LABEL_MANAGED,
    LABEL_NAME, MANAGED_BY,
};
use crate::Ctx;
use freya_crd::{FreyaDiffusionWorker, FreyaStudio, WorkerPlacement};
use k8s_openapi::api::apps::v1::{Deployment, DeploymentSpec};
use k8s_openapi::api::core::v1::{
    Container, ContainerPort, EnvVar, EnvVarSource, PersistentVolumeClaim,
    PersistentVolumeClaimSpec, PersistentVolumeClaimVolumeSource, PodSpec, PodTemplateSpec,
    SecretKeySelector, Service, ServicePort, ServiceSpec, Volume, VolumeMount,
};
use k8s_openapi::api::networking::v1::{
    HTTPIngressPath, HTTPIngressRuleValue, Ingress, IngressBackend, IngressRule,
    IngressServiceBackend, IngressSpec, ServiceBackendPort,
};
use k8s_openapi::api::rbac::v1::{PolicyRule, Role, RoleBinding, RoleRef, Subject};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, ObjectMeta, OwnerReference};
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
use kube::api::{Patch, PatchParams};
use kube::runtime::controller::Action;
use kube::{Api, Resource, ResourceExt};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn};

pub async fn reconcile(studio: Arc<FreyaStudio>, ctx: Arc<Ctx>) -> Result<Action, Error> {
    let ns = studio.namespace().ok_or(Error::MissingNamespace)?;
    let name = studio.name_any();
    info!(%ns, studio = %name, "reconciling FreyaStudio");

    let owner = studio.controller_owner_ref(&()).ok_or(Error::OwnerRef)?;
    let creds =
        crate::reconcile::credentials::ensure_studio_credentials(&ctx, &ns, &studio, &owner)
            .await?;
    ensure_gallery_pvc(&ctx, &ns, &name, &studio, &owner).await?;
    ensure_api_deployment(&ctx, &ns, &name, &studio, &owner).await?;
    ensure_ui_deployment(&ctx, &ns, &name, &studio, &owner).await?;
    ensure_service(&ctx, &ns, &name, "api", 8080, &owner).await?;
    ensure_service(&ctx, &ns, &name, "ui", 80, &owner).await?;

    let mcp_endpoint = if studio.spec.mcp_enabled {
        ensure_mcp_deployment(&ctx, &ns, &name, &studio, &owner, creds.api_token.as_ref()).await?;
        ensure_service(&ctx, &ns, &name, "mcp", 8090, &owner).await?;
        Some(format!("http://{name}-mcp.{ns}.svc.cluster.local:8090/mcp"))
    } else {
        None
    };

    if studio.spec.ingress.enabled {
        if studio.spec.api_token_secret_ref.is_none() {
            warn!(
                studio = %name,
                "ingress enabled without apiTokenSecretRef — API stays open to the cluster/Internet"
            );
        }
        ensure_ingress(&ctx, &ns, &name, &studio, &owner).await?;
    }

    let api_svc = format!("http://{name}-api.{ns}.svc.cluster.local:8080");
    let ui_svc = format!("http://{name}-ui.{ns}.svc.cluster.local:80");
    patch_status(&ctx, &ns, &name, "Ready", &api_svc, &ui_svc).await?;

    Ok(Action::requeue(Duration::from_secs(120)))
}

async fn resolve_worker(
    ctx: &Ctx,
    ns: &str,
    worker_name: &str,
) -> Result<(String, Option<freya_crd::SecretKeyRef>), Error> {
    let workers: Api<FreyaDiffusionWorker> = Api::namespaced(ctx.client.clone(), ns);
    let worker = workers.get(worker_name).await.map_err(Error::Kube)?;
    let url = match worker.spec.placement {
        WorkerPlacement::External => worker
            .spec
            .external_endpoint
            .clone()
            .or_else(|| worker.status.as_ref().and_then(|s| s.endpoint.clone()))
            .ok_or(Error::MissingWorkerEndpoint)?,
        WorkerPlacement::InCluster => format!(
            "http://{worker_name}.{ns}.svc.cluster.local:{}",
            worker.spec.port
        ),
    };
    Ok((url, worker.spec.auth_secret_ref.clone()))
}

/// Ingress host plus in-cluster Service DNS and port-forward loopback.
/// Empty or `*` on the MCP process is loopback only, so this list is explicit.
fn mcp_allowed_hosts(studio: &FreyaStudio, name: &str, ns: &str) -> String {
    let svc = format!("{name}-mcp");
    let mut hosts = vec![
        svc.clone(),
        format!("{svc}.{ns}"),
        format!("{svc}.{ns}.svc"),
        format!("{svc}.{ns}.svc.cluster.local"),
        "localhost".into(),
        "127.0.0.1".into(),
        "::1".into(),
    ];
    if let Some(host) = studio
        .spec
        .ingress
        .host
        .as_deref()
        .map(str::trim)
        .filter(|h| !h.is_empty())
    {
        hosts.push(host.to_string());
    }
    hosts.join(",")
}

fn labels(instance: &str, component: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        (LABEL_MANAGED.to_string(), MANAGED_BY.to_string()),
        (LABEL_NAME.to_string(), "freya".to_string()),
        (LABEL_INSTANCE.to_string(), instance.to_string()),
        (LABEL_COMPONENT.to_string(), component.to_string()),
    ])
}

async fn ensure_gallery_pvc(
    ctx: &Ctx,
    ns: &str,
    name: &str,
    studio: &FreyaStudio,
    owner: &OwnerReference,
) -> Result<(), Error> {
    let pvc_name = format!("{name}-gallery");
    let pvc = PersistentVolumeClaim {
        metadata: ObjectMeta {
            name: Some(pvc_name.clone()),
            namespace: Some(ns.to_string()),
            labels: Some(labels(name, "gallery")),
            owner_references: Some(vec![owner.clone()]),
            ..Default::default()
        },
        spec: Some(PersistentVolumeClaimSpec {
            access_modes: Some(vec!["ReadWriteOnce".into()]),
            resources: Some(k8s_openapi::api::core::v1::VolumeResourceRequirements {
                requests: Some(BTreeMap::from([(
                    "storage".into(),
                    k8s_openapi::apimachinery::pkg::api::resource::Quantity(
                        studio.spec.storage.gallery_size.clone(),
                    ),
                )])),
                ..Default::default()
            }),
            storage_class_name: studio.spec.storage.storage_class_name.clone(),
            ..Default::default()
        }),
        ..Default::default()
    };

    let api: Api<PersistentVolumeClaim> = Api::namespaced(ctx.client.clone(), ns);
    api.patch(
        &pvc_name,
        &PatchParams::apply(MANAGED_BY).force(),
        &Patch::Apply(&pvc),
    )
    .await
    .map_err(Error::Kube)?;
    Ok(())
}

fn image_parts(
    custom: &Option<freya_crd::ImageRef>,
    defaults: (String, String),
) -> (String, String, String) {
    let (def_repo, def_tag) = defaults;
    let repo = custom
        .as_ref()
        .and_then(|i| i.repository.clone())
        .unwrap_or(def_repo);
    let tag = custom
        .as_ref()
        .and_then(|i| i.tag.clone())
        .unwrap_or(def_tag);
    let pull = custom
        .as_ref()
        .and_then(|i| i.pull_policy.clone())
        .unwrap_or_else(|| "IfNotPresent".into());
    (repo, tag, pull)
}

fn secret_env(name: &str, secret: &freya_crd::SecretKeyRef, optional: bool) -> EnvVar {
    EnvVar {
        name: name.into(),
        value_from: Some(EnvVarSource {
            secret_key_ref: Some(SecretKeySelector {
                name: secret.name.clone(),
                key: secret.key.clone(),
                optional: Some(optional),
            }),
            ..Default::default()
        }),
        ..Default::default()
    }
}

async fn ensure_api_deployment(
    ctx: &Ctx,
    ns: &str,
    name: &str,
    studio: &FreyaStudio,
    owner: &OwnerReference,
) -> Result<(), Error> {
    let (repo, tag, pull) = image_parts(&studio.spec.api_image, default_api_image());
    let image = format!("{repo}:{tag}");
    let dep_name = format!("{name}-api");
    let gallery = format!("{name}-gallery");
    let secret = &studio.spec.open_router.secret_ref;

    let mut env = vec![
        EnvVar {
            name: "FREYA_GALLERY_DIR".into(),
            value: Some("/data/gallery".into()),
            ..Default::default()
        },
        EnvVar {
            name: "FREYA_PACKS_DIR".into(),
            value: Some("/packs".into()),
            ..Default::default()
        },
        EnvVar {
            name: "OPENROUTER_BASE_URL".into(),
            value: Some(studio.spec.open_router.base_url.clone()),
            ..Default::default()
        },
        EnvVar {
            name: "FREYA_CHAT_MODEL".into(),
            value: Some(studio.spec.open_router.chat_model.clone()),
            ..Default::default()
        },
        EnvVar {
            name: "FREYA_IMAGE_MODEL".into(),
            value: Some(studio.spec.open_router.image_model.clone()),
            ..Default::default()
        },
        secret_env("OPENROUTER_API_KEY", secret, false),
    ];

    if let Some(ref worker_name) = studio.spec.diffusion_worker_ref {
        env.push(EnvVar {
            name: "FREYA_DIFFUSION_WORKER_REF".into(),
            value: Some(worker_name.clone()),
            ..Default::default()
        });
        let (worker_url, worker_auth, tls_insecure) = resolve_worker(ctx, ns, worker_name).await?;
        env.push(EnvVar {
            name: "FREYA_DIFFUSION_WORKER_URL".into(),
            value: Some(worker_url.clone()),
            ..Default::default()
        });
        if tls_insecure && super::tls_skip_allowed(&worker_url) {
            env.push(EnvVar {
                name: "FREYA_WORKER_TLS_INSECURE".into(),
                value: Some("true".into()),
                ..Default::default()
            });
        } else if tls_insecure {
            warn!(
                %worker_url,
                "ignoring freya.dataknife.ai/tls-insecure on a public worker URL"
            );
        }
        if let Some(ref auth) = worker_auth {
            env.push(secret_env("FREYA_WORKER_TOKEN", auth, true));
        }
    }

    if let Some(ref api_tok) = studio.spec.api_token_secret_ref {
        env.push(secret_env("FREYA_API_TOKEN", api_tok, false));
    }

    if let Some(ref civ) = studio.spec.civitai.secret_ref {
        env.push(secret_env("CIVITAI_API_KEY", civ, true));
    }

    if let Some(ref hf) = studio.spec.hugging_face.secret_ref {
        env.push(secret_env("HF_TOKEN", hf, true));
    }

    if matches!(
        studio.spec.chat_backend.kind,
        freya_crd::ChatBackendKind::Vllm
    ) {
        if let Some(url) = &studio.spec.chat_backend.base_url {
            env.push(EnvVar {
                name: "FREYA_CHAT_BASE_URL".into(),
                value: Some(url.clone()),
                ..Default::default()
            });
            env.push(EnvVar {
                name: "FREYA_CHAT_BACKEND".into(),
                value: Some("vllm".into()),
                ..Default::default()
            });
        }
    }

    let dep = Deployment {
        metadata: ObjectMeta {
            name: Some(dep_name.clone()),
            namespace: Some(ns.to_string()),
            labels: Some(labels(name, "api")),
            owner_references: Some(vec![owner.clone()]),
            ..Default::default()
        },
        spec: Some(DeploymentSpec {
            replicas: Some(studio.spec.replicas),
            selector: LabelSelector {
                match_labels: Some(labels(name, "api")),
                ..Default::default()
            },
            template: PodTemplateSpec {
                metadata: Some(ObjectMeta {
                    labels: Some(labels(name, "api")),
                    ..Default::default()
                }),
                spec: Some(PodSpec {
                    containers: vec![Container {
                        name: "api".into(),
                        image: Some(image),
                        image_pull_policy: Some(pull),
                        ports: Some(vec![ContainerPort {
                            container_port: 8080,
                            name: Some("http".into()),
                            ..Default::default()
                        }]),
                        env: Some(env),
                        volume_mounts: Some(vec![VolumeMount {
                            name: "gallery".into(),
                            mount_path: "/data/gallery".into(),
                            ..Default::default()
                        }]),
                        ..Default::default()
                    }],
                    volumes: Some(vec![Volume {
                        name: "gallery".into(),
                        persistent_volume_claim: Some(PersistentVolumeClaimVolumeSource {
                            claim_name: gallery,
                            ..Default::default()
                        }),
                        ..Default::default()
                    }]),
                    ..Default::default()
                }),
            },
            ..Default::default()
        }),
        ..Default::default()
    };

    let api: Api<Deployment> = Api::namespaced(ctx.client.clone(), ns);
    api.patch(
        &dep_name,
        &PatchParams::apply(MANAGED_BY).force(),
        &Patch::Apply(&dep),
    )
    .await
    .map_err(Error::Kube)?;
    Ok(())
}

async fn ensure_ui_deployment(
    ctx: &Ctx,
    ns: &str,
    name: &str,
    studio: &FreyaStudio,
    owner: &OwnerReference,
) -> Result<(), Error> {
    let (repo, tag, pull) = image_parts(&studio.spec.ui_image, default_ui_image());
    let image = format!("{repo}:{tag}");
    let dep_name = format!("{name}-ui");

    let dep = Deployment {
        metadata: ObjectMeta {
            name: Some(dep_name.clone()),
            namespace: Some(ns.to_string()),
            labels: Some(labels(name, "ui")),
            owner_references: Some(vec![owner.clone()]),
            ..Default::default()
        },
        spec: Some(DeploymentSpec {
            replicas: Some(studio.spec.replicas),
            selector: LabelSelector {
                match_labels: Some(labels(name, "ui")),
                ..Default::default()
            },
            template: PodTemplateSpec {
                metadata: Some(ObjectMeta {
                    labels: Some(labels(name, "ui")),
                    ..Default::default()
                }),
                spec: Some(PodSpec {
                    containers: vec![Container {
                        name: "ui".into(),
                        image: Some(image),
                        image_pull_policy: Some(pull),
                        ports: Some(vec![ContainerPort {
                            container_port: 80,
                            name: Some("http".into()),
                            ..Default::default()
                        }]),
                        env: Some(vec![EnvVar {
                            // Consumed by nginx envsubst template → upstream for /v1/.
                            name: "FREYA_API_UPSTREAM".into(),
                            value: Some(format!("http://{name}-api:8080")),
                            ..Default::default()
                        }]),
                        ..Default::default()
                    }],
                    ..Default::default()
                }),
            },
            ..Default::default()
        }),
        ..Default::default()
    };

    let api: Api<Deployment> = Api::namespaced(ctx.client.clone(), ns);
    api.patch(
        &dep_name,
        &PatchParams::apply(MANAGED_BY).force(),
        &Patch::Apply(&dep),
    )
    .await
    .map_err(Error::Kube)?;
    Ok(())
}

/// Stateless MCP proxy: Streamable HTTP at `/mcp`, talks to in-cluster API.
async fn ensure_mcp_deployment(
    ctx: &Ctx,
    ns: &str,
    name: &str,
    studio: &FreyaStudio,
    owner: &OwnerReference,
    api_tok: Option<&freya_crd::SecretKeyRef>,
) -> Result<(), Error> {
    let (repo, tag, pull) = image_parts(&studio.spec.mcp_image, default_mcp_image());
    let image = format!("{repo}:{tag}");
    let dep_name = format!("{name}-mcp");

    // Host allowlist is the Ingress host when one is set. Callers still need the API bearer.
    let mut env = vec![
        EnvVar {
            name: "FREYA_API_URL".into(),
            value: Some(format!("http://{name}-api:8080")),
            ..Default::default()
        },
        EnvVar {
            name: "FREYA_MCP_BIND".into(),
            value: Some("0.0.0.0:8090".into()),
            ..Default::default()
        },
        EnvVar {
            name: "FREYA_MCP_ALLOWED_HOSTS".into(),
            value: Some(mcp_allowed_hosts(studio, name, ns)),
            ..Default::default()
        },
    ];
    if let Some(api_tok) = api_tok {
        env.push(secret_env("FREYA_API_TOKEN", api_tok, false));
    }

    let dep = Deployment {
        metadata: ObjectMeta {
            name: Some(dep_name.clone()),
            namespace: Some(ns.to_string()),
            labels: Some(labels(name, "mcp")),
            owner_references: Some(vec![owner.clone()]),
            ..Default::default()
        },
        spec: Some(DeploymentSpec {
            replicas: Some(studio.spec.replicas),
            selector: LabelSelector {
                match_labels: Some(labels(name, "mcp")),
                ..Default::default()
            },
            template: PodTemplateSpec {
                metadata: Some(ObjectMeta {
                    labels: Some(labels(name, "mcp")),
                    ..Default::default()
                }),
                spec: Some(PodSpec {
                    containers: vec![Container {
                        name: "mcp".into(),
                        image: Some(image),
                        image_pull_policy: Some(pull),
                        ports: Some(vec![ContainerPort {
                            container_port: 8090,
                            name: Some("http".into()),
                            ..Default::default()
                        }]),
                        env: Some(env),
                        ..Default::default()
                    }],
                    ..Default::default()
                }),
            },
            ..Default::default()
        }),
        ..Default::default()
    };

    let api: Api<Deployment> = Api::namespaced(ctx.client.clone(), ns);
    api.patch(
        &dep_name,
        &PatchParams::apply(MANAGED_BY).force(),
        &Patch::Apply(&dep),
    )
    .await
    .map_err(Error::Kube)?;
    Ok(())
}

async fn ensure_service(
    ctx: &Ctx,
    ns: &str,
    instance: &str,
    component: &str,
    port: i32,
    owner: &OwnerReference,
) -> Result<(), Error> {
    let svc_name = format!("{instance}-{component}");
    let svc = Service {
        metadata: ObjectMeta {
            name: Some(svc_name.clone()),
            namespace: Some(ns.to_string()),
            labels: Some(labels(instance, component)),
            owner_references: Some(vec![owner.clone()]),
            ..Default::default()
        },
        spec: Some(ServiceSpec {
            selector: Some(labels(instance, component)),
            ports: Some(vec![ServicePort {
                name: Some("http".into()),
                port,
                target_port: Some(IntOrString::Int(port)),
                ..Default::default()
            }]),
            ..Default::default()
        }),
        ..Default::default()
    };
    let api: Api<Service> = Api::namespaced(ctx.client.clone(), ns);
    api.patch(
        &svc_name,
        &PatchParams::apply(MANAGED_BY).force(),
        &Patch::Apply(&svc),
    )
    .await
    .map_err(Error::Kube)?;
    Ok(())
}

async fn ensure_ingress(
    ctx: &Ctx,
    ns: &str,
    name: &str,
    studio: &FreyaStudio,
    owner: &OwnerReference,
) -> Result<(), Error> {
    let host = studio
        .spec
        .ingress
        .host
        .clone()
        .unwrap_or_else(|| format!("{name}.freya.local"));
    let ing = Ingress {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            namespace: Some(ns.to_string()),
            labels: Some(labels(name, "ingress")),
            owner_references: Some(vec![owner.clone()]),
            ..Default::default()
        },
        spec: Some(IngressSpec {
            ingress_class_name: studio.spec.ingress.class_name.clone(),
            rules: Some(vec![IngressRule {
                host: Some(host),
                http: Some(HTTPIngressRuleValue {
                    paths: vec![
                        HTTPIngressPath {
                            path: Some("/api".into()),
                            path_type: "Prefix".into(),
                            backend: IngressBackend {
                                service: Some(IngressServiceBackend {
                                    name: format!("{name}-api"),
                                    port: Some(ServiceBackendPort {
                                        number: Some(8080),
                                        ..Default::default()
                                    }),
                                }),
                                ..Default::default()
                            },
                        },
                        HTTPIngressPath {
                            path: Some("/".into()),
                            path_type: "Prefix".into(),
                            backend: IngressBackend {
                                service: Some(IngressServiceBackend {
                                    name: format!("{name}-ui"),
                                    port: Some(ServiceBackendPort {
                                        number: Some(80),
                                        ..Default::default()
                                    }),
                                }),
                                ..Default::default()
                            },
                        },
                    ],
                }),
            }]),
            ..Default::default()
        }),
        ..Default::default()
    };
    let api: Api<Ingress> = Api::namespaced(ctx.client.clone(), ns);
    api.patch(
        name,
        &PatchParams::apply(MANAGED_BY).force(),
        &Patch::Apply(&ing),
    )
    .await
    .map_err(Error::Kube)?;
    Ok(())
}

async fn patch_status(
    ctx: &Ctx,
    ns: &str,
    name: &str,
    phase: &str,
    api_endpoint: &str,
    ui_endpoint: &str,
) -> Result<(), Error> {
    let api: Api<FreyaStudio> = Api::namespaced(ctx.client.clone(), ns);
    let status = serde_json::json!({
        "status": {
            "phase": phase,
            "apiEndpoint": api_endpoint,
            "uiEndpoint": ui_endpoint,
        }
    });
    api.patch_status(name, &PatchParams::default(), &Patch::Merge(&status))
        .await
        .map_err(Error::Kube)?;
    Ok(())
}

pub fn error_policy(studio: Arc<FreyaStudio>, err: &Error, _ctx: Arc<Ctx>) -> Action {
    warn!(studio = %studio.name_any(), error = %err, "studio reconcile failed");
    Action::requeue(Duration::from_secs(30))
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("missing namespace")]
    MissingNamespace,
    #[error("owner reference")]
    OwnerRef,
    #[error("diffusion worker missing externalEndpoint")]
    MissingWorkerEndpoint,
    #[error("kube: {0}")]
    Kube(#[from] kube::Error),
}
