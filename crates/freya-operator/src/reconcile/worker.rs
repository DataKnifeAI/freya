//! Reconcile FreyaDiffusionWorker — in-cluster GPU Deployment or external remote endpoint.

use crate::reconcile::{
    default_worker_image, LABEL_COMPONENT, LABEL_INSTANCE, LABEL_MANAGED, LABEL_NAME, MANAGED_BY,
};
use crate::Ctx;
use freya_crd::{FreyaDiffusionWorker, WorkerPlacement};
use k8s_openapi::api::apps::v1::{Deployment, DeploymentSpec};
use k8s_openapi::api::core::v1::{
    Container, ContainerPort, EnvVar, PersistentVolumeClaim, PersistentVolumeClaimSpec,
    PersistentVolumeClaimVolumeSource, PodSpec, PodTemplateSpec, ResourceRequirements, Service,
    ServicePort, ServiceSpec, Volume, VolumeMount,
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

pub async fn reconcile(worker: Arc<FreyaDiffusionWorker>, ctx: Arc<Ctx>) -> Result<Action, Error> {
    let ns = worker.namespace().ok_or(Error::MissingNamespace)?;
    let name = worker.name_any();
    info!(%ns, worker = %name, placement = ?worker.spec.placement, "reconciling FreyaDiffusionWorker");

    let owner = worker.controller_owner_ref(&()).ok_or(Error::OwnerRef)?;

    let endpoint = match worker.spec.placement {
        WorkerPlacement::External => {
            let ep = worker
                .spec
                .external_endpoint
                .clone()
                .ok_or(Error::MissingExternalEndpoint)?;
            patch_status(&ctx, &ns, &name, "Ready", &ep).await?;
            return Ok(Action::requeue(Duration::from_secs(300)));
        }
        WorkerPlacement::InCluster => {
            ensure_models_pvc(&ctx, &ns, &name, &worker, &owner).await?;
            ensure_deployment(&ctx, &ns, &name, &worker, &owner).await?;
            ensure_service(&ctx, &ns, &name, worker.spec.port, &owner).await?;
            format!("http://{name}.{ns}.svc.cluster.local:{}", worker.spec.port)
        }
    };

    patch_status(&ctx, &ns, &name, "Ready", &endpoint).await?;
    Ok(Action::requeue(Duration::from_secs(120)))
}

fn labels(instance: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        (LABEL_MANAGED.to_string(), MANAGED_BY.to_string()),
        (LABEL_NAME.to_string(), "freya-diffusion".to_string()),
        (LABEL_INSTANCE.to_string(), instance.to_string()),
        (LABEL_COMPONENT.to_string(), "worker".to_string()),
    ])
}

