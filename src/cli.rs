use clap::Parser;

#[derive(Parser, Debug)]
#[command(
    name = "bk-check",
    about = "Check Buildkite CI status for a GitHub PR",
    version
)]
pub struct Args {
    /// GitHub PR URL (e.g., https://github.com/ROKT/canal/pull/14908)
    pub pr_url: String,

    /// Maximum number of log lines to include per failed job
    #[arg(long, default_value_t = 100)]
    pub max_log_lines: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn test_default_args() {
        let args = Args::parse_from(["bk-check", "https://github.com/ROKT/canal/pull/14908"]);
        assert_eq!(args.pr_url, "https://github.com/ROKT/canal/pull/14908");
        assert_eq!(args.max_log_lines, 100);
    }

    #[test]
    fn test_custom_max_log_lines() {
        let args = Args::parse_from([
            "bk-check",
            "https://github.com/ROKT/canal/pull/14908",
            "--max-log-lines",
            "200",
        ]);
        assert_eq!(args.max_log_lines, 200);
    }

    #[test]
    fn test_missing_pr_url() {
        let result = Args::try_parse_from(["bk-check"]);
        assert!(result.is_err());
    }
}
