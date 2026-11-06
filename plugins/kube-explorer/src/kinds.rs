//! A trimmed table of the core, apps, batch, networking and rbac kinds: which
//! top-level and `spec` keys exist, which fields the API server owns and which
//! keys are deprecated. Everything outside the table (custom resources, other
//! groups, newer versions of a kind) is left alone, and the `spec` keys of a
//! kind are only judged where the table is complete. Written from the public
//! API reference; a field added by a Kubernetes release newer than the table
//! shows as an Info, never as an error.

use crate::manifest::{Doc, Value};

pub const UNKNOWN_KEY: &str = "unknown-key";
pub const UNKNOWN_KIND: &str = "unknown-kind";
pub const READ_ONLY_FIELD: &str = "read-only-field";
pub const DEPRECATED_KEY: &str = "deprecated-key";
pub const MISSING_KEY: &str = "missing-key";
pub const INVALID_VALUE: &str = "invalid-value";

pub enum Problem {
    UnknownKey,
    UnknownKind,
    ReadOnly,
    Deprecated,
    MissingKey,
    InvalidValue,
}

pub struct Issue {
    pub start: usize,
    pub end: usize,
    pub problem: Problem,
    pub message: String,
}

struct Kind {
    group: &'static str,
    name: &'static str,
    top: &'static [&'static str],
    spec: Option<&'static [&'static str]>,
}

const POD_SPEC: &[&str] = &[
    "activeDeadlineSeconds", "affinity", "automountServiceAccountToken", "containers", "dnsConfig", "dnsPolicy",
    "enableServiceLinks", "ephemeralContainers", "hostAliases", "hostIPC", "hostNetwork", "hostPID", "hostUsers",
    "hostname", "imagePullSecrets", "initContainers", "nodeName", "nodeSelector", "os", "overhead",
    "preemptionPolicy", "priority", "priorityClassName", "readinessGates", "resourceClaims", "resources",
    "restartPolicy", "runtimeClassName", "schedulerName", "schedulingGates", "securityContext", "serviceAccount",
    "serviceAccountName", "setHostnameAsFQDN", "shareProcessNamespace", "subdomain", "terminationGracePeriodSeconds",
    "tolerations", "topologySpreadConstraints", "volumes",
];

