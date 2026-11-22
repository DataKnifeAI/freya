//! Reconcile FreyaStudio → API/UI/MCP Deployments, Services, gallery PVC, optional Ingress.

use crate::reconcile::{
    default_api_image, default_mcp_image, default_ui_image, LABEL_COMPONENT, LABEL_INSTANCE,
    LABEL_MANAGED, LABEL_NAME, MANAGED_BY,
};
use crate::Ctx;
use freya_crd::{FreyaDiffusionWorker, FreyaStudio, WorkerPlacement};
use k8s_openapi::api::apps::v1::{Deployment, DeploymentSpec};
use k8s_openapi::api::core::v1::{
    Container, ContainerPort, EnvVar, EnvVarSource, PersistentVolumeClaim,
    PersistentVolumeClaimSpec, PersistentVolumeClaimVolumeSource, PodSpec, PodTemplateSpec,
    SecretKeySelector, Service, ServiceAccount, ServicePort, ServiceSpec, Volume, VolumeMount,
};
use k8s_openapi::api::rbac::v1::{PolicyRule, Role, RoleBinding, RoleRef, Subject};
use k8s_openapi::api::networking::v1::{
    HTTPIngressPath, HTTPIngressRuleValue, Ingress, IngressBackend, IngressRule,
    IngressServiceBackend, IngressSpec, IngressTLS, ServiceBackendPort,
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
    let creds = crate::reconcile::credentials::ensure_studio_credentials(
        &ctx, &ns, &studio, &owner,
    )
    .await?;
    ensure_gallery_pvc(&ctx, &ns, &name, &studio, &owner).await?;
    ensure_api_token_sync_rbac(&ctx, &ns, &name, &studio, &owner, creds.api_token.as_ref()).await?;
    ensure_api_deployment(&ctx, &ns, &name, &studio, &owner, &creds).await?;
    ensure_ui_deployment(&ctx, &ns, &name, &studio, &owner).await?;
    ensure_service(&ctx, &ns, &name, "api", 8080, &owner).await?;
    ensure_service(&ctx, &ns, &name, "ui", 80, &owner).await?;

    let mcp_endpoint = if studio.spec.mcp_enabled {
        ensure_mcp_deployment(&ctx, &ns, &name, &studio, &owner, creds.api_token.as_ref()).await?;
        ensure_service(&ctx, &ns, &name, "mcp", 8090, &owner).await?;
        Some(format!(
            "http://{name}-mcp.{ns}.svc.cluster.local:8090/mcp"
        ))
    } else {
        None
    };

    if studio.spec.ingress.enabled {
        if creds.api_token.is_none() {
            warn!(
                studio = %name,
                "ingress enabled without API token Secret — API stays open to the cluster/Internet"
            );
        }
        ensure_ingress(&ctx, &ns, &name, &studio, &owner).await?;
    }

    let api_svc = format!("http://{name}-api.{ns}.svc.cluster.local:8080");
    let ui_svc = format!("http://{name}-ui.{ns}.svc.cluster.local:80");
    patch_status(
        &ctx,
        &ns,
        &name,
        "Ready",
        &api_svc,
        &ui_svc,
        mcp_endpoint.as_deref(),
        &creds,
    )
    .await?;

    Ok(Action::requeue(Duration::from_secs(120)))
}

const TLS_INSECURE_ANNOTATION: &str = "freya.dataknife.ai/tls-insecure";

