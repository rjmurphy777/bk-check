use anyhow::{anyhow, Context, Result};
use reqwest::Client;
use serde::Deserialize;

const BUILDKITE_API_BASE: &str = "https://api.buildkite.com";

pub struct BuildkiteClient {
    client: Client,
    base_url: String,
    token: String,
}

#[derive(Debug, Deserialize)]
pub struct BkBuild {
    pub number: u64,
    pub state: String,
    pub branch: String,
    pub commit: String,
    pub message: Option<String>,
    pub web_url: String,
    pub jobs: Vec<BkJob>,
}

#[derive(Debug, Deserialize)]
pub struct BkJob {
    pub id: String,
    pub name: Option<String>,
    #[serde(rename = "type")]
    pub job_type: String,
    pub state: Option<String>,
    pub exit_status: Option<i32>,
    pub soft_failed: Option<bool>,
    pub web_url: Option<String>,
    pub log_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct BkLogResponse {
    pub content: String,
    pub size: u64,
}

impl BuildkiteClient {
    pub fn new(token: String) -> Self {
        Self::with_base_url(token, BUILDKITE_API_BASE.to_string())
    }

    pub fn with_base_url(token: String, base_url: String) -> Self {
        let client = Client::builder()
            .user_agent("bk-check/0.1")
            .build()
            .expect("Failed to build HTTP client");
        BuildkiteClient {
            client,
            base_url,
            token,
        }
    }

    pub async fn get_build(&self, org: &str, pipeline: &str, build_number: u64) -> Result<BkBuild> {
        let url = format!(
            "{}/v2/organizations/{}/pipelines/{}/builds/{}",
            self.base_url, org, pipeline, build_number
        );
        let resp = self
            .client
            .get(&url)
            .bearer_auth(&self.token)
            .send()
            .await
            .context("Failed to reach Buildkite API")?;

        if resp.status() == 403 {
            return Err(anyhow!(
                "Access denied to Buildkite build {org}/{pipeline}/builds/{build_number}. Check your BUILDKITE_API_TOKEN permissions."
            ));
        }

        if resp.status() == 404 {
            return Err(anyhow!(
                "Buildkite build not found: {org}/{pipeline}/builds/{build_number}"
            ));
        }

        let resp = resp
            .error_for_status()
            .context("Buildkite API returned an error")?;

        resp.json::<BkBuild>()
            .await
            .context("Failed to parse Buildkite build response")
    }

    pub async fn get_job_log(
        &self,
        org: &str,
        pipeline: &str,
        build_number: u64,
        job_id: &str,
    ) -> Result<BkLogResponse> {
        let url = format!(
            "{}/v2/organizations/{}/pipelines/{}/builds/{}/jobs/{}/log",
            self.base_url, org, pipeline, build_number, job_id
        );
        let resp = self
            .client
            .get(&url)
            .bearer_auth(&self.token)
            .header("Accept", "application/json")
            .send()
            .await
            .context("Failed to reach Buildkite API for job log")?;

        if resp.status() == 404 {
            return Err(anyhow!(
                "Job log not found: {org}/{pipeline}/builds/{build_number}/jobs/{job_id}"
            ));
        }

        let resp = resp
            .error_for_status()
            .context("Buildkite API returned an error for job log")?;

        resp.json::<BkLogResponse>()
            .await
            .context("Failed to parse job log response")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn sample_build_json() -> serde_json::Value {
        serde_json::json!({
            "number": 5939,
            "state": "failed",
            "branch": "feature-branch",
            "commit": "abc123",
            "message": "Fix the thing",
            "web_url": "https://buildkite.com/rokt/catalog-ci-pipeline/builds/5939",
            "jobs": [
                {
                    "id": "job-1",
                    "name": ":pytest: Run pytest (shard 1/20)",
                    "type": "script",
                    "state": "passed",
                    "exit_status": 0,
                    "soft_failed": false,
                    "web_url": "https://buildkite.com/rokt/catalog-ci-pipeline/builds/5939#job-1",
                    "log_url": "https://api.buildkite.com/v2/organizations/rokt/pipelines/catalog-ci-pipeline/builds/5939/jobs/job-1/log"
                },
                {
                    "id": "job-2",
                    "name": ":pytest: Run pytest (shard 2/20)",
                    "type": "script",
                    "state": "failed",
                    "exit_status": 1,
                    "soft_failed": false,
                    "web_url": "https://buildkite.com/rokt/catalog-ci-pipeline/builds/5939#job-2",
                    "log_url": "https://api.buildkite.com/v2/organizations/rokt/pipelines/catalog-ci-pipeline/builds/5939/jobs/job-2/log"
                },
                {
                    "id": "job-3",
                    "name": null,
                    "type": "waiter",
                    "state": null,
                    "exit_status": null,
                    "soft_failed": null,
                    "web_url": null,
                    "log_url": null
                }
            ]
        })
    }

    #[tokio::test]
    async fn test_get_build() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path(
                "/v2/organizations/rokt/pipelines/catalog-ci-pipeline/builds/5939",
            ))
            .and(header("authorization", "Bearer bk-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(sample_build_json()))
            .mount(&server)
            .await;

        let client = BuildkiteClient::with_base_url("bk-token".to_string(), server.uri());
        let build = client
            .get_build("rokt", "catalog-ci-pipeline", 5939)
            .await
            .unwrap();

        assert_eq!(build.number, 5939);
        assert_eq!(build.state, "failed");
        assert_eq!(build.jobs.len(), 3);
        assert_eq!(build.jobs[0].job_type, "script");
        assert_eq!(build.jobs[0].state.as_deref(), Some("passed"));
        assert_eq!(build.jobs[1].state.as_deref(), Some("failed"));
        assert_eq!(build.jobs[2].job_type, "waiter");
    }

    #[tokio::test]
    async fn test_get_build_403() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path(
                "/v2/organizations/rokt/pipelines/catalog-ci-pipeline/builds/5939",
            ))
            .respond_with(ResponseTemplate::new(403))
            .mount(&server)
            .await;