const KINDS: &[Kind] = &[
    Kind { group: "", name: "Pod", top: &["spec"], spec: Some(POD_SPEC) },
    Kind {
        group: "",
        name: "Service",
        top: &["spec"],
        spec: Some(&[
            "allocateLoadBalancerNodePorts", "clusterIP", "clusterIPs", "externalIPs", "externalName",
            "externalTrafficPolicy", "healthCheckNodePort", "internalTrafficPolicy", "ipFamilies", "ipFamilyPolicy",
            "loadBalancerClass", "loadBalancerIP", "loadBalancerSourceRanges", "ports", "publishNotReadyAddresses",
            "selector", "sessionAffinity", "sessionAffinityConfig", "trafficDistribution", "type",
        ]),
    },
    Kind { group: "", name: "ConfigMap", top: &["data", "binaryData", "immutable"], spec: None },
    Kind { group: "", name: "Secret", top: &["data", "stringData", "type", "immutable"], spec: None },
    Kind { group: "", name: "Namespace", top: &["spec"], spec: Some(&["finalizers"]) },
    Kind {
        group: "",
        name: "PersistentVolumeClaim",
        top: &["spec"],
        spec: Some(&[
            "accessModes", "dataSource", "dataSourceRef", "resources", "selector", "storageClassName",
            "volumeAttributesClassName", "volumeMode", "volumeName",
        ]),
    },
    Kind { group: "", name: "PersistentVolume", top: &["spec"], spec: None },
    Kind { group: "", name: "ServiceAccount", top: &["automountServiceAccountToken", "imagePullSecrets", "secrets"], spec: None },
    Kind { group: "", name: "Node", top: &["spec"], spec: None },
    Kind { group: "", name: "Endpoints", top: &["subsets"], spec: None },
    Kind { group: "", name: "LimitRange", top: &["spec"], spec: None },
    Kind { group: "", name: "ResourceQuota", top: &["spec"], spec: None },
    Kind { group: "", name: "ReplicationController", top: &["spec"], spec: None },
    Kind { group: "", name: "PodTemplate", top: &["template"], spec: None },
    Kind { group: "", name: "List", top: &["items"], spec: None },
    Kind {
        group: "apps",
        name: "Deployment",
        top: &["spec"],
        spec: Some(&["minReadySeconds", "paused", "progressDeadlineSeconds", "replicas", "revisionHistoryLimit", "selector", "strategy", "template"]),
    },
    Kind {
        group: "apps",
        name: "StatefulSet",
        top: &["spec"],
        spec: Some(&[
            "minReadySeconds", "ordinals", "persistentVolumeClaimRetentionPolicy", "podManagementPolicy", "replicas",
            "revisionHistoryLimit", "selector", "serviceName", "template", "updateStrategy", "volumeClaimTemplates",
        ]),
    },
    Kind {
        group: "apps",
        name: "DaemonSet",
        top: &["spec"],
        spec: Some(&["minReadySeconds", "revisionHistoryLimit", "selector", "template", "updateStrategy"]),
    },
    Kind { group: "apps", name: "ReplicaSet", top: &["spec"], spec: Some(&["minReadySeconds", "replicas", "selector", "template"]) },
    Kind { group: "apps", name: "ControllerRevision", top: &["data", "revision"], spec: None },
    Kind {
        group: "batch",
        name: "Job",
        top: &["spec"],
        spec: Some(&[
            "activeDeadlineSeconds", "backoffLimit", "backoffLimitPerIndex", "completionMode", "completions",
            "managedBy", "manualSelector", "maxFailedIndexes", "parallelism", "podFailurePolicy",
            "podReplacementPolicy", "selector", "successPolicy", "suspend", "template", "ttlSecondsAfterFinished",
        ]),
    },
    Kind {
        group: "batch",
        name: "CronJob",
        top: &["spec"],
        spec: Some(&[
            "concurrencyPolicy", "failedJobsHistoryLimit", "jobTemplate", "schedule", "startingDeadlineSeconds",
            "successfulJobsHistoryLimit", "suspend", "timeZone",
        ]),
    },
    Kind {
        group: "networking.k8s.io",
        name: "Ingress",
        top: &["spec"],
        spec: Some(&["defaultBackend", "ingressClassName", "rules", "tls"]),
    },
    Kind { group: "networking.k8s.io", name: "IngressClass", top: &["spec"], spec: None },
    Kind {
        group: "networking.k8s.io",
        name: "NetworkPolicy",
        top: &["spec"],
        spec: Some(&["egress", "ingress", "podSelector", "policyTypes"]),
    },
    Kind { group: "autoscaling", name: "HorizontalPodAutoscaler", top: &["spec"], spec: None },
    Kind { group: "policy", name: "PodDisruptionBudget", top: &["spec"], spec: None },
    Kind {
        group: "storage.k8s.io",
        name: "StorageClass",
        top: &["allowVolumeExpansion", "allowedTopologies", "mountOptions", "parameters", "provisioner", "reclaimPolicy", "volumeBindingMode"],
        spec: None,
    },
    Kind { group: "scheduling.k8s.io", name: "PriorityClass", top: &["description", "globalDefault", "preemptionPolicy", "value"], spec: None },
    Kind { group: "coordination.k8s.io", name: "Lease", top: &["spec"], spec: None },
    Kind { group: "discovery.k8s.io", name: "EndpointSlice", top: &["addressType", "endpoints", "ports"], spec: None },
    Kind { group: "node.k8s.io", name: "RuntimeClass", top: &["handler", "overhead", "scheduling"], spec: None },
    Kind { group: "certificates.k8s.io", name: "CertificateSigningRequest", top: &["spec"], spec: None },
    Kind { group: "apiextensions.k8s.io", name: "CustomResourceDefinition", top: &["spec"], spec: None },
    Kind { group: "admissionregistration.k8s.io", name: "ValidatingWebhookConfiguration", top: &["webhooks"], spec: None },
    Kind { group: "admissionregistration.k8s.io", name: "MutatingWebhookConfiguration", top: &["webhooks"], spec: None },
    Kind { group: "rbac.authorization.k8s.io", name: "Role", top: &["rules"], spec: None },
    Kind { group: "rbac.authorization.k8s.io", name: "ClusterRole", top: &["rules", "aggregationRule"], spec: None },
    Kind { group: "rbac.authorization.k8s.io", name: "RoleBinding", top: &["roleRef", "subjects"], spec: None },
    Kind { group: "rbac.authorization.k8s.io", name: "ClusterRoleBinding", top: &["roleRef", "subjects"], spec: None },
];

