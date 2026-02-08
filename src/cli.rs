use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "bk-check",
    about = "Check Buildkite CI status for a GitHub PR",
    version
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// GitHub PR URL (e.g., https://github.com/ROKT/canal/pull/14908)
    pub pr_url: Option<String>,

    /// Maximum number of log lines to include per failed job
    #[arg(long, default_value_t = 100)]
    pub max_log_lines: usize,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Update bk-check to the latest version from GitHub
    Update,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn test_pr_url_directly() {
        let cli = Cli::parse_from(["bk-check", "https://github.com/ROKT/canal/pull/14908"]);
        assert_eq!(
            cli.pr_url.as_deref(),
            Some("https://github.com/ROKT/canal/pull/14908")
        );
        assert!(cli.command.is_none());
        assert_eq!(cli.max_log_lines, 100);
    }

    #[test]
    fn test_pr_url_with_max_log_lines() {
        let cli = Cli::parse_from([
            "bk-check",
            "https://github.com/ROKT/canal/pull/14908",
            "--max-log-lines",
            "200",
        ]);
        assert_eq!(
            cli.pr_url.as_deref(),
            Some("https://github.com/ROKT/canal/pull/14908")
        );
        assert_eq!(cli.max_log_lines, 200);
    }

    #[test]
    fn test_update_command() {
        let cli = Cli::parse_from(["bk-check", "update"]);
        assert!(matches!(cli.command, Some(Command::Update)));
    }

    #[test]
    fn test_no_args() {
        let cli = Cli::parse_from(["bk-check"]);
        assert!(cli.command.is_none());
        assert!(cli.pr_url.is_none());
    }
}