        let client = BuildkiteClient::with_base_url("bad-token".to_string(), server.uri());
        let result = client.get_build("rokt", "catalog-ci-pipeline", 5939).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Access denied"));
    }

    #[tokio::test]
    async fn test_get_build_404() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path(
                "/v2/organizations/rokt/pipelines/catalog-ci-pipeline/builds/99999",
            ))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let client = BuildkiteClient::with_base_url("bk-token".to_string(), server.uri());
        let result = client.get_build("rokt", "catalog-ci-pipeline", 99999).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[tokio::test]
    async fn test_get_job_log() {
        let server = MockServer::start().await;

        let log_body = serde_json::json!({
            "content": "Running tests...\nFAILED test_something\nImportError: No module named 'foo'",
            "size": 85
        });

        Mock::given(method("GET"))
            .and(path(
                "/v2/organizations/rokt/pipelines/catalog-ci-pipeline/builds/5939/jobs/job-2/log",
            ))
            .and(header("authorization", "Bearer bk-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&log_body))
            .mount(&server)
            .await;

        let client = BuildkiteClient::with_base_url("bk-token".to_string(), server.uri());
        let log = client
            .get_job_log("rokt", "catalog-ci-pipeline", 5939, "job-2")
            .await
            .unwrap();

        assert!(log.content.contains("ImportError"));
        assert_eq!(log.size, 85);
    }

    #[tokio::test]
    async fn test_get_job_log_not_found() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path(
                "/v2/organizations/rokt/pipelines/catalog-ci-pipeline/builds/5939/jobs/bad-id/log",
            ))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let client = BuildkiteClient::with_base_url("bk-token".to_string(), server.uri());
        let result = client
            .get_job_log("rokt", "catalog-ci-pipeline", 5939, "bad-id")
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_auth_header_sent() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/v2/organizations/o/pipelines/p/builds/1"))
            .and(header("authorization", "Bearer secret-bk-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "number": 1,
                "state": "passed",
                "branch": "main",
                "commit": "sha",
                "message": null,
                "web_url": "https://buildkite.com/o/p/builds/1",
                "jobs": []
            })))
            .mount(&server)
            .await;

        let client = BuildkiteClient::with_base_url("secret-bk-token".to_string(), server.uri());
        let result = client.get_build("o", "p", 1).await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_new_constructor() {
        let client = BuildkiteClient::new("token".to_string());
        assert_eq!(client.base_url, "https://api.buildkite.com");
    }
}