/// Groups whose kinds the table lists completely, plus kinds served but not worth a template.
const COMPLETE_GROUPS: &[(&str, &[&str])] = &[
    ("", &["Binding", "ComponentStatus", "Config", "DeleteOptions", "Event", "Eviction", "RangeAllocation", "Status"]),
    ("apps", &[]),
    ("batch", &[]),
    ("networking.k8s.io", &["IPAddress", "ServiceCIDR"]),
    ("rbac.authorization.k8s.io", &[]),
    ("autoscaling", &["HorizontalPodAutoscaler", "Scale"]),
    ("policy", &["PodDisruptionBudget", "Eviction"]),
    ("storage.k8s.io", &["StorageClass", "VolumeAttachment", "CSIDriver", "CSINode", "CSIStorageCapacity", "VolumeAttributesClass"]),
    ("scheduling.k8s.io", &["PriorityClass"]),
    ("coordination.k8s.io", &["Lease", "LeaseCandidate"]),
    ("discovery.k8s.io", &["EndpointSlice"]),
    ("node.k8s.io", &["RuntimeClass"]),
    ("certificates.k8s.io", &["CertificateSigningRequest", "ClusterTrustBundle", "PodCertificateRequest"]),
    (
        "admissionregistration.k8s.io",
        &[
            "ValidatingWebhookConfiguration", "MutatingWebhookConfiguration", "ValidatingAdmissionPolicy",
            "ValidatingAdmissionPolicyBinding", "MutatingAdmissionPolicy", "MutatingAdmissionPolicyBinding",
        ],
    ),
    ("apiextensions.k8s.io", &["CustomResourceDefinition"]),
    ("flowcontrol.apiserver.k8s.io", &["FlowSchema", "PriorityLevelConfiguration"]),
    ("events.k8s.io", &["Event"]),
    ("resource.k8s.io", &["ResourceClaim", "ResourceClaimTemplate", "DeviceClass", "ResourceSlice", "DeviceTaintRule"]),
    ("authentication.k8s.io", &["TokenReview", "TokenRequest", "SelfSubjectReview"]),
    (
        "authorization.k8s.io",
        &["SubjectAccessReview", "LocalSubjectAccessReview", "SelfSubjectAccessReview", "SelfSubjectRulesReview"],
    ),
];

/// Kinds the API serves for reading or for a subresource call, not for `kubectl apply`.
const NON_EDITABLE_KINDS: &[(&str, &str, &str)] = &[
    ("", "ComponentStatus", "`ComponentStatus` is read-only: the API server reports it and does not accept it."),
    ("", "Binding", "`Binding` is created through a pod's `binding` subresource, not applied as a manifest."),
    ("", "Eviction", "`Eviction` is created through a pod's `eviction` subresource, not applied as a manifest."),
    ("authentication.k8s.io", "TokenReview", "`TokenReview` is a request/response object; it is created, never stored."),
    ("authorization.k8s.io", "SubjectAccessReview", "`SubjectAccessReview` is a request/response object; it is created, never stored."),
];

const DEPRECATED_KINDS: &[(&str, &str, &str)] = &[
    ("", "Endpoints", "`Endpoints` is deprecated since Kubernetes v1.33; use `discovery.k8s.io/v1` `EndpointSlice`."),
    ("", "ComponentStatus", "`ComponentStatus` is deprecated since Kubernetes v1.19."),
];

/// Required keys: kind, `key` (top level) or `spec.key`, and a `spec` key that must be present for
/// the document to look whole rather than like a patch (`spec.selector` is judged by `missing-selector`).
const REQUIRED: &[(&str, &str, &str)] = &[
    ("Pod", "spec.containers", ""),
    ("Deployment", "spec.template", "selector"),
    ("StatefulSet", "spec.template", "selector"),
    ("StatefulSet", "spec.serviceName", "template"),
    ("DaemonSet", "spec.template", "selector"),
    ("ReplicaSet", "spec.template", "selector"),
    ("CronJob", "spec.jobTemplate", "schedule"),
    ("CronJob", "spec.schedule", "jobTemplate"),
    ("NetworkPolicy", "spec.podSelector", ""),
    ("PersistentVolumeClaim", "spec.accessModes", "resources"),
    ("RoleBinding", "roleRef", ""),
    ("ClusterRoleBinding", "roleRef", ""),
    ("ControllerRevision", "revision", ""),
    ("List", "items", ""),
];

