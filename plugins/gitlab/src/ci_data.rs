//! Keyword and variable names offered by completion in `.gitlab-ci.yml`. Typed from the GitLab CI/CD YAML
//! reference and the predefined variables page as read in November 2026; GitLab adds names over time, so a
//! missing one is a gap here, not an error in the file.

pub const GLOBAL_KEYWORDS: &[&str] = &[
    "stages", "variables", "default", "include", "workflow", "image", "services", "before_script", "after_script", "cache", "spec",
];

pub const JOB_KEYWORDS: &[&str] = &[
    "script", "before_script", "after_script", "stage", "image", "services", "variables", "rules", "only", "except", "needs",
    "dependencies", "extends", "artifacts", "cache", "environment", "when", "allow_failure", "retry", "timeout", "tags",
    "parallel", "trigger", "coverage", "resource_group", "interruptible", "release", "secrets", "inherit", "start_in",
    "pages", "id_tokens", "hooks", "run", "manual_confirmation", "dast_configuration",
];

pub const CI_VARIABLES: &[&str] = &[
    "CI", "CI_API_V4_URL", "CI_BUILDS_DIR", "CI_COMMIT_AUTHOR", "CI_COMMIT_BEFORE_SHA", "CI_COMMIT_BRANCH",
    "CI_COMMIT_DESCRIPTION", "CI_COMMIT_MESSAGE", "CI_COMMIT_REF_NAME", "CI_COMMIT_REF_PROTECTED", "CI_COMMIT_REF_SLUG",
    "CI_COMMIT_SHA", "CI_COMMIT_SHORT_SHA", "CI_COMMIT_TAG", "CI_COMMIT_TIMESTAMP", "CI_COMMIT_TITLE", "CI_CONCURRENT_ID",
    "CI_CONFIG_PATH", "CI_DEBUG_TRACE", "CI_DEFAULT_BRANCH", "CI_DEPENDENCY_PROXY_SERVER", "CI_DEPLOY_PASSWORD",
    "CI_DEPLOY_USER", "CI_DISPOSABLE_ENVIRONMENT", "CI_ENVIRONMENT_NAME", "CI_ENVIRONMENT_SLUG", "CI_ENVIRONMENT_URL",
    "CI_EXTERNAL_PULL_REQUEST_IID", "CI_JOB_ID", "CI_JOB_IMAGE", "CI_JOB_JWT", "CI_JOB_MANUAL", "CI_JOB_NAME",
    "CI_JOB_STAGE", "CI_JOB_STATUS", "CI_JOB_TOKEN", "CI_JOB_URL", "CI_MERGE_REQUEST_DIFF_ID", "CI_MERGE_REQUEST_IID",
    "CI_MERGE_REQUEST_LABELS", "CI_MERGE_REQUEST_PROJECT_ID", "CI_MERGE_REQUEST_PROJECT_PATH",
    "CI_MERGE_REQUEST_SOURCE_BRANCH_NAME", "CI_MERGE_REQUEST_TARGET_BRANCH_NAME", "CI_MERGE_REQUEST_TITLE",
    "CI_NODE_INDEX", "CI_NODE_TOTAL", "CI_OPEN_MERGE_REQUESTS", "CI_PAGES_DOMAIN", "CI_PAGES_URL", "CI_PIPELINE_ID",
    "CI_PIPELINE_IID", "CI_PIPELINE_SOURCE", "CI_PIPELINE_URL", "CI_PROJECT_DIR", "CI_PROJECT_ID", "CI_PROJECT_NAME",
    "CI_PROJECT_NAMESPACE", "CI_PROJECT_PATH", "CI_PROJECT_PATH_SLUG", "CI_PROJECT_ROOT_NAMESPACE", "CI_PROJECT_TITLE",
    "CI_PROJECT_URL", "CI_PROJECT_VISIBILITY", "CI_REGISTRY", "CI_REGISTRY_IMAGE", "CI_REGISTRY_PASSWORD",
    "CI_REGISTRY_USER", "CI_REPOSITORY_URL", "CI_RUNNER_DESCRIPTION", "CI_RUNNER_ID", "CI_RUNNER_TAGS", "CI_SERVER",
    "CI_SERVER_HOST", "CI_SERVER_NAME", "CI_SERVER_URL", "CI_SERVER_VERSION", "CI_SHARED_ENVIRONMENT", "GITLAB_CI",
    "GITLAB_USER_EMAIL", "GITLAB_USER_ID", "GITLAB_USER_LOGIN", "GITLAB_USER_NAME", "TRIGGER_PAYLOAD",
];
