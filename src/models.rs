use serde::Serialize;

use crate::log_parser;

#[derive(Debug, Serialize)]
pub struct BuildReport {
    pub pr_url: String,
    pub branch: String,
    pub commit: String,
    pub overall_status: String,
    pub build_url: String,
    pub passed_jobs: Vec<JobSummary>,
    pub failed_jobs: Vec<FailedJobGroup>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct JobSummary {
    pub name: String,
    pub state: String,
}

/// A single failed job before grouping (internal use).
#[derive(Debug, Clone)]
pub struct FailedJob {
    pub name: String,
    pub state: String,
    pub exit_status: Option<i32>,
    pub web_url: Option<String>,
    pub failure_log: String,
}

/// A group of failed jobs that share the same (or very similar) failure log.
/// When only one job has a particular failure, `jobs` has a single entry.
#[derive(Debug, Serialize, Clone)]
pub struct FailedJobGroup {
    pub jobs: Vec<FailedJobInfo>,
    pub failure_log: String,
}

/// Job metadata within a group (everything except the log, which is shared).
#[derive(Debug, Serialize, Clone)]
pub struct FailedJobInfo {
    pub name: String,
    pub state: String,
    pub exit_status: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub web_url: Option<String>,
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

        let grouped = group_failed_jobs(failed_jobs);

        BuildReport {
            pr_url,
            branch,
            commit,
            overall_status,
            build_url,
            passed_jobs,
            failed_jobs: grouped,
            warnings,
        }
    }
}