/// Accepted values of an enumerated field: kind, `spec` key (`parent.key` one level down), values.
const ENUMS: &[(&str, &str, &[&str])] = &[
    ("Pod", "restartPolicy", &["Always", "OnFailure", "Never"]),
    ("Pod", "dnsPolicy", &["ClusterFirst", "ClusterFirstWithHostNet", "Default", "None"]),
    ("Pod", "preemptionPolicy", &["PreemptLowerPriority", "Never"]),
    ("Service", "type", &["ClusterIP", "NodePort", "LoadBalancer", "ExternalName"]),
    ("Service", "sessionAffinity", &["None", "ClientIP"]),
    ("Service", "externalTrafficPolicy", &["Cluster", "Local"]),
    ("Service", "internalTrafficPolicy", &["Cluster", "Local"]),
    ("Service", "ipFamilyPolicy", &["SingleStack", "PreferDualStack", "RequireDualStack"]),
    ("Deployment", "strategy.type", &["Recreate", "RollingUpdate"]),
    ("StatefulSet", "podManagementPolicy", &["OrderedReady", "Parallel"]),
    ("StatefulSet", "updateStrategy.type", &["RollingUpdate", "OnDelete"]),
    ("DaemonSet", "updateStrategy.type", &["RollingUpdate", "OnDelete"]),
    ("Job", "completionMode", &["NonIndexed", "Indexed"]),
    ("Job", "podReplacementPolicy", &["TerminatingOrFailed", "Failed"]),
    ("CronJob", "concurrencyPolicy", &["Allow", "Forbid", "Replace"]),
    ("PersistentVolumeClaim", "volumeMode", &["Filesystem", "Block"]),
    ("PersistentVolume", "volumeMode", &["Filesystem", "Block"]),
    ("PersistentVolume", "persistentVolumeReclaimPolicy", &["Retain", "Delete", "Recycle"]),
];

const INTEGER_FIELDS: &[&str] = &[
    "replicas", "minReadySeconds", "revisionHistoryLimit", "progressDeadlineSeconds", "backoffLimit", "completions",
    "parallelism", "activeDeadlineSeconds", "ttlSecondsAfterFinished", "terminationGracePeriodSeconds",
    "successfulJobsHistoryLimit", "failedJobsHistoryLimit", "startingDeadlineSeconds", "priority",
];

const BOOLEAN_FIELDS: &[&str] = &[
    "paused", "suspend", "hostNetwork", "hostPID", "hostIPC", "hostUsers", "shareProcessNamespace",
    "automountServiceAccountToken", "setHostnameAsFQDN", "enableServiceLinks", "publishNotReadyAddresses",
    "manualSelector", "allocateLoadBalancerNodePorts",
];

const DEPRECATED_VALUES: &[(&str, &str, &str, &str)] = &[(
    "PersistentVolume",
    "persistentVolumeReclaimPolicy",
    "Recycle",
    "The `Recycle` reclaim policy is deprecated; use dynamic provisioning with `Delete` or `Retain`.",
)];

const READ_ONLY_METADATA: &[&str] = &[
    "creationTimestamp", "deletionGracePeriodSeconds", "deletionTimestamp", "generation", "managedFields",
    "resourceVersion", "selfLink", "uid",
];

const DEPRECATED_SPEC: &[(&str, &str, &str)] = &[
    ("Pod", "serviceAccount", "`spec.serviceAccount` is deprecated; use `serviceAccountName`."),
    ("Service", "loadBalancerIP", "`spec.loadBalancerIP` is deprecated since Kubernetes v1.24; use a provider annotation or the cloud's own setting."),
    ("Deployment", "rollbackTo", "`spec.rollbackTo` was removed in apps/v1; use `kubectl rollout undo`."),
];

