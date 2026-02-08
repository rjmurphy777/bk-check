use anyhow::{anyhow, Context, Result};
use reqwest::Client;
use serde::Deserialize;

const GITHUB_API_BASE: &str = "https://api.github.com";

pub struct GitHubClient {
    client: Client,
    base_url: String,
    token: String,
}

#[derive(Debug, Deserialize)]
pub struct PrResponse {
    pub head: PrHead,
}

#[derive(Debug, Deserialize)]
pub struct PrHead {
    #[serde(rename = "ref")]
    pub ref_name: String,
    pub sha: String,
}

#[derive(Debug, Deserialize)]
pub struct CommitStatus {
    pub state: String,
    pub statuses: Vec<StatusEntry>,
}

#[derive(Debug, Deserialize)]
pub struct StatusEntry {
    pub context: String,
    pub state: String,
    pub target_url: Option<String>,
    pub description: Option<String>,
}

impl GitHubClient {
    pub fn new(token: String) -> Self {
        Self::with_base_url(token, GITHUB_API_BASE.to_string())
    }

    pub fn with_base_url(token: String, base_url: String) -> Self {
        let client = Client::builder()
            .user_agent("bk-check/0.1")
            .build()
            .expect("Failed to build HTTP client");
        GitHubClient {
            client,
            base_url,
            token,
        }
    }

    pub async fn get_pr(&self, owner: &str, repo: &str, pr_number: u64) -> Result<PrResponse> {
        let url = format!(
            "{}/repos/{}/{}/pulls/{}",
            self.base_url, owner, repo, pr_number
        );
        let resp = self
            .client
            .get(&url)
            .bearer_auth(&self.token)
            .send()
            .await
            .context("Failed to reach GitHub API")?;

        if resp.status() == 404 {
            return Err(anyhow!("PR not found: {owner}/{repo}/pull/{pr_number}"));
        }

        let resp = resp
            .error_for_status()
            .context("GitHub API returned an error")?;

        resp.json::<PrResponse>()
            .await
            .context("Failed to parse PR response")
    }

    pub async fn get_commit_status(
        &self,
        owner: &str,
        repo: &str,
        sha: &str,
    ) -> Result<CommitStatus> {
        let url = format!(
            "{}/repos/{}/{}/commits/{}/status",
            self.base_url, owner, repo, sha
        );
        let resp = self
            .client
            .get(&url)
            .bearer_auth(&self.token)
            .send()
            .await
            .context("Failed to reach GitHub API for commit status")?;

        let resp = resp
            .error_for_status()
            .context("GitHub API returned an error for commit status")?;

        resp.json::<CommitStatus>()
            .await
            .context("Failed to parse commit status response")
    }
}