async fn resolve_worker(
    ctx: &Ctx,
    ns: &str,
    worker_name: &str,
) -> Result<(String, Option<freya_crd::SecretKeyRef>, bool), Error> {
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
    let tls_insecure = worker
        .annotations()
        .get(TLS_INSECURE_ANNOTATION)
        .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
        .unwrap_or(false);
    Ok((url, worker.spec.auth_secret_ref.clone(), tls_insecure))
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
    creds: &crate::reconcile::credentials::StudioCredentials,
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
        // Keep UI-rotated API token (+ provider keys) on the gallery PVC across pod restarts.
        EnvVar {
            name: "FREYA_SECRETS_FILE".into(),
            value: Some("/data/gallery/.freya-runtime.json".into()),
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
        EnvVar {
            name: "FREYA_ROUTER_MODEL".into(),
            value: Some(studio.spec.open_router.router_model.clone()),
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
        let (worker_url, worker_auth, tls_insecure) =
            resolve_worker(ctx, ns, worker_name).await?;
        env.push(EnvVar {
            name: "FREYA_DIFFUSION_WORKER_URL".into(),
            value: Some(worker_url),
            ..Default::default()
        });
        if tls_insecure {
            env.push(EnvVar {
                name: "FREYA_WORKER_TLS_INSECURE".into(),
                value: Some("true".into()),
                ..Default::default()
            });
        }
        if let Some(ref auth) = worker_auth {
            env.push(secret_env("FREYA_WORKER_TOKEN", auth, true));
        }
    }

    if let Some(ref admin_sec) = creds.admin_secret_name {
        env.push(EnvVar {
            name: "FREYA_ADMIN_USER".into(),
            value_from: Some(EnvVarSource {
                secret_key_ref: Some(SecretKeySelector {
                    name: admin_sec.clone(),
                    key: freya_crd::SECRET_KEY_ADMIN_USERNAME.into(),
                    optional: Some(true),
                }),
                ..Default::default()
            }),
            ..Default::default()
        });
        env.push(EnvVar {
            name: "FREYA_ADMIN_PASSWORD".into(),
            value_from: Some(EnvVarSource {
                secret_key_ref: Some(SecretKeySelector {
                    name: admin_sec.clone(),
                    key: freya_crd::SECRET_KEY_ADMIN_PASSWORD.into(),
                    optional: Some(false),
                }),
                ..Default::default()
            }),
            ..Default::default()
        });
    }

    if let Some(ref api_tok) = creds.api_token {
        env.push(secret_env("FREYA_API_TOKEN", api_tok, false));
        // POST /v1/auth/token patches this Secret so MCP/env stay aligned after UI rotate.
        env.push(EnvVar {
            name: "FREYA_API_TOKEN_SECRET_NAME".into(),
            value: Some(api_tok.name.clone()),
            ..Default::default()
        });
        env.push(EnvVar {
            name: "FREYA_API_TOKEN_SECRET_KEY".into(),
            value: Some(api_tok.key.clone()),
            ..Default::default()
        });
        env.push(EnvVar {
            name: "FREYA_NAMESPACE".into(),
            value: Some(ns.to_string()),
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
                    service_account_name: Some(format!("{name}-api")),
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

/// ServiceAccount + Role so freya-api can patch `apiTokenSecretRef` after UI rotate.
async fn ensure_api_token_sync_rbac(
    ctx: &Ctx,
    ns: &str,
    name: &str,
    studio: &FreyaStudio,
    owner: &OwnerReference,
    api_tok: Option<&freya_crd::SecretKeyRef>,
) -> Result<(), Error> {
    let _ = studio;
    let sa_name = format!("{name}-api");
    let role_name = format!("{name}-api-token-sync");

    let sa = ServiceAccount {
        metadata: ObjectMeta {
            name: Some(sa_name.clone()),
            namespace: Some(ns.to_string()),
            labels: Some(labels(name, "api")),
            owner_references: Some(vec![owner.clone()]),
            ..Default::default()
        },
        ..Default::default()
    };
    let sa_api: Api<ServiceAccount> = Api::namespaced(ctx.client.clone(), ns);
    sa_api
        .patch(
            &sa_name,
            &PatchParams::apply(MANAGED_BY).force(),
            &Patch::Apply(&sa),
        )
        .await
        .map_err(Error::Kube)?;

    let Some(api_tok) = api_tok else {
        return Ok(());
    };

    let role = Role {
        metadata: ObjectMeta {
            name: Some(role_name.clone()),
            namespace: Some(ns.to_string()),
            labels: Some(labels(name, "api")),
            owner_references: Some(vec![owner.clone()]),
            ..Default::default()
        },
        rules: Some(vec![PolicyRule {
            api_groups: Some(vec!["".into()]),
            resources: Some(vec!["secrets".into()]),
            resource_names: Some(vec![api_tok.name.clone()]),
            verbs: vec!["get".into(), "patch".into(), "update".into()],
            ..Default::default()
        }]),
    };
    let role_api: Api<Role> = Api::namespaced(ctx.client.clone(), ns);
    role_api
        .patch(
            &role_name,
            &PatchParams::apply(MANAGED_BY).force(),
            &Patch::Apply(&role),
        )
        .await
        .map_err(Error::Kube)?;

    let binding = RoleBinding {
        metadata: ObjectMeta {
            name: Some(role_name.clone()),
            namespace: Some(ns.to_string()),
            labels: Some(labels(name, "api")),
            owner_references: Some(vec![owner.clone()]),
            ..Default::default()
        },
        role_ref: RoleRef {
            api_group: "rbac.authorization.k8s.io".into(),
            kind: "Role".into(),
            name: role_name.clone(),
        },
        subjects: Some(vec![Subject {
            kind: "ServiceAccount".into(),
            name: sa_name,
            namespace: Some(ns.to_string()),
            ..Default::default()
        }]),
    };
    let rb_api: Api<RoleBinding> = Api::namespaced(ctx.client.clone(), ns);
    rb_api
        .patch(
            &role_name,
            &PatchParams::apply(MANAGED_BY).force(),
            &Patch::Apply(&binding),
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

    // Streamable HTTP Host allowlist: empty/`*` disables DNS-rebinding guard.
    // Behind Ingress the controller already routes by Host; API bearer still applies.
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
            value: Some("*".into()),
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
    let tls = studio.spec.ingress.tls_secret_name.as_ref().map(|secret| {
        vec![IngressTLS {
            hosts: Some(vec![host.clone()]),
            secret_name: Some(secret.clone()),
        }]
    });

    // SSE / Streamable HTTP: disable buffering and extend timeouts for MCP.
    let mut annotations = BTreeMap::from([(
        "nginx.ingress.kubernetes.io/ssl-redirect".into(),
        "true".into(),
    )]);
    if studio.spec.mcp_enabled {
        annotations.insert(
            "nginx.ingress.kubernetes.io/proxy-buffering".into(),
            "off".into(),
        );
        annotations.insert(
            "nginx.ingress.kubernetes.io/proxy-read-timeout".into(),
            "3600".into(),
        );
        annotations.insert(
            "nginx.ingress.kubernetes.io/proxy-send-timeout".into(),
            "3600".into(),
        );
    }

    // Longest / most-specific Prefix paths first so `/mcp` is not swallowed by `/`.
    let mut paths = Vec::new();
    if studio.spec.mcp_enabled {
        paths.push(HTTPIngressPath {
            path: Some("/mcp".into()),
            path_type: "Prefix".into(),
            backend: IngressBackend {
                service: Some(IngressServiceBackend {
                    name: format!("{name}-mcp"),
                    port: Some(ServiceBackendPort {
                        number: Some(8090),
                        ..Default::default()
                    }),
                }),
                ..Default::default()
            },
        });
    }
    paths.push(HTTPIngressPath {
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
    });
    paths.push(HTTPIngressPath {
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
    });

    let ing = Ingress {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            namespace: Some(ns.to_string()),
            labels: Some(labels(name, "ingress")),
            owner_references: Some(vec![owner.clone()]),
            annotations: Some(annotations),
            ..Default::default()
        },
        spec: Some(IngressSpec {
            ingress_class_name: studio.spec.ingress.class_name.clone(),
            tls,
            rules: Some(vec![IngressRule {
                host: Some(host),
                http: Some(HTTPIngressRuleValue { paths }),
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
    mcp_endpoint: Option<&str>,
    creds: &crate::reconcile::credentials::StudioCredentials,
) -> Result<(), Error> {
    let api: Api<FreyaStudio> = Api::namespaced(ctx.client.clone(), ns);
    let mut status_body = serde_json::json!({
        "phase": phase,
        "apiEndpoint": api_endpoint,
        "uiEndpoint": ui_endpoint,
        "adminCredentialsGenerated": creds.admin_generated,
    });
    if let Some(mcp) = mcp_endpoint {
        status_body["mcpEndpoint"] = serde_json::Value::String(mcp.to_string());
    } else {
        status_body["mcpEndpoint"] = serde_json::Value::Null;
    }
    if let Some(ref admin) = creds.admin_secret_name {
        status_body["adminCredentialsSecretName"] = serde_json::Value::String(admin.clone());
    } else {
        status_body["adminCredentialsSecretName"] = serde_json::Value::Null;
    }
    if let Some(ref tok) = creds.api_token {
        status_body["apiTokenSecretName"] = serde_json::Value::String(tok.name.clone());
    } else {
        status_body["apiTokenSecretName"] = serde_json::Value::Null;
    }
    let status = serde_json::json!({ "status": status_body });
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