const DEPRECATED_ANNOTATIONS: &[(&str, &str, &str)] = &[
    ("Ingress", "kubernetes.io/ingress.class", "The `kubernetes.io/ingress.class` annotation is deprecated; use `spec.ingressClassName`."),
    ("PersistentVolumeClaim", "volume.beta.kubernetes.io/storage-class", "The `volume.beta.kubernetes.io/storage-class` annotation is deprecated; use `spec.storageClassName`."),
    ("PersistentVolumeClaim", "volume.beta.kubernetes.io/storage-provisioner", "The `volume.beta.kubernetes.io/storage-provisioner` annotation is deprecated; use `volume.kubernetes.io/storage-provisioner`."),
    ("Pod", "seccomp.security.alpha.kubernetes.io/pod", "The seccomp annotation is deprecated since Kubernetes v1.19; use `spec.securityContext.seccompProfile`."),
];

fn has_alias(text: &str) -> bool {
    text.lines().any(|line| {
        let content = line.trim_start();
        !content.starts_with('#')
            && (content.contains("<<:") || content.contains(": &") || content.contains(": *") || content.starts_with("- &") || content.starts_with("- *"))
    })
}

fn find_kind<'a>(group: &str, name: &str) -> Option<&'a Kind> {
    KINDS.iter().find(|kind| kind.group == group && kind.name == name)
}

fn value_range(value: &Value) -> (usize, usize) {
    (value.start, value.end)
}

