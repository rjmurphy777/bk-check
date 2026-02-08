use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct BuildReport {
    pub pr_url: String,
    pub branch: String,
    pub commit: String,
    pub overall_status: String,
    pub build_url: String,
    pub passed_jobs: Vec<JobSummary>,
    pub failed_jobs: Vec<FailedJob>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct JobSummary {
    pub name: String,
    pub state: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct FailedJob {
    pub name: String,
    pub state: String,
    pub exit_status: Option<i32>,
    pub web_url: Option<String>,
    pub failure_log: String,
}

impl BuildReport {
    pub fn new(
        pr_url: String,
        branch: String,
        commit: String,
        build_url: String,
        passed_jobs: Vec<JobSummary>,
        failed_jobs: Vec<FailedJob>,
        warnings: Vec<String>,
    ) -> Self {
        let overall_status = if failed_jobs.is_empty() {
            "success".to_string()
        } else {
            "failure".to_string()
        };

        BuildReport {
            pr_url,
            branch,
            commit,
            overall_status,
            build_url,
            passed_jobs,
            failed_jobs,
            warnings,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_report_all_passed() {
        let report = BuildReport::new(
            "https://github.com/ROKT/canal/pull/14916".to_string(),
            "main".to_string(),
            "abc123".to_string(),
            "https://buildkite.com/rokt/pipeline/builds/1".to_string(),
            vec![JobSummary {
                name: "test".to_string(),
                state: "passed".to_string(),
            }],
            vec![],
            vec![],
        );
        assert_eq!(report.overall_status, "success");
    }

    #[test]
    fn test_build_report_with_failures() {
        let report = BuildReport::new(
            "https://github.com/ROKT/canal/pull/14908".to_string(),
            "feature".to_string(),
            "def456".to_string(),
            "https://buildkite.com/rokt/pipeline/builds/2".to_string(),
            vec![],
            vec![FailedJob {
                name: "pytest shard 1".to_string(),
                state: "failed".to_string(),
                exit_status: Some(1),
                web_url: None,
                failure_log: "ImportError: no module".to_string(),
            }],
            vec![],
        );
        assert_eq!(report.overall_status, "failure");
    }

    #[test]
    fn test_build_report_serialization() {
        let report = BuildReport::new(
            "https://github.com/ROKT/canal/pull/1".to_string(),
            "main".to_string(),
            "abc".to_string(),
            "https://buildkite.com/rokt/p/builds/1".to_string(),
            vec![JobSummary {
                name: "lint".to_string(),
                state: "passed".to_string(),
            }],
            vec![],
            vec![],
        );
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"overall_status\":\"success\""));
        // warnings should be omitted when empty
        assert!(!json.contains("warnings"));
    }

    #[test]
    fn test_build_report_serialization_with_warnings() {
        let report = BuildReport::new(
            "https://github.com/ROKT/canal/pull/1".to_string(),
            "main".to_string(),
            "abc".to_string(),
            "https://buildkite.com/rokt/p/builds/1".to_string(),
            vec![],
            vec![],
            vec!["Some warning".to_string()],
        );
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("warnings"));
    }

    #[test]
    fn test_mixed_jobs_report() {
        let report = BuildReport::new(
            "url".to_string(),
            "branch".to_string(),
            "sha".to_string(),
            "build_url".to_string(),
            vec![
                JobSummary {
                    name: "lint".to_string(),
                    state: "passed".to_string(),
                },
                JobSummary {
                    name: "build".to_string(),
                    state: "passed".to_string(),
                },
            ],
            vec![FailedJob {
                name: "test".to_string(),
                state: "failed".to_string(),
                exit_status: Some(2),
                web_url: Some("https://example.com".to_string()),
                failure_log: "assertion failed".to_string(),
            }],
            vec![],
        );
        assert_eq!(report.overall_status, "failure");
        assert_eq!(report.passed_jobs.len(), 2);
        assert_eq!(report.failed_jobs.len(), 1);
    }
}
