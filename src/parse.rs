use anyhow::{anyhow, Result};
use url::Url;

#[derive(Debug, Clone, PartialEq)]
pub struct PrInfo {
    pub owner: String,
    pub repo: String,
    pub pr_number: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BkBuildInfo {
    pub org: String,
    pub pipeline: String,
    pub build_number: u64,
}

pub fn parse_pr_url(input: &str) -> Result<PrInfo> {
    let trimmed = input.trim_end_matches('/');
    let url = Url::parse(trimmed).map_err(|_| anyhow!("Invalid URL: {input}"))?;

    if url.host_str() != Some("github.com") {
        return Err(anyhow!("Not a GitHub URL: {input}"));
    }

    let segments: Vec<&str> = url
        .path_segments()
        .ok_or_else(|| anyhow!("No path in URL: {input}"))?
        .collect();

    if segments.len() < 4 || segments[2] != "pull" {
        return Err(anyhow!(
            "Expected format: https://github.com/OWNER/REPO/pull/NUMBER, got: {input}"
        ));
    }

    let pr_number: u64 = segments[3]
        .parse()
        .map_err(|_| anyhow!("Invalid PR number: {}", segments[3]))?;

    Ok(PrInfo {
        owner: segments[0].to_string(),
        repo: segments[1].to_string(),
        pr_number,
    })
}

pub fn parse_buildkite_url(input: &str) -> Result<BkBuildInfo> {
    let trimmed = input.trim_end_matches('/');
    let url = Url::parse(trimmed).map_err(|_| anyhow!("Invalid Buildkite URL: {input}"))?;

    if url.host_str() != Some("buildkite.com") {
        return Err(anyhow!("Not a Buildkite URL: {input}"));
    }

    let segments: Vec<&str> = url
        .path_segments()
        .ok_or_else(|| anyhow!("No path in Buildkite URL: {input}"))?
        .collect();

    if segments.len() < 4 || segments[2] != "builds" {
        return Err(anyhow!(
            "Expected format: https://buildkite.com/ORG/PIPELINE/builds/NUMBER, got: {input}"
        ));
    }

    let build_number: u64 = segments[3]
        .parse()
        .map_err(|_| anyhow!("Invalid build number: {}", segments[3]))?;

    Ok(BkBuildInfo {
        org: segments[0].to_string(),
        pipeline: segments[1].to_string(),
        build_number,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_pr_url_valid() {
        let result = parse_pr_url("https://github.com/ROKT/canal/pull/14908").unwrap();
        assert_eq!(
            result,
            PrInfo {
                owner: "ROKT".to_string(),
                repo: "canal".to_string(),
                pr_number: 14908,
            }
        );
    }

    #[test]
    fn test_parse_pr_url_trailing_slash() {
        let result = parse_pr_url("https://github.com/ROKT/canal/pull/14908/").unwrap();
        assert_eq!(result.pr_number, 14908);
    }

    #[test]
    fn test_parse_pr_url_invalid_host() {
        assert!(parse_pr_url("https://gitlab.com/ROKT/canal/pull/14908").is_err());
    }

    #[test]
    fn test_parse_pr_url_missing_pull() {
        assert!(parse_pr_url("https://github.com/ROKT/canal/issues/14908").is_err());
    }

    #[test]
    fn test_parse_pr_url_not_a_url() {
        assert!(parse_pr_url("not-a-url").is_err());
    }

    #[test]
    fn test_parse_pr_url_invalid_number() {
        assert!(parse_pr_url("https://github.com/ROKT/canal/pull/abc").is_err());
    }

    #[test]
    fn test_parse_buildkite_url_valid() {
        let result =
            parse_buildkite_url("https://buildkite.com/rokt/catalog-ci-pipeline/builds/5939")
                .unwrap();
        assert_eq!(
            result,
            BkBuildInfo {
                org: "rokt".to_string(),
                pipeline: "catalog-ci-pipeline".to_string(),
                build_number: 5939,
            }
        );
    }

    #[test]
    fn test_parse_buildkite_url_trailing_slash() {
        let result =
            parse_buildkite_url("https://buildkite.com/rokt/catalog-ci-pipeline/builds/5939/")
                .unwrap();
        assert_eq!(result.build_number, 5939);
    }

    #[test]
    fn test_parse_buildkite_url_invalid_host() {
        assert!(parse_buildkite_url("https://example.com/rokt/pipeline/builds/1").is_err());
    }

    #[test]
    fn test_parse_buildkite_url_missing_builds() {
        assert!(parse_buildkite_url("https://buildkite.com/rokt/pipeline/jobs/1").is_err());
    }

    #[test]
    fn test_parse_buildkite_url_invalid_number() {
        assert!(parse_buildkite_url("https://buildkite.com/rokt/pipeline/builds/abc").is_err());
    }
}