pub fn check(doc: &Doc, text: &str) -> Vec<Issue> {
    let (Some(api_version), Some(kind)) = (&doc.api_version, &doc.kind) else { return Vec::new() };
    let (group, version) = api_version.text.rsplit_once('/').unwrap_or(("", api_version.text.as_str()));
    if has_alias(&text[doc.range.clone()]) {
        return Vec::new();
    }
    let mut issues = Vec::new();
    let on_kind = |problem: Problem, message: &str| Issue { start: kind.start, end: kind.end, problem, message: message.to_owned() };
    if let Some((_, _, message)) = NON_EDITABLE_KINDS.iter().find(|(g, name, _)| *g == group && *name == kind.text) {
        issues.push(on_kind(Problem::ReadOnly, message));
    }
    if let Some((_, _, message)) = DEPRECATED_KINDS.iter().find(|(g, name, _)| *g == group && *name == kind.text) {
        issues.push(on_kind(Problem::Deprecated, message));
    }
    let Some(entry) = find_kind(group, &kind.text) else {
        let known = COMPLETE_GROUPS.iter().find(|(known, _)| *known == group);
        if let Some((_, extra)) = known {
            if !kind.text.ends_with("List") && !extra.contains(&kind.text.as_str()) && !kind.text.contains(['{', '$']) {
                let place = if group.is_empty() { version.to_owned() } else { api_version.text.clone() };
                issues.push(Issue {
                    start: kind.start,
                    end: kind.end,
                    problem: Problem::UnknownKind,
                    message: format!("`{}` is not a kind of {place}; custom resources use their own apiVersion.", kind.text),
                });
            }
        }
        return issues;
    };
    for (key, range, value) in &doc.top_entries {
        let known = ["apiVersion", "kind", "metadata", "status"].contains(&key.as_str()) || entry.top.contains(&key.as_str());
        if !known && !key.starts_with('$') {
            issues.push(Issue {
                start: range.start,
                end: range.end,
                problem: Problem::UnknownKey,
                message: format!("`{key}` is not a field of {}.", kind.text),
            });
        }
        let empty = value.as_ref().is_some_and(|v| v.text == "{}");
        if key == "status" && !empty {
            issues.push(Issue {
                start: range.start,
                end: range.end,
                problem: Problem::ReadOnly,
                message: "`status` is written by the API server; it is ignored when applied.".into(),
            });
        }
    }
    for (key, key_value, value) in &doc.metadata_keys {
        if READ_ONLY_METADATA.contains(&key.as_str()) && value.as_ref().is_none_or(|v| v.text != "null") {
            let (start, end) = value_range(key_value);
            issues.push(Issue {
                start,
                end,
                problem: Problem::ReadOnly,
                message: format!("`metadata.{key}` is set by the API server; remove it from a manifest you apply."),
            });
        }
        if key == "clusterName" {
            let (start, end) = value_range(key_value);
            issues.push(Issue {
                start,
                end,
                problem: Problem::Deprecated,
                message: "`metadata.clusterName` was removed in Kubernetes v1.25.".into(),
            });
        }
    }
    for (annotation, _) in &doc.annotation_keys {
        if let Some((_, _, message)) = DEPRECATED_ANNOTATIONS.iter().find(|(k, key, _)| *k == kind.text && *key == annotation.text) {
            issues.push(Issue { start: annotation.start, end: annotation.end, problem: Problem::Deprecated, message: (*message).to_owned() });
        }
    }
    if version != "v1" || doc.spec_inline {
        return issues;
    }
    let in_spec = |key: &str| doc.spec_keys.iter().any(|k| k.text == key);
    for (_, path, needs) in REQUIRED.iter().filter(|(name, ..)| *name == kind.text) {
        let present = match path.strip_prefix("spec.") {
            Some(key) => in_spec(key) || !doc.top_keys.iter().any(|k| k == "spec"),
            None => doc.top_keys.iter().any(|k| k == path),
        };
        if !present && (needs.is_empty() || in_spec(needs)) {
            let message = format!("{} requires `{path}`.", kind.text);
            issues.push(on_kind(Problem::MissingKey, &message));
        }
    }
    for (path, value) in &doc.spec_values {
        if value.text.is_empty() || value.text.starts_with(['{', '[', '|', '>', '&', '*', '!']) || value.text.contains(['$', '{']) {
            continue;
        }
        let name = path.as_str();
        let invalid = |message: String| Issue { start: value.start, end: value.end, problem: Problem::InvalidValue, message };
        if let Some((_, _, allowed)) = ENUMS.iter().find(|(k, field, _)| *k == kind.text && *field == name) {
            if !allowed.contains(&value.text.as_str()) {
                issues.push(invalid(format!("`{}` is not a value of `spec.{name}` in the bundled reference ({}).", value.text, allowed.join(", "))));
            }
        }
        if let Some((.., message)) = DEPRECATED_VALUES.iter().find(|(k, field, v, _)| *k == kind.text && *field == name && *v == value.text) {
            issues.push(Issue { start: value.start, end: value.end, problem: Problem::Deprecated, message: (*message).to_owned() });
        }
        if INTEGER_FIELDS.contains(&name) && value.text.parse::<i64>().is_err() && entry.spec.is_some() {
            issues.push(invalid(format!("`spec.{name}` takes an integer, not `{}`.", value.text)));
        }
        if BOOLEAN_FIELDS.contains(&name) && entry.spec.is_some() && !["true", "false"].contains(&value.text.to_lowercase().as_str()) {
            issues.push(invalid(format!("`spec.{name}` takes true or false, not `{}`.", value.text)));
        }
    }
    for key in &doc.spec_keys {
        if let Some((_, _, message)) = DEPRECATED_SPEC.iter().find(|(k, name, _)| *k == kind.text && *name == key.text) {
            issues.push(Issue { start: key.start, end: key.end, problem: Problem::Deprecated, message: (*message).to_owned() });
            continue;
        }
        if entry.spec.is_some_and(|allowed| !allowed.contains(&key.text.as_str()) && !key.text.starts_with('$')) {
            issues.push(Issue {
                start: key.start,
                end: key.end,
                problem: Problem::UnknownKey,
                message: format!("`{}` is not a field of {} `spec` in the bundled reference; a release newer than it may have added it.", key.text, kind.text),
            });
        }
    }
    issues
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::documents;

    fn found(text: &str) -> Vec<(String, String)> {
        let docs = documents(text);
        docs.iter()
            .flat_map(|doc| check(doc, text))
            .map(|issue| {
                let name = match issue.problem {
                    Problem::UnknownKey => UNKNOWN_KEY,
                    Problem::UnknownKind => UNKNOWN_KIND,
                    Problem::ReadOnly => READ_ONLY_FIELD,
                    Problem::Deprecated => DEPRECATED_KEY,
                    Problem::MissingKey => MISSING_KEY,
                    Problem::InvalidValue => INVALID_VALUE,
                };
                (name.to_owned(), text[issue.start..issue.end].to_owned())
            })
            .collect()
    }

    fn pairs(items: &[(&str, &str)]) -> Vec<(String, String)> {
        items.iter().map(|(a, b)| ((*a).to_owned(), (*b).to_owned())).collect()
    }

    #[test]
    fn a_misspelled_top_level_or_spec_key_is_reported_on_the_key() {
        let text = "apiVersion: apps/v1\nkind: Deployment\nmetadata:\n  name: a\nspecs:\n  replicas: 1\n---\napiVersion: apps/v1\nkind: Deployment\nspec:\n  replica: 1\n  selector: {}\n";
        assert_eq!(found(text), pairs(&[(UNKNOWN_KEY, "specs"), (MISSING_KEY, "Deployment"), (UNKNOWN_KEY, "replica")]));
    }

    #[test]
    fn every_bundled_template_is_clean() {
        for template in ["pod", "deployment", "statefulset", "daemonset", "job", "cronjob", "service", "ingress", "configmap", "secret", "namespace", "role", "rolebinding", "clusterrole", "clusterrolebinding"] {
            let text = std::fs::read_to_string(format!("templates/{template}.yaml")).unwrap().replace("$NAME$", "x").replace("$END$", "");
            assert_eq!(found(&text), Vec::new(), "{template}");
        }
    }

    #[test]
    fn a_kind_the_group_does_not_have_is_reported_but_custom_groups_are_quiet() {
        let text = "apiVersion: apps/v1\nkind: Pod\n---\napiVersion: v1\nkind: Deploymnt\n---\napiVersion: cert-manager.io/v1\nkind: Certificate\nspec:\n  anything: 1\n---\napiVersion: monitoring.coreos.com/v1\nkind: Pod\n";
        assert_eq!(found(text), pairs(&[(UNKNOWN_KIND, "Pod"), (UNKNOWN_KIND, "Deploymnt")]));
    }

    #[test]
    fn list_kinds_and_served_extras_are_quiet() {
        assert!(found("apiVersion: v1\nkind: PodList\n---\napiVersion: v1\nkind: Event\n---\napiVersion: v1\nkind: List\nitems: []\n").is_empty());
    }

    #[test]
    fn kinds_without_a_complete_spec_table_only_get_the_top_level_check() {
        assert!(found("apiVersion: v1\nkind: Node\nspec:\n  whatever: 1\n").is_empty());
        assert_eq!(found("apiVersion: v1\nkind: Node\nrules: []\n"), pairs(&[(UNKNOWN_KEY, "rules")]));
    }

    #[test]
    fn older_versions_of_a_kind_skip_the_spec_check() {
        assert!(found("apiVersion: apps/v1beta1\nkind: Deployment\nspec:\n  rollbackTo: {}\n  whatever: 1\n").is_empty());
    }

    #[test]
    fn server_owned_fields_are_reported_unless_empty_or_null() {
        let text = "apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: a\n  uid: 1\n  creationTimestamp: null\n  resourceVersion: \"4\"\nstatus: {}\n---\napiVersion: v1\nkind: Pod\nstatus:\n  phase: Running\n";
        assert_eq!(found(text), pairs(&[(READ_ONLY_FIELD, "uid"), (READ_ONLY_FIELD, "resourceVersion"), (READ_ONLY_FIELD, "status")]));
    }

    #[test]
    fn deprecated_keys_and_annotations_are_named() {
        let text = "apiVersion: networking.k8s.io/v1\nkind: Ingress\nmetadata:\n  annotations:\n    kubernetes.io/ingress.class: nginx\n---\napiVersion: v1\nkind: Pod\nspec:\n  serviceAccount: a\n  containers: []\n---\napiVersion: apps/v1\nkind: Deployment\nspec:\n  rollbackTo: {}\n";
        assert_eq!(
            found(text),
            pairs(&[(DEPRECATED_KEY, "kubernetes.io/ingress.class"), (DEPRECATED_KEY, "serviceAccount"), (DEPRECATED_KEY, "rollbackTo")])
        );
    }

    #[test]
    fn anchors_aliases_and_merge_keys_silence_the_document() {
        for body in ["base: &b\n  a: 1\nbogus: *b\n", "<<: *x\nbogus: 1\n", "spec:\n  <<: *x\n  bogus: 1\n"] {
            assert!(found(&format!("apiVersion: apps/v1\nkind: Deployment\n{body}")).is_empty(), "{body}");
        }
    }

    #[test]
    fn required_keys_are_reported_only_on_a_document_that_looks_whole() {
        let whole = "apiVersion: apps/v1\nkind: Deployment\nspec:\n  selector: {}\n";
        assert_eq!(found(whole), pairs(&[(MISSING_KEY, "Deployment")]));
        assert!(found("apiVersion: apps/v1\nkind: Deployment\nspec:\n  replicas: 2\n").is_empty());
        assert!(found("apiVersion: apps/v1\nkind: Deployment\nmetadata:\n  name: a\n").is_empty());
        assert_eq!(found("apiVersion: v1\nkind: Pod\nspec:\n  hostNetwork: true\n"), pairs(&[(MISSING_KEY, "Pod")]));
        assert_eq!(found("apiVersion: rbac.authorization.k8s.io/v1\nkind: RoleBinding\nsubjects: []\n"), pairs(&[(MISSING_KEY, "RoleBinding")]));
        assert_eq!(found("apiVersion: batch/v1\nkind: CronJob\nspec:\n  schedule: '* * * * *'\n"), pairs(&[(MISSING_KEY, "CronJob")]));
        assert!(found("apiVersion: apps/v1beta2\nkind: Deployment\nspec:\n  selector: {}\n").is_empty());
    }

    #[test]
    fn enum_integer_and_boolean_values_are_checked_and_templated_values_are_not() {
        let text = "apiVersion: v1\nkind: Pod\nspec:\n  restartPolicy: Sometimes\n  hostNetwork: maybe\n  priority: high\n  dnsPolicy: ClusterFirst\n  terminationGracePeriodSeconds: ${GRACE}\n  containers: []\n";
        assert_eq!(found(text), pairs(&[(INVALID_VALUE, "Sometimes"), (INVALID_VALUE, "maybe"), (INVALID_VALUE, "high")]));
        let nested = "apiVersion: apps/v1\nkind: Deployment\nspec:\n  selector: {}\n  template: {}\n  strategy:\n    type: Rolling\n  replicas: 3\n";
        assert_eq!(found(nested), pairs(&[(INVALID_VALUE, "Rolling")]));
        assert_eq!(found("apiVersion: v1\nkind: Service\nspec:\n  type: ClusterIP\n  ports: []\n"), Vec::new());
        assert_eq!(found("apiVersion: v1\nkind: Service\nspec:\n  type: Cluster\n"), pairs(&[(INVALID_VALUE, "Cluster")]));
    }

    #[test]
    fn non_editable_deprecated_kinds_and_values_are_named() {
        assert_eq!(found("apiVersion: v1\nkind: ComponentStatus\n"), pairs(&[(READ_ONLY_FIELD, "ComponentStatus"), (DEPRECATED_KEY, "ComponentStatus")]));
        assert_eq!(found("apiVersion: v1\nkind: Endpoints\nsubsets: []\n"), pairs(&[(DEPRECATED_KEY, "Endpoints")]));
        assert_eq!(found("apiVersion: v1\nkind: PersistentVolume\nspec:\n  persistentVolumeReclaimPolicy: Recycle\n"), pairs(&[(DEPRECATED_KEY, "Recycle")]));
        assert_eq!(found("apiVersion: v1\nkind: Pod\nmetadata:\n  annotations:\n    seccomp.security.alpha.kubernetes.io/pod: runtime/default\nspec:\n  containers: []\n"), pairs(&[(DEPRECATED_KEY, "seccomp.security.alpha.kubernetes.io/pod")]));
    }

    #[test]
    fn the_added_kinds_have_their_top_level_keys_checked() {
        assert_eq!(found("apiVersion: storage.k8s.io/v1\nkind: StorageClass\nprovisioner: x\nprovisoner: y\n"), pairs(&[(UNKNOWN_KEY, "provisoner")]));
        assert_eq!(found("apiVersion: autoscaling/v2\nkind: HorizontalPodAutoscaler\nspec: {}\nwebhooks: []\n"), pairs(&[(UNKNOWN_KEY, "webhooks")]));
    }

    #[test]
    fn more_api_groups_know_their_kinds() {
        assert_eq!(found("apiVersion: storage.k8s.io/v1\nkind: StorageClas\n"), pairs(&[(UNKNOWN_KIND, "StorageClas")]));
        assert!(found("apiVersion: storage.k8s.io/v1\nkind: StorageClass\nprovisioner: x\n---\napiVersion: policy/v1\nkind: PodDisruptionBudget\n---\napiVersion: autoscaling/v2\nkind: HorizontalPodAutoscaler\n").is_empty());
    }

    #[test]
    fn inline_spec_is_not_read() {
        assert!(found("apiVersion: apps/v1\nkind: Deployment\nspec: {bogus: 1}\n").is_empty());
    }
}
