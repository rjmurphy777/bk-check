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

pub fn clean_log(raw: &str, max_lines: usize) -> String {
    let stripped = strip_ansi(raw);
    let stripped = strip_timestamps(&stripped);
    let lines: Vec<&str> = stripped.lines().collect();
    let extracted = extract_failure_section(&lines, max_lines);
    extracted
}

fn strip_ansi(input: &str) -> String {
    let re = Regex::new(r"\x1b\[[0-9;]*[a-zA-Z]").unwrap();
    re.replace_all(input, "").to_string()
}

fn strip_timestamps(input: &str) -> String {
    let re = Regex::new(r"(?m)^\[\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z\]\s?").unwrap();
    re.replace_all(input, "").to_string()
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
        let mut lines: Vec<String> = (0..50).map(|i| format!("normal line {}", i)).collect();
        lines.push("Traceback (most recent call last):".to_string());
        lines.push("  File \"test.py\", line 10".to_string());
        lines.push("ModuleNotFoundError: No module named 'foo'".to_string());
        for i in 0..10 {
            lines.push(format!("after error {}", i));
        }

        let line_refs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
        let result = extract_failure_section(&line_refs, 20);

        assert!(result.contains("Traceback (most recent call last)"));
        assert!(result.contains("ModuleNotFoundError"));
    }

    #[test]
    fn test_extract_no_markers_takes_tail() {
        let lines: Vec<String> = (0..200).map(|i| format!("line {}", i)).collect();
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
            lines.push(format!("line {}", i));
        }
        lines.push("FAILED test case".to_string());
        for i in 0..500 {
            lines.push(format!("after {}", i));
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
}
