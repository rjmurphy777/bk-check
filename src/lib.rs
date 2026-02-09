pub mod buildkite;
pub mod cli;
pub mod github;
pub mod log_parser;
pub mod models;
pub mod parse;

use anyhow::{anyhow, Context, Result};
use futures::future::join_all;
use tokio::sync::Semaphore;

use buildkite::BuildkiteClient;
use github::GitHubClient;
use models::{BuildReport, FailedJob, JobSummary};
use parse::{parse_buildkite_url, parse_pr_url};

pub async fn run(
    pr_url_str: &str,
    max_log_lines: usize,
    gh_client: &GitHubClient,
    bk_client: &BuildkiteClient,
) -> Result<BuildReport> {
    // 1. Parse PR URL
    let pr_info = parse_pr_url(pr_url_str)?;

    // 2. Fetch PR metadata from GitHub
    let pr = gh_client
        .get_pr(&pr_info.owner, &pr_info.repo, pr_info.pr_number)
        .await
        .context("Failed to fetch PR info")?;

    let branch = pr.head.ref_name;
    let sha = pr.head.sha;

    // 3. Fetch commit status to find Buildkite build URL
    let status = gh_client
        .get_commit_status(&pr_info.owner, &pr_info.repo, &sha)
        .await
        .context("Failed to fetch commit status")?;

    // Find Buildkite status entries
    let bk_statuses: Vec<_> = status
        .statuses
        .iter()
        .filter(|s| s.context.starts_with("buildkite/"))
        .filter_map(|s| s.target_url.as_ref())
        .collect();

    if bk_statuses.is_empty() {
        return Err(anyhow!("No Buildkite status checks found for commit {sha}"));
    }

    // Use the first Buildkite build URL found
    let bk_url = bk_statuses[0];
    let bk_info =
        parse_buildkite_url(bk_url).context("Failed to parse Buildkite URL from GitHub status")?;

    // 4. Fetch build details from Buildkite
    let build = bk_client
        .get_build(&bk_info.org, &bk_info.pipeline, bk_info.build_number)
        .await
        .context("Failed to fetch Buildkite build")?;

    // 5. Categorize jobs (sync, no I/O)
    let mut passed_jobs = Vec::new();
    let mut warnings = Vec::new();

    struct FailedJobMeta {
        name: String,
        state: String,
        exit_status: Option<i32>,
        web_url: Option<String>,
        job_id: String,
    }

    let mut failed_metas = Vec::new();

    for job in &build.jobs {
        if job.job_type != "script" {
            continue;
        }

        let name = job
            .name
            .clone()
            .unwrap_or_else(|| format!("unnamed-{}", job.id));
        let state = job.state.clone().unwrap_or_else(|| "unknown".to_string());

        if job.soft_failed == Some(true) {
            warnings.push(format!("Soft-failed: {name} ({state})"));
            passed_jobs.push(JobSummary {
                name,
                state: format!("{state} (soft-failed)"),
            });
            continue;
        }

        match state.as_str() {
            "passed" => {
                passed_jobs.push(JobSummary { name, state });
            }
            "failed" | "waiting_failed" | "canceled" | "timed_out" => {
                failed_metas.push(FailedJobMeta {
                    name,
                    state,
                    exit_status: job.exit_status,
                    web_url: job.web_url.clone(),
                    job_id: job.id.clone(),
                });
            }
            _ => {
                passed_jobs.push(JobSummary { name, state });
            }
        }
    }

    // 6. Fetch all failed job logs concurrently (max 10 at a time)
    let semaphore = Semaphore::new(10);
    let org = &bk_info.org;
    let pipeline = &bk_info.pipeline;
    let build_number = bk_info.build_number;
    let log_futures = failed_metas.iter().map(|meta| {
        let sem = &semaphore;
        async move {
            let _permit = sem.acquire().await.unwrap();
            bk_client
                .get_job_log(org, pipeline, build_number, &meta.job_id)
                .await
        }
    });
    let log_results = join_all(log_futures).await;

    // 7. Assemble FailedJob structs
    let mut failed_jobs = Vec::new();
    for (meta, log_result) in failed_metas.into_iter().zip(log_results) {
        let failure_log = match log_result {
            Ok(log_resp) => log_parser::clean_log(&log_resp.content, max_log_lines),
            Err(e) => {
                warnings.push(format!("Failed to fetch log for {}: {e}", meta.name));
                "(log unavailable)".to_string()
            }
        };
        failed_jobs.push(FailedJob {
            name: meta.name,
            state: meta.state,
            exit_status: meta.exit_status,
            web_url: meta.web_url,
            failure_log,
        });
    }

    Ok(BuildReport::new(
        pr_url_str.to_string(),
        branch,
        sha,
        build.web_url,
        passed_jobs,
        failed_jobs,
        warnings,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn setup_github_mocks(
        server: &MockServer,
        sha: &str,
        bk_build_url: &str,
        bk_state: &str,
    ) {
        // PR endpoint
        Mock::given(method("GET"))
            .and(path("/repos/ROKT/canal/pulls/14908"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "head": {
                    "ref": "feature-branch",
                    "sha": sha
                }
            })))
            .mount(server)
            .await;

        // Commit status endpoint
        Mock::given(method("GET"))
            .and(path(format!("/repos/ROKT/canal/commits/{sha}/status")))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "state": bk_state,
                "statuses": [
                    {
                        "context": "buildkite/catalog-ci-pipeline",
                        "state": bk_state,
                        "target_url": bk_build_url,
                        "description": "Build result"
                    }
                ]
            })))
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn test_full_flow_failure() {
        let gh_server = MockServer::start().await;
        let bk_server = MockServer::start().await;

        let bk_build_url = format!(
            "{}/rokt/catalog-ci-pipeline/builds/5939",
            "https://buildkite.com"
        );

        setup_github_mocks(&gh_server, "sha123", &bk_build_url, "failure").await;

        // Buildkite build endpoint
        Mock::given(method("GET"))
            .and(path(
                "/v2/organizations/rokt/pipelines/catalog-ci-pipeline/builds/5939",
            ))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "number": 5939,
                    "state": "failed",
                    "branch": "feature-branch",
                    "commit": "sha123",
                    "message": "Fix things",
                    "web_url": "https://buildkite.com/rokt/catalog-ci-pipeline/builds/5939",
                    "jobs": [
                        {
                            "id": "job-pass",
                            "name": "lint",
                            "type": "script",
                            "state": "passed",
                            "exit_status": 0,
                            "soft_failed": false,
                            "web_url": "https://buildkite.com/rokt/catalog-ci-pipeline/builds/5939#job-pass",
                            "log_url": null
                        },
                        {
                            "id": "job-fail",
                            "name": "pytest shard 1",
                            "type": "script",
                            "state": "failed",
                            "exit_status": 1,
                            "soft_failed": false,
                            "web_url": "https://buildkite.com/rokt/catalog-ci-pipeline/builds/5939#job-fail",
                            "log_url": "http://example.com/log"
                        },
                        {
                            "id": "job-waiter",
                            "name": null,
                            "type": "waiter",
                            "state": null,
                            "exit_status": null,
                            "soft_failed": null,
                            "web_url": null,
                            "log_url": null
                        }
                    ]
                })),
            )
            .mount(&bk_server)
            .await;

        // Job log endpoint
        Mock::given(method("GET"))
            .and(path(
                "/v2/organizations/rokt/pipelines/catalog-ci-pipeline/builds/5939/jobs/job-fail/log",
            ))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "content": "Running tests...\nImportError: No module named 'catalog.service'\nFAILED test_something",
                    "size": 80
                })),
            )
            .mount(&bk_server)
            .await;

        let gh_client = GitHubClient::with_base_url("gh-token".to_string(), gh_server.uri());
        let bk_client = BuildkiteClient::with_base_url("bk-token".to_string(), bk_server.uri());

        let report = run(
            "https://github.com/ROKT/canal/pull/14908",
            100,
            &gh_client,
            &bk_client,
        )
        .await
        .unwrap();

        assert_eq!(report.overall_status, "failure");
        assert_eq!(report.branch, "feature-branch");
        assert_eq!(report.commit, "sha123");
        assert_eq!(report.passed_jobs.len(), 1);
        assert_eq!(report.passed_jobs[0].name, "lint");
        assert_eq!(report.failed_jobs.len(), 1);
        assert_eq!(report.failed_jobs[0].jobs[0].name, "pytest shard 1");
        assert!(report.failed_jobs[0].failure_log.contains("ImportError"));
    }

    #[tokio::test]
    async fn test_full_flow_success() {
        let gh_server = MockServer::start().await;
        let bk_server = MockServer::start().await;

        let bk_build_url = "https://buildkite.com/rokt/catalog-ci-pipeline/builds/5959";

        setup_github_mocks(&gh_server, "sha456", bk_build_url, "success").await;

        Mock::given(method("GET"))
            .and(path(
                "/v2/organizations/rokt/pipelines/catalog-ci-pipeline/builds/5959",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "number": 5959,
                "state": "passed",
                "branch": "main",
                "commit": "sha456",
                "message": "All good",
                "web_url": "https://buildkite.com/rokt/catalog-ci-pipeline/builds/5959",
                "jobs": [
                    {
                        "id": "j1",
                        "name": "lint",
                        "type": "script",
                        "state": "passed",
                        "exit_status": 0,
                        "soft_failed": false,
                        "web_url": null,
                        "log_url": null
                    },
                    {
                        "id": "j2",
                        "name": "test",
                        "type": "script",
                        "state": "passed",
                        "exit_status": 0,
                        "soft_failed": false,
                        "web_url": null,
                        "log_url": null
                    }
                ]
            })))
            .mount(&bk_server)
            .await;

        let gh_client = GitHubClient::with_base_url("gh-token".to_string(), gh_server.uri());
        let bk_client = BuildkiteClient::with_base_url("bk-token".to_string(), bk_server.uri());

        let report = run(
            "https://github.com/ROKT/canal/pull/14908",
            100,
            &gh_client,
            &bk_client,
        )
        .await
        .unwrap();

        assert_eq!(report.overall_status, "success");
        assert_eq!(report.passed_jobs.len(), 2);
        assert!(report.failed_jobs.is_empty());
    }

    #[tokio::test]
    async fn test_no_buildkite_checks() {
        let gh_server = MockServer::start().await;
        let bk_server = MockServer::start().await;

        // PR endpoint
        Mock::given(method("GET"))
            .and(path("/repos/ROKT/canal/pulls/14908"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "head": {
                    "ref": "feature-branch",
                    "sha": "sha789"
                }
            })))
            .mount(&gh_server)
            .await;

        // Commit status with no Buildkite entries
        Mock::given(method("GET"))
            .and(path("/repos/ROKT/canal/commits/sha789/status"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "state": "success",
                "statuses": [
                    {
                        "context": "ci/circleci",
                        "state": "success",
                        "target_url": "https://circleci.com/build/123",
                        "description": "CircleCI build"
                    }
                ]
            })))
            .mount(&gh_server)
            .await;

        let gh_client = GitHubClient::with_base_url("gh-token".to_string(), gh_server.uri());
        let bk_client = BuildkiteClient::with_base_url("bk-token".to_string(), bk_server.uri());

        let result = run(
            "https://github.com/ROKT/canal/pull/14908",
            100,
            &gh_client,
            &bk_client,
        )
        .await;

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("No Buildkite status checks"));
    }

    #[tokio::test]
    async fn test_buildkite_api_unavailable() {
        let gh_server = MockServer::start().await;
        let bk_server = MockServer::start().await;

        let bk_build_url = "https://buildkite.com/rokt/catalog-ci-pipeline/builds/5939";

        setup_github_mocks(&gh_server, "sha111", bk_build_url, "failure").await;

        // Buildkite returns 500
        Mock::given(method("GET"))
            .and(path(
                "/v2/organizations/rokt/pipelines/catalog-ci-pipeline/builds/5939",
            ))
            .respond_with(ResponseTemplate::new(500))
            .mount(&bk_server)
            .await;

        let gh_client = GitHubClient::with_base_url("gh-token".to_string(), gh_server.uri());
        let bk_client = BuildkiteClient::with_base_url("bk-token".to_string(), bk_server.uri());

        let result = run(
            "https://github.com/ROKT/canal/pull/14908",
            100,
            &gh_client,
            &bk_client,
        )
        .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_soft_failed_jobs_are_warnings() {
        let gh_server = MockServer::start().await;
        let bk_server = MockServer::start().await;

        let bk_build_url = "https://buildkite.com/rokt/pipeline/builds/1";

        setup_github_mocks(&gh_server, "sha_sf", bk_build_url, "success").await;

        Mock::given(method("GET"))
            .and(path("/v2/organizations/rokt/pipelines/pipeline/builds/1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "number": 1,
                "state": "passed",
                "branch": "main",
                "commit": "sha_sf",
                "message": null,
                "web_url": "https://buildkite.com/rokt/pipeline/builds/1",
                "jobs": [
                    {
                        "id": "j1",
                        "name": "optional-lint",
                        "type": "script",
                        "state": "failed",
                        "exit_status": 1,
                        "soft_failed": true,
                        "web_url": null,
                        "log_url": null
                    }
                ]
            })))
            .mount(&bk_server)
            .await;

        let gh_client = GitHubClient::with_base_url("gh-token".to_string(), gh_server.uri());
        let bk_client = BuildkiteClient::with_base_url("bk-token".to_string(), bk_server.uri());

        let report = run(
            "https://github.com/ROKT/canal/pull/14908",
            100,
            &gh_client,
            &bk_client,
        )
        .await
        .unwrap();

        assert_eq!(report.overall_status, "success");
        assert!(report.failed_jobs.is_empty());
        assert_eq!(report.warnings.len(), 1);
        assert!(report.warnings[0].contains("Soft-failed"));
    }

    #[tokio::test]
    async fn test_log_fetch_failure_adds_warning() {
        let gh_server = MockServer::start().await;
        let bk_server = MockServer::start().await;

        let bk_build_url = "https://buildkite.com/rokt/pipeline/builds/2";

        setup_github_mocks(&gh_server, "sha_lf", bk_build_url, "failure").await;

        Mock::given(method("GET"))
            .and(path("/v2/organizations/rokt/pipelines/pipeline/builds/2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "number": 2,
                "state": "failed",
                "branch": "main",
                "commit": "sha_lf",
                "message": null,
                "web_url": "https://buildkite.com/rokt/pipeline/builds/2",
                "jobs": [
                    {
                        "id": "jfail",
                        "name": "test",
                        "type": "script",
                        "state": "failed",
                        "exit_status": 1,
                        "soft_failed": false,
                        "web_url": null,
                        "log_url": null
                    }
                ]
            })))
            .mount(&bk_server)
            .await;

        // Log endpoint returns 500
        Mock::given(method("GET"))
            .and(path(
                "/v2/organizations/rokt/pipelines/pipeline/builds/2/jobs/jfail/log",
            ))
            .respond_with(ResponseTemplate::new(500))
            .mount(&bk_server)
            .await;

        let gh_client = GitHubClient::with_base_url("gh-token".to_string(), gh_server.uri());
        let bk_client = BuildkiteClient::with_base_url("bk-token".to_string(), bk_server.uri());

        let report = run(
            "https://github.com/ROKT/canal/pull/14908",
            100,
            &gh_client,
            &bk_client,
        )
        .await
        .unwrap();

        assert_eq!(report.overall_status, "failure");
        assert_eq!(report.failed_jobs.len(), 1);
        assert_eq!(report.failed_jobs[0].failure_log, "(log unavailable)");
        assert_eq!(report.failed_jobs[0].jobs[0].name, "test");
        assert!(!report.warnings.is_empty());
        assert!(report.warnings[0].contains("Failed to fetch log"));
    }

    #[tokio::test]
    async fn test_unnamed_and_running_jobs() {
        let gh_server = MockServer::start().await;
        let bk_server = MockServer::start().await;

        let bk_build_url = "https://buildkite.com/rokt/pipeline/builds/3";

        setup_github_mocks(&gh_server, "sha_ur", bk_build_url, "success").await;

        Mock::given(method("GET"))
            .and(path("/v2/organizations/rokt/pipelines/pipeline/builds/3"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "number": 3,
                "state": "running",
                "branch": "main",
                "commit": "sha_ur",
                "message": null,
                "web_url": "https://buildkite.com/rokt/pipeline/builds/3",
                "jobs": [
                    {
                        "id": "j-unnamed",
                        "name": null,
                        "type": "script",
                        "state": null,
                        "exit_status": null,
                        "soft_failed": null,
                        "web_url": null,
                        "log_url": null
                    },
                    {
                        "id": "j-running",
                        "name": "deploy",
                        "type": "script",
                        "state": "running",
                        "exit_status": null,
                        "soft_failed": false,
                        "web_url": null,
                        "log_url": null
                    }
                ]
            })))
            .mount(&bk_server)
            .await;

        let gh_client = GitHubClient::with_base_url("gh-token".to_string(), gh_server.uri());
        let bk_client = BuildkiteClient::with_base_url("bk-token".to_string(), bk_server.uri());

        let report = run(
            "https://github.com/ROKT/canal/pull/14908",
            100,
            &gh_client,
            &bk_client,
        )
        .await
        .unwrap();

        assert_eq!(report.overall_status, "success");
        assert_eq!(report.passed_jobs.len(), 2);
        assert_eq!(report.passed_jobs[0].name, "unnamed-j-unnamed");
        assert_eq!(report.passed_jobs[0].state, "unknown");
        assert_eq!(report.passed_jobs[1].name, "deploy");
        assert_eq!(report.passed_jobs[1].state, "running");
    }
}
