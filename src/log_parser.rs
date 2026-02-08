use regex::Regex;

const FAILURE_MARKERS: &[&str] = &[
    "ERRORS",
    "FAILED",
    "FAILURES",
    "Error:",
    "error:",
    "Traceback (most recent call last)",
    "ModuleNotFoundError",
    "ImportError",
    "AssertionError",
    "AssertError",
    "panic!",
    "PANIC",
    "thread '",
    "fatal:",
    "Exception:",
];

/// Lines starting with these prefixes indicate Buildkite infrastructure noise
/// (artifact uploads, hooks, docker cleanup) that should be stripped.
const NOISE_PREFIXES: &[&str] = &[
    "^^^ +++",
    "~~~ Uploading artifacts",
    "~~~ Running global pre-exit hook",
    "~~~ Running plugin",
    "~~~ :docker:",
    "~~~ Stopping ssh-agent",
    "~~~ Agent pre-exit hook",
    "+++ :warning: Failed to run command",
    "$ buildkite-agent artifact",
    "$ docker compose",
    "$ /var/lib/buildkite-agent",
    "$ /etc/buildkite-agent",
];

pub fn clean_log(raw: &str, max_lines: usize) -> String {
    let stripped = strip_ansi(raw);
    let stripped = strip_timestamps(&stripped);
    let stripped = strip_buildkite_noise(&stripped);
    let lines: Vec<&str> = stripped.lines().collect();

    extract_failure_section(&lines, max_lines)
}

fn strip_ansi(input: &str) -> String {
    let re = Regex::new(r"\x1b\[[0-9;]*[a-zA-Z]").unwrap();
    re.replace_all(input, "").to_string()
}

fn strip_timestamps(input: &str) -> String {
    let re = Regex::new(r"(?m)^\[\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z\]\s?").unwrap();
    re.replace_all(input, "").to_string()
}

/// Remove Buildkite infrastructure noise: once we hit a noise section,
/// drop everything until we see a non-noise line that looks like test output again.
/// Also drops isolated noise lines scattered through the log.
fn strip_buildkite_noise(input: &str) -> String {
    let mut result = Vec::new();
    let mut in_noise_section = false;

    for line in input.lines() {
        let trimmed = line.trim();

        if is_noise_line(trimmed) {
            in_noise_section = true;
            continue;
        }

        // Once in a noise section, only exit if we see a substantive test line
        if in_noise_section {
            if is_test_output(trimmed) {
                in_noise_section = false;
            } else {
                continue;
            }
        }

        result.push(line);
    }

    result.join("\n")
}

fn is_noise_line(line: &str) -> bool {
    for prefix in NOISE_PREFIXES {
        if line.starts_with(prefix) {
            return true;
        }
    }
    // Docker compose output with container/network status
    if line.contains("[+] Killing") || line.contains("[+] Removing") || line.contains("[+] Running")
    {
        return true;
    }
    // Container status lines (✔ Container ..., ⠋ Container ...)
    if line.contains("Container buildkite") || line.contains("Network buildkite") {
        return true;
    }
    // Buildkite agent log lines with timestamps
    if line.starts_with("2")
        && (line.contains("INFO   Found")
            || line.contains("INFO   Uploading")
            || line.contains("INFO   Successfully")
            || line.contains("INFO   Creating")
            || line.contains("INFO   Artifact"))
    {
        return true;
    }
    // SSH agent / DataDog / K8s cleanup lines
    if line.starts_with("Agent pid")
        || line.starts_with("# SSH_")
        || line == "{}#"
        || line.starts_with("{}")
    {
        return true;
    }
    if line.contains("EC2 region:")
        || line.contains("Get EC2 instance")
        || line.contains("Get Datadog API key")
        || line.contains("Sending Docker storage usage")
    {
        return true;
    }
    if line.contains(".kube folder not detected") || line.contains("cleanup skipped") {
        return true;
    }
    // WARN lines from docker compose about obsolete version
    if line.starts_with("WARN[") && line.contains("version") && line.contains("obsolete") {
        return true;
    }
    // "Going to remove" from docker rm
    if line.starts_with("Going to remove buildkite") {
        return true;
    }
    // user command error lines from Buildkite
    if line.starts_with("user command error:") {
        return true;
    }
    false
}

/// Check if a line looks like actual test/build output (not infrastructure).
fn is_test_output(line: &str) -> bool {
    if line.is_empty() {
        return false;
    }
    // Known test output patterns
    for marker in FAILURE_MARKERS {
        if line.contains(marker) {
            return true;
        }
    }
    // Python test output patterns
    if line.starts_with("E ")
        || line.starts_with("  ")
        || line.contains("test")
        || line.contains("assert")
    {
        return true;
    }
    false
}