pub fn resolve_github_token_with(
    env_token: Option<String>,
    gh_fallback: impl FnOnce() -> Result<String>,
) -> Result<String> {
    if let Some(token) = env_token {
        if !token.is_empty() {
            return Ok(token);
        }
    }

    let token = gh_fallback()?.trim().to_string();

    if token.is_empty() {
        return Err(anyhow!(
            "No GitHub token found. Set GITHUB_TOKEN or run 'gh auth login'"
        ));
    }

    Ok(token)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn test_new_constructor() {
        let client = GitHubClient::new("token".to_string());
        assert_eq!(client.base_url, "https://api.github.com");
    }

    #[tokio::test]
    async fn test_get_pr() {
        let server = MockServer::start().await;

        let body = serde_json::json!({
            "head": {
                "ref": "feature-branch",
                "sha": "abc123def456"
            }
        });

        Mock::given(method("GET"))
            .and(path("/repos/ROKT/canal/pulls/14908"))
            .and(header("authorization", "Bearer test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .mount(&server)
            .await;

        let client = GitHubClient::with_base_url("test-token".to_string(), server.uri());
        let pr = client.get_pr("ROKT", "canal", 14908).await.unwrap();
        assert_eq!(pr.head.ref_name, "feature-branch");
        assert_eq!(pr.head.sha, "abc123def456");
    }

    #[tokio::test]
    async fn test_get_pr_not_found() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/repos/ROKT/canal/pulls/99999"))
            .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
                "message": "Not Found"
            })))
            .mount(&server)
            .await;

        let client = GitHubClient::with_base_url("test-token".to_string(), server.uri());
        let result = client.get_pr("ROKT", "canal", 99999).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("PR not found"));
    }

    #[tokio::test]
    async fn test_get_commit_status() {
        let server = MockServer::start().await;

        let body = serde_json::json!({
            "state": "failure",
            "statuses": [
                {
                    "context": "buildkite/catalog-ci-pipeline",
                    "state": "failure",
                    "target_url": "https://buildkite.com/rokt/catalog-ci-pipeline/builds/5939",
                    "description": "Build #5939 failed"
                }
            ]
        });

        Mock::given(method("GET"))
            .and(path("/repos/ROKT/canal/commits/abc123/status"))
            .and(header("authorization", "Bearer test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .mount(&server)
            .await;

        let client = GitHubClient::with_base_url("test-token".to_string(), server.uri());
        let status = client
            .get_commit_status("ROKT", "canal", "abc123")
            .await
            .unwrap();
        assert_eq!(status.state, "failure");
        assert_eq!(status.statuses.len(), 1);
        assert_eq!(status.statuses[0].context, "buildkite/catalog-ci-pipeline");
        assert_eq!(
            status.statuses[0].target_url.as_deref(),
            Some("https://buildkite.com/rokt/catalog-ci-pipeline/builds/5939")
        );
    }

    #[tokio::test]
    async fn test_get_commit_status_success() {
        let server = MockServer::start().await;

        let body = serde_json::json!({
            "state": "success",
            "statuses": [
                {
                    "context": "buildkite/catalog-ci-pipeline",
                    "state": "success",
                    "target_url": "https://buildkite.com/rokt/catalog-ci-pipeline/builds/5959",
                    "description": "Build #5959 passed"
                }
            ]
        });

        Mock::given(method("GET"))
            .and(path("/repos/ROKT/canal/commits/def456/status"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .mount(&server)
            .await;

        let client = GitHubClient::with_base_url("test-token".to_string(), server.uri());
        let status = client
            .get_commit_status("ROKT", "canal", "def456")
            .await
            .unwrap();
        assert_eq!(status.state, "success");
        assert_eq!(status.statuses[0].state, "success");
    }

    #[tokio::test]
    async fn test_auth_header_sent() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/repos/ROKT/canal/pulls/1"))
            .and(header("authorization", "Bearer my-secret-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "head": { "ref": "b", "sha": "s" }
            })))
            .mount(&server)
            .await;

        let client = GitHubClient::with_base_url("my-secret-token".to_string(), server.uri());
        let result = client.get_pr("ROKT", "canal", 1).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_get_pr_network_error() {
        let client =
            GitHubClient::with_base_url("token".to_string(), "http://127.0.0.1:1".to_string());
        let result = client.get_pr("ROKT", "canal", 1).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Failed to reach GitHub API"));
    }

    #[tokio::test]
    async fn test_get_commit_status_network_error() {
        let client =
            GitHubClient::with_base_url("token".to_string(), "http://127.0.0.1:1".to_string());
        let result = client.get_commit_status("ROKT", "canal", "sha").await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Failed to reach GitHub API"));
    }

    #[tokio::test]
    async fn test_get_pr_server_error() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/repos/ROKT/canal/pulls/1"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let client = GitHubClient::with_base_url("token".to_string(), server.uri());
        let result = client.get_pr("ROKT", "canal", 1).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_get_commit_status_server_error() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/repos/ROKT/canal/commits/sha/status"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let client = GitHubClient::with_base_url("token".to_string(), server.uri());
        let result = client.get_commit_status("ROKT", "canal", "sha").await;
        assert!(result.is_err());
    }

    #[test]
    fn test_resolve_token_from_env() {
        let result = resolve_github_token_with(Some("my-token".to_string()), || unreachable!());
        assert_eq!(result.unwrap(), "my-token");
    }

    #[test]
    fn test_resolve_token_empty_env_falls_back() {
        let result =
            resolve_github_token_with(Some("".to_string()), || Ok("gh-token\n".to_string()));
        assert_eq!(result.unwrap(), "gh-token");
    }

    #[test]
    fn test_resolve_token_no_env_falls_back() {
        let result = resolve_github_token_with(None, || Ok("fallback-token\n".to_string()));
        assert_eq!(result.unwrap(), "fallback-token");
    }

    #[test]
    fn test_resolve_token_fallback_empty() {
        let result = resolve_github_token_with(None, || Ok("  \n".to_string()));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No GitHub token"));
    }

    #[test]
    fn test_resolve_token_fallback_error() {
        let result = resolve_github_token_with(None, || Err(anyhow!("gh not found")));
        assert!(result.is_err());
    }
}