/// Group failed jobs by normalized failure log content.
/// Jobs with identical errors (modulo shard numbers, timings, etc.) are collapsed
/// into a single group with one copy of the failure log and a list of affected jobs.
fn group_failed_jobs(jobs: Vec<FailedJob>) -> Vec<FailedJobGroup> {
    use std::collections::BTreeMap;

    // Use BTreeMap to maintain insertion order by first occurrence
    let mut groups: BTreeMap<String, (String, Vec<FailedJobInfo>)> = BTreeMap::new();
    let mut key_order: Vec<String> = Vec::new();

    for job in jobs {
        let key = log_parser::normalize_for_grouping(&job.failure_log);
        let info = FailedJobInfo {
            name: job.name,
            state: job.state,
            exit_status: job.exit_status,
            web_url: job.web_url,
        };

        if let Some((_log, infos)) = groups.get_mut(&key) {
            infos.push(info);
        } else {
            key_order.push(key.clone());
            groups.insert(key, (job.failure_log, vec![info]));
        }
    }

    key_order
        .into_iter()
        .filter_map(|key| {
            groups
                .remove(&key)
                .map(|(failure_log, jobs)| FailedJobGroup { jobs, failure_log })
        })
        .collect()
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
        assert_eq!(report.failed_jobs.len(), 1);
        assert_eq!(report.failed_jobs[0].jobs.len(), 1);
        assert_eq!(report.failed_jobs[0].jobs[0].name, "pytest shard 1");
        assert!(report.failed_jobs[0].failure_log.contains("ImportError"));
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
        assert_eq!(report.failed_jobs[0].jobs.len(), 1);
    }

    #[test]
    fn test_dedup_groups_identical_failures() {
        let report = BuildReport::new(
            "url".to_string(),
            "branch".to_string(),
            "sha".to_string(),
            "build_url".to_string(),
            vec![],
            vec![
                FailedJob {
                    name: "pytest shard 1/20".to_string(),
                    state: "failed".to_string(),
                    exit_status: Some(2),
                    web_url: Some("https://bk.com/1".to_string()),
                    failure_log: "ModuleNotFoundError: No module named 'scrapfly'".to_string(),
                },
                FailedJob {
                    name: "pytest shard 2/20".to_string(),
                    state: "failed".to_string(),
                    exit_status: Some(2),
                    web_url: Some("https://bk.com/2".to_string()),
                    failure_log: "ModuleNotFoundError: No module named 'scrapfly'".to_string(),
                },
                FailedJob {
                    name: "pytest shard 3/20".to_string(),
                    state: "failed".to_string(),
                    exit_status: Some(2),
                    web_url: Some("https://bk.com/3".to_string()),
                    failure_log: "ModuleNotFoundError: No module named 'scrapfly'".to_string(),
                },
            ],
            vec![],
        );

        // All 3 shards should be grouped into 1 group
        assert_eq!(report.failed_jobs.len(), 1);
        assert_eq!(report.failed_jobs[0].jobs.len(), 3);
        assert_eq!(report.failed_jobs[0].jobs[0].name, "pytest shard 1/20");
        assert_eq!(report.failed_jobs[0].jobs[1].name, "pytest shard 2/20");
        assert_eq!(report.failed_jobs[0].jobs[2].name, "pytest shard 3/20");
        assert!(report.failed_jobs[0]
            .failure_log
            .contains("ModuleNotFoundError"));
    }

    #[test]
    fn test_dedup_groups_similar_failures_with_shard_numbers() {
        let report = BuildReport::new(
            "url".to_string(),
            "branch".to_string(),
            "sha".to_string(),
            "build_url".to_string(),
            vec![],
            vec![
                FailedJob {
                    name: "shard 1/20".to_string(),
                    state: "failed".to_string(),
                    exit_status: Some(2),
                    web_url: None,
                    failure_log: "Error in shard 1/20 after 59.72s\ncanal-coverage-1.xml"
                        .to_string(),
                },
                FailedJob {
                    name: "shard 19/20".to_string(),
                    state: "failed".to_string(),
                    exit_status: Some(2),
                    web_url: None,
                    failure_log: "Error in shard 19/20 after 56.74s\ncanal-coverage-19.xml"
                        .to_string(),
                },
            ],
            vec![],
        );

        // Different shard numbers but same error pattern → grouped
        assert_eq!(report.failed_jobs.len(), 1);
        assert_eq!(report.failed_jobs[0].jobs.len(), 2);
    }

    #[test]
    fn test_dedup_keeps_different_failures_separate() {
        let report = BuildReport::new(
            "url".to_string(),
            "branch".to_string(),
            "sha".to_string(),
            "build_url".to_string(),
            vec![],
            vec![
                FailedJob {
                    name: "test".to_string(),
                    state: "failed".to_string(),
                    exit_status: Some(1),
                    web_url: None,
                    failure_log: "ImportError: cannot import 'foo'".to_string(),
                },
                FailedJob {
                    name: "lint".to_string(),
                    state: "failed".to_string(),
                    exit_status: Some(1),
                    web_url: None,
                    failure_log: "SyntaxError: unexpected indent".to_string(),
                },
            ],
            vec![],
        );

        // Different errors → separate groups
        assert_eq!(report.failed_jobs.len(), 2);
        assert_eq!(report.failed_jobs[0].jobs.len(), 1);
        assert_eq!(report.failed_jobs[1].jobs.len(), 1);
    }

    #[test]
    fn test_dedup_serialization_structure() {
        let report = BuildReport::new(
            "url".to_string(),
            "branch".to_string(),
            "sha".to_string(),
            "build_url".to_string(),
            vec![],
            vec![
                FailedJob {
                    name: "shard 1".to_string(),
                    state: "failed".to_string(),
                    exit_status: Some(2),
                    web_url: Some("https://bk.com/1".to_string()),
                    failure_log: "same error".to_string(),
                },
                FailedJob {
                    name: "shard 2".to_string(),
                    state: "failed".to_string(),
                    exit_status: Some(2),
                    web_url: Some("https://bk.com/2".to_string()),
                    failure_log: "same error".to_string(),
                },
            ],
            vec![],
        );

        let json = serde_json::to_string_pretty(&report).unwrap();
        // Should have "jobs" array within failed_jobs
        assert!(json.contains("\"jobs\""));
        // Should have failure_log once, not twice
        let log_count = json.matches("\"failure_log\"").count();
        assert_eq!(log_count, 1);
    }
}
