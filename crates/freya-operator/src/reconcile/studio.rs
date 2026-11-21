//! Reconcile FreyaStudio → API/UI Deployments, Services, gallery PVC, optional Ingress.

use crate::reconcile::{
    default_api_image, default_ui_image, LABEL_COMPONENT, LABEL_INSTANCE, LABEL_MANAGED,
    LABEL_NAME, MANAGED_BY,
};
use crate::Ctx;
use freya_crd::FreyaStudio;
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
    ensure_gallery_pvc(&ctx, &ns, &name, &studio, &owner).await?;
    ensure_api_deployment(&ctx, &ns, &name, &studio, &owner).await?;
    ensure_ui_deployment(&ctx, &ns, &name, &studio, &owner).await?;
    ensure_service(&ctx, &ns, &name, "api", 8080, &owner).await?;
    ensure_service(&ctx, &ns, &name, "ui", 80, &owner).await?;
    if studio.spec.ingress.enabled {
        ensure_ingress(&ctx, &ns, &name, &studio, &owner).await?;
    }

    let api_svc = format!("http://{name}-api.{ns}.svc.cluster.local:8080");
    let ui_svc = format!("http://{name}-ui.{ns}.svc.cluster.local:80");
    patch_status(&ctx, &ns, &name, "Ready", &api_svc, &ui_svc).await?;

    Ok(Action::requeue(Duration::from_secs(120)))
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

    if let Some(ref worker) = studio.spec.diffusion_worker_ref {
        env.push(EnvVar {
            name: "FREYA_DIFFUSION_WORKER_REF".into(),
            value: Some(worker.clone()),
            ..Default::default()
        });
        env.push(EnvVar {
            name: "FREYA_DIFFUSION_WORKER_URL".into(),
            value: Some(format!("http://{worker}.{ns}.svc.cluster.local:8091")),
            ..Default::default()
        });
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
                            name: "FREYA_API_URL".into(),
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
    #[error("kube: {0}")]
    Kube(#[from] kube::Error),
}
