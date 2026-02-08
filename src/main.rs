use anyhow::{anyhow, Context, Result};
use clap::Parser;

use bk_check::buildkite::BuildkiteClient;
use bk_check::cli::{Cli, Command};
use bk_check::github::{resolve_github_token_with, GitHubClient};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    if let Some(Command::Update) = cli.command {
        return run_update();
    }

    let pr_url = cli
        .pr_url
        .ok_or_else(|| anyhow!("Usage: bk-check <PR_URL> or bk-check update"))?;

    let gh_token = resolve_github_token_with(std::env::var("GITHUB_TOKEN").ok(), || {
        let output = std::process::Command::new("gh")
            .args(["auth", "token"])
            .output()
            .context("Failed to run 'gh auth token'. Install gh CLI or set GITHUB_TOKEN")?;

        if !output.status.success() {
            return Err(anyhow!(
                "'gh auth token' failed. Set GITHUB_TOKEN or authenticate with 'gh auth login'"
            ));
        }

        String::from_utf8(output.stdout).context("Invalid UTF-8 from gh auth token")
    })
    .context("Failed to resolve GitHub token")?;

    let bk_token = std::env::var("BUILDKITE_API_TOKEN")
        .context("BUILDKITE_API_TOKEN environment variable is required")?;

    let gh_client = GitHubClient::new(gh_token);
    let bk_client = BuildkiteClient::new(bk_token);

    let report = bk_check::run(&pr_url, cli.max_log_lines, &gh_client, &bk_client).await?;

    let json = serde_json::to_string_pretty(&report).context("Failed to serialize report")?;
    println!("{json}");

    if report.overall_status == "failure" {
        std::process::exit(1);
    }

    Ok(())
}

fn run_update() -> Result<()> {
    println!("Updating bk-check...");

    let status = std::process::Command::new("cargo")
        .args([
            "install",
            "--git",
            "https://github.com/rjmurphy777/bk-check.git",
            "--force",
        ])
        .status()
        .context("Failed to run cargo install")?;

    if !status.success() {
        return Err(anyhow!("Update failed"));
    }

    println!("bk-check updated successfully.");
    Ok(())
}