async fn ensure_models_pvc(
    ctx: &Ctx,
    ns: &str,
    name: &str,
    worker: &FreyaDiffusionWorker,
    owner: &OwnerReference,
) -> Result<(), Error> {
    let pvc_name = format!("{name}-models");
    let pvc = PersistentVolumeClaim {
        metadata: ObjectMeta {
            name: Some(pvc_name.clone()),
            namespace: Some(ns.to_string()),
            labels: Some(labels(name)),
            owner_references: Some(vec![owner.clone()]),
            ..Default::default()
        },
        spec: Some(PersistentVolumeClaimSpec {
            access_modes: Some(vec!["ReadWriteOnce".into()]),
            resources: Some(k8s_openapi::api::core::v1::VolumeResourceRequirements {
                requests: Some(BTreeMap::from([(
                    "storage".into(),
                    k8s_openapi::apimachinery::pkg::api::resource::Quantity(
                        worker.spec.models_pvc_size.clone(),
                    ),
                )])),
                ..Default::default()
            }),
            storage_class_name: worker.spec.storage_class_name.clone(),
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

async fn ensure_deployment(
    ctx: &Ctx,
    ns: &str,
    name: &str,
    worker: &FreyaDiffusionWorker,
    owner: &OwnerReference,
) -> Result<(), Error> {
    let (repo, tag) = match &worker.spec.image {
        Some(i) => (
            i.repository
                .clone()
                .unwrap_or_else(|| default_worker_image().0),
            i.tag.clone().unwrap_or_else(|| default_worker_image().1),
        ),
        None => default_worker_image(),
    };
    let image = format!("{repo}:{tag}");
    let port = worker.spec.port;

    let mut resources = ResourceRequirements {
        limits: Some(BTreeMap::from([(
            "nvidia.com/gpu".into(),
            k8s_openapi::apimachinery::pkg::api::resource::Quantity(
                worker.spec.gpu_count.to_string(),
            ),
        )])),
        ..Default::default()
    };
    let _ = &mut resources;

    let dep = Deployment {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            namespace: Some(ns.to_string()),
            labels: Some(labels(name)),
            owner_references: Some(vec![owner.clone()]),
            ..Default::default()
        },
        spec: Some(DeploymentSpec {
            replicas: Some(1),
            selector: LabelSelector {
                match_labels: Some(labels(name)),
                ..Default::default()
            },
            template: PodTemplateSpec {
                metadata: Some(ObjectMeta {
                    labels: Some(labels(name)),
                    ..Default::default()
                }),
                spec: Some(PodSpec {
                    node_selector: worker.spec.node_selector.clone(),
                    containers: vec![Container {
                        name: "diffusion".into(),
                        image: Some(image),
                        ports: Some(vec![ContainerPort {
                            container_port: port,
                            name: Some("http".into()),
                            ..Default::default()
                        }]),
                        env: Some({
                            let mut env = vec![
                                EnvVar {
                                    name: "FREYA_WORKER_MODEL".into(),
                                    value: Some(worker.spec.model.clone()),
                                    ..Default::default()
                                },
                                EnvVar {
                                    name: "FREYA_WORKER_PORT".into(),
                                    value: Some(port.to_string()),
                                    ..Default::default()
                                },
                                EnvVar {
                                    name: "FREYA_MODELS_DIR".into(),
                                    value: Some("/models".into()),
                                    ..Default::default()
                                },
                            ];
                            if let Some(ref auth) = worker.spec.auth_secret_ref {
                                env.push(EnvVar {
                                    name: "FREYA_WORKER_TOKEN".into(),
                                    value_from: Some(k8s_openapi::api::core::v1::EnvVarSource {
                                        secret_key_ref: Some(
                                            k8s_openapi::api::core::v1::SecretKeySelector {
                                                name: auth.name.clone(),
                                                key: auth.key.clone(),
                                                optional: Some(false),
                                            },
                                        ),
                                        ..Default::default()
                                    }),
                                    ..Default::default()
                                });
                            } else {
                                warn!(
                                    worker = %name,
                                    "authSecretRef unset — worker /v1 routes are open on the cluster network"
                                );
                            }
                            env
                        }),
                        resources: Some(resources),
                        volume_mounts: Some(vec![VolumeMount {
                            name: "models".into(),
                            mount_path: "/models".into(),
                            ..Default::default()
                        }]),
                        ..Default::default()
                    }],
                    volumes: Some(vec![Volume {
                        name: "models".into(),
                        persistent_volume_claim: Some(PersistentVolumeClaimVolumeSource {
                            claim_name: format!("{name}-models"),
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
        name,
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
    name: &str,
    port: i32,
    owner: &OwnerReference,
) -> Result<(), Error> {
    let svc = Service {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            namespace: Some(ns.to_string()),
            labels: Some(labels(name)),
            owner_references: Some(vec![owner.clone()]),
            ..Default::default()
        },
        spec: Some(ServiceSpec {
            selector: Some(labels(name)),
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
        name,
        &PatchParams::apply(MANAGED_BY).force(),
        &Patch::Apply(&svc),
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
    endpoint: &str,
) -> Result<(), Error> {
    let api: Api<FreyaDiffusionWorker> = Api::namespaced(ctx.client.clone(), ns);
    let status = serde_json::json!({
        "status": {
            "phase": phase,
            "endpoint": endpoint,
        }
    });
    api.patch_status(name, &PatchParams::default(), &Patch::Merge(&status))
        .await
        .map_err(Error::Kube)?;
    Ok(())
}

pub fn error_policy(worker: Arc<FreyaDiffusionWorker>, err: &Error, _ctx: Arc<Ctx>) -> Action {
    warn!(worker = %worker.name_any(), error = %err, "worker reconcile failed");
    Action::requeue(Duration::from_secs(30))
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("missing namespace")]
    MissingNamespace,
    #[error("owner reference")]
    OwnerRef,
    #[error("placement=external requires spec.externalEndpoint")]
    MissingExternalEndpoint,
    #[error("kube: {0}")]
    Kube(#[from] kube::Error),
}