/// Normalize a failure log for grouping purposes.
/// Strips variable parts (shard numbers, timings, file paths with numbers)
/// so that identical errors across shards compare as equal.
pub fn normalize_for_grouping(log: &str) -> String {
    let re_shard = Regex::new(r"shard \d+/\d+").unwrap();
    let re_timing = Regex::new(r"\d+\.\d+s").unwrap();
    let re_coverage_file = Regex::new(r"canal-coverage-\d+\.xml").unwrap();
    let re_group = Regex::new(r"--group \d+").unwrap();

    let s = re_shard.replace_all(log, "shard N/N");
    let s = re_timing.replace_all(&s, "N.Ns");
    let s = re_coverage_file.replace_all(&s, "canal-coverage-N.xml");
    let s = re_group.replace_all(&s, "--group N");
    s.to_string()
}

fn extract_failure_section(lines: &[&str], max_lines: usize) -> String {
    // Find the last failure marker position
    let mut best_marker_idx: Option<usize> = None;
    for (i, line) in lines.iter().enumerate() {
        for marker in FAILURE_MARKERS {
            if line.contains(marker) {
                best_marker_idx = Some(i);
                break;
            }
        }
    }

    let selected = if let Some(marker_idx) = best_marker_idx {
        // Take lines around the marker, biased toward showing content after it
        let context_before = max_lines / 4;
        let start = marker_idx.saturating_sub(context_before);
        let end = (start + max_lines).min(lines.len());
        &lines[start..end]
    } else {
        // No marker found: take last max_lines lines
        let start = lines.len().saturating_sub(max_lines);
        &lines[start..]
    };

    selected.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_ansi() {
        let input = "\x1b[31mERROR\x1b[0m: something failed";
        assert_eq!(strip_ansi(input), "ERROR: something failed");
    }

    #[test]
    fn test_strip_ansi_multiple() {
        let input = "\x1b[1;32mOK\x1b[0m and \x1b[33mWARN\x1b[0m";
        assert_eq!(strip_ansi(input), "OK and WARN");
    }

    #[test]
    fn test_strip_timestamps() {
        let input = "[2026-02-07T19:39:15Z] Some log line\n[2026-02-07T19:39:16Z] Another line";
        let result = strip_timestamps(input);
        assert_eq!(result, "Some log line\nAnother line");
    }

    #[test]
    fn test_strip_timestamps_no_timestamps() {
        let input = "No timestamps here";
        assert_eq!(strip_timestamps(input), "No timestamps here");
    }

    #[test]
    fn test_extract_with_traceback() {
        let mut lines: Vec<String> = (0..50).map(|i| format!("normal line {i}")).collect();
        lines.push("Traceback (most recent call last):".to_string());
        lines.push("  File \"test.py\", line 10".to_string());
        lines.push("ModuleNotFoundError: No module named 'foo'".to_string());
        for i in 0..10 {
            lines.push(format!("after error {i}"));
        }

        let line_refs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
        let result = extract_failure_section(&line_refs, 20);

        assert!(result.contains("Traceback (most recent call last)"));
        assert!(result.contains("ModuleNotFoundError"));
    }

    #[test]
    fn test_extract_no_markers_takes_tail() {
        let lines: Vec<String> = (0..200).map(|i| format!("line {i}")).collect();
        let line_refs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
        let result = extract_failure_section(&line_refs, 50);

        assert!(result.contains("line 199"));
        assert!(result.contains("line 150"));
        assert!(!result.contains("line 149"));
    }

    #[test]
    fn test_extract_short_log() {
        let lines = vec!["line 1", "line 2", "line 3"];
        let result = extract_failure_section(&lines, 100);
        assert_eq!(result, "line 1\nline 2\nline 3");
    }

    #[test]
    fn test_clean_log_full_pipeline() {
        let raw = "\x1b[31m[2026-02-07T19:39:15Z] normal line\x1b[0m\n\
                   \x1b[31m[2026-02-07T19:39:16Z] Traceback (most recent call last):\x1b[0m\n\
                   \x1b[31m[2026-02-07T19:39:17Z]   File \"test.py\", line 10\x1b[0m\n\
                   \x1b[31m[2026-02-07T19:39:18Z] ImportError: cannot import 'foo'\x1b[0m";
        let result = clean_log(raw, 100);
        assert!(result.contains("Traceback (most recent call last)"));
        assert!(result.contains("ImportError: cannot import 'foo'"));
        assert!(!result.contains("\x1b["));
        assert!(!result.contains("[2026-02-07"));
    }

    #[test]
    fn test_max_lines_cap() {
        let mut lines: Vec<String> = Vec::new();
        for i in 0..500 {
            lines.push(format!("line {i}"));
        }
        lines.push("FAILED test case".to_string());
        for i in 0..500 {
            lines.push(format!("after {i}"));
        }

        let line_refs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
        let result = extract_failure_section(&line_refs, 100);
        let result_lines: Vec<&str> = result.lines().collect();
        assert!(result_lines.len() <= 100);
    }

    #[test]
    fn test_extract_uses_last_marker() {
        let lines = vec![
            "early error: something",
            "lots of normal output",
            "more normal output",
            "FAILED final test",
            "details about final failure",
        ];
        let result = extract_failure_section(&lines, 10);
        assert!(result.contains("FAILED final test"));
        assert!(result.contains("details about final failure"));
    }

    #[test]
    fn test_strip_buildkite_noise_removes_artifact_upload() {
        let input = "FAILED test_something\n\
                     ^^^ +++\n\
                     ~~~ Uploading artifacts\n\
                     $ buildkite-agent artifact upload coverage.xml\n\
                     2026-02-07 19:40:31 INFO   Found 1 files that match\n\
                     2026-02-07 19:40:32 INFO   Uploading artifact\n\
                     2026-02-07 19:40:32 INFO   Successfully uploaded";
        let result = strip_buildkite_noise(input);
        assert!(result.contains("FAILED test_something"));
        assert!(!result.contains("Uploading artifacts"));
        assert!(!result.contains("buildkite-agent"));
    }

    #[test]
    fn test_strip_buildkite_noise_removes_docker_cleanup() {
        let input = "Error: ModuleNotFoundError\n\
                     ^^^ +++\n\
                     ~~~ :docker: Cleaning up after docker-compose\n\
                     $ docker compose -f docker-compose.yml kill\n\
                     [+] Killing 0/2\n\
                     Container buildkite019c-redis-1  Killing\n\
                     [+] Killing 2/2\n\
                     Going to remove buildkite019c-postgres-1";
        let result = strip_buildkite_noise(input);
        assert!(result.contains("Error: ModuleNotFoundError"));
        assert!(!result.contains("docker compose"));
        assert!(!result.contains("Killing"));
        assert!(!result.contains("Going to remove"));
    }

    #[test]
    fn test_strip_buildkite_noise_removes_pre_exit_hooks() {
        let input = "test failed\n\
                     ~~~ Running global pre-exit hook\n\
                     $ /etc/buildkite-agent/hooks/pre-exit\n\
                     ~~~ Stopping ssh-agent 4346\n\
                     Agent pid 4346 killed\n\
                     # SSH_AGENT_PID removed\n\
                     # SSH_AUTH_SOCK removed\n\
                     ~~~ Agent pre-exit hook: Cleanup Kubernetes config\n\
                     /var/lib/buildkite-agent/.kube folder not detected. cleanup skipped.";
        let result = strip_buildkite_noise(input);
        assert!(result.contains("test failed"));
        assert!(!result.contains("pre-exit"));
        assert!(!result.contains("SSH_AGENT_PID"));
        assert!(!result.contains(".kube folder"));
    }

    #[test]
    fn test_strip_buildkite_noise_preserves_test_output() {
        let input = "Running tests...\n\
                     Traceback (most recent call last):\n\
                     File \"test.py\", line 10\n\
                     ModuleNotFoundError: No module named 'foo'\n\
                     \n\
                     === short test summary info ===\n\
                     FAILED tests/test_foo.py::test_bar";
        let result = strip_buildkite_noise(input);
        assert_eq!(result, input);
    }

    #[test]
    fn test_clean_log_strips_noise_before_extraction() {
        let mut raw = String::new();
        raw.push_str("Running tests...\n");
        raw.push_str("ModuleNotFoundError: No module named 'foo'\n");
        raw.push_str("=== short test summary info ===\n");
        raw.push_str("FAILED tests/test_foo.py\n");
        raw.push_str("^^^ +++\n");
        // Add 200 lines of infrastructure noise
        for i in 0..200 {
            raw.push_str(&format!("$ docker compose cleanup line {i}\n"));
        }
        let result = clean_log(&raw, 50);
        assert!(result.contains("ModuleNotFoundError"));
        assert!(!result.contains("docker compose"));
        // Should be much shorter than 200 lines since noise was stripped
        assert!(result.lines().count() <= 50);
    }

    #[test]
    fn test_strip_noise_recovers_on_test_output() {
        // After entering a noise section, test output with a failure marker should be kept
        let input = "first test line\n\
                     ^^^ +++\n\
                     some noise line\n\
                     another noise line\n\
                     FAILED tests/test_bar.py::test_baz - AssertionError";
        let result = strip_buildkite_noise(input);
        assert!(result.contains("first test line"));
        assert!(result.contains("FAILED tests/test_bar.py"));
        assert!(!result.contains("some noise line"));
    }

    #[test]
    fn test_strip_noise_recovers_on_python_patterns() {
        // After noise, python-style test output should be kept
        let input = "error output\n\
                     ^^^ +++\n\
                     random infra line\n\
                     E   ModuleNotFoundError: no module";
        let result = strip_buildkite_noise(input);
        assert!(result.contains("error output"));
        assert!(result.contains("E   ModuleNotFoundError"));
        assert!(!result.contains("random infra line"));
    }

    #[test]
    fn test_strip_noise_skips_non_test_lines_in_noise_section() {
        // Non-test lines after noise starts should stay suppressed
        let input = "real output\n\
                     ^^^ +++\n\
                     random unknown line\n\
                     another unknown line";
        let result = strip_buildkite_noise(input);
        assert!(result.contains("real output"));
        assert!(!result.contains("random unknown line"));
        assert!(!result.contains("another unknown line"));
    }

    #[test]
    fn test_strip_noise_info_timestamp_lines() {
        let input = "test output\n\
                     2026-02-07 19:40:31 INFO   Found 1 files that match\n\
                     2026-02-07 19:40:32 INFO   Uploading artifact foo\n\
                     2026-02-07 19:40:32 INFO   Successfully uploaded\n\
                     2026-02-07 19:40:33 INFO   Creating artifacts\n\
                     2026-02-07 19:40:34 INFO   Artifact uploads done\n\
                     more test output";
        let result = strip_buildkite_noise(input);
        assert!(result.contains("test output"));
        assert!(!result.contains("INFO   Found"));
        assert!(!result.contains("INFO   Uploading"));
        assert!(!result.contains("INFO   Successfully"));
        assert!(!result.contains("INFO   Creating"));
        assert!(!result.contains("INFO   Artifact"));
    }

    #[test]
    fn test_strip_noise_json_and_ssh_lines() {
        let input = "test output\n\
                     {}# SSH_AGENT_PID removed\n\
                     {}\n\
                     Agent pid 1234 killed\n\
                     # SSH_AUTH_SOCK removed\n\
                     more output";
        let result = strip_buildkite_noise(input);
        assert!(result.contains("test output"));
        assert!(!result.contains("{}#"));
        assert!(!result.contains("Agent pid"));
        assert!(!result.contains("# SSH_"));
    }

    #[test]
    fn test_strip_noise_datadog_ec2_lines() {
        let input = "test output\n\
                     Get EC2 instance region...\n\
                     EC2 region: us-west-2\n\
                     Get Datadog API key from AWS secret\n\
                     Sending Docker storage usage logs to DataDog...";
        let result = strip_buildkite_noise(input);
        assert!(result.contains("test output"));
        assert!(!result.contains("EC2 region:"));
        assert!(!result.contains("Get EC2 instance"));
        assert!(!result.contains("Get Datadog API key"));
        assert!(!result.contains("Sending Docker storage usage"));
    }

    #[test]
    fn test_strip_noise_warn_obsolete_version() {
        let input = "test output\n\
                     WARN[0000] /some/path/docker-compose.yml: `version` is obsolete\n\
                     more output";
        let result = strip_buildkite_noise(input);
        assert!(result.contains("test output"));
        assert!(!result.contains("WARN[0000]"));
        assert!(!result.contains("obsolete"));
    }

    #[test]
    fn test_strip_noise_user_command_error() {
        let input = "test output\n\
                     user command error: The plugin docker-compose command hook exited with status 2";
        let result = strip_buildkite_noise(input);
        assert!(result.contains("test output"));
        assert!(!result.contains("user command error:"));
    }

    #[test]
    fn test_is_test_output_indented_line() {
        // Indented lines (starting with spaces) should be recognized as test output
        let input = "real output\n\
                     ^^^ +++\n\
                       File \"test.py\", line 10";
        let result = strip_buildkite_noise(input);
        assert!(result.contains("File \"test.py\""));
    }

    #[test]
    fn test_is_test_output_assert_keyword() {
        let input = "real output\n\
                     ^^^ +++\n\
                     assert x == 1";
        let result = strip_buildkite_noise(input);
        assert!(result.contains("assert x == 1"));
    }

    #[test]
    fn test_strip_noise_kube_cleanup() {
        let input = "test output\n\
                     /var/lib/buildkite-agent/.kube folder not detected. cleanup skipped.";
        let result = strip_buildkite_noise(input);
        assert!(result.contains("test output"));
        assert!(!result.contains(".kube folder"));
    }

    #[test]
    fn test_normalize_for_grouping() {
        let log1 = "FAILED shard 1/20 in 59.72s\ncanal-coverage-1.xml\n--group 1";
        let log2 = "FAILED shard 19/20 in 56.74s\ncanal-coverage-19.xml\n--group 19";
        assert_eq!(normalize_for_grouping(log1), normalize_for_grouping(log2));
    }

    #[test]
    fn test_normalize_preserves_error_content() {
        let log = "ModuleNotFoundError: No module named 'scrapfly'";
        assert_eq!(normalize_for_grouping(log), log);
    }
}
