# bk-check

[![CI](https://github.com/rjmurphy777/bk-check/actions/workflows/ci.yml/badge.svg)](https://github.com/rjmurphy777/bk-check/actions/workflows/ci.yml)
[![Tests](https://github.com/rjmurphy777/bk-check/actions/workflows/ci.yml/badge.svg?event=push&label=tests)](https://github.com/rjmurphy777/bk-check/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/rjmurphy777/bk-check/graph/badge.svg)](https://codecov.io/gh/rjmurphy777/bk-check)

CLI tool that checks Buildkite CI status for a GitHub PR and reports failures with logs.

Takes a GitHub PR URL, finds the associated Buildkite build via commit status checks, and outputs a structured JSON report with passed/failed jobs and cleaned failure logs — designed for quick triage and LLM consumption.

## Installation

### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) (1.70+)
- A [Buildkite API token](https://buildkite.com/user/api-access-tokens) with `read_builds` and `read_build_logs` permissions
- A GitHub token (via `GITHUB_TOKEN` env var or [GitHub CLI](https://cli.github.com/) authentication)

### Build from source

```bash
git clone https://github.com/rjmurphy777/bk-check.git
cd bk-check
cargo build --release
```

The binary will be at `target/release/bk-check`. You can copy it to a directory on your `PATH`:

```bash
cp target/release/bk-check /usr/local/bin/
```

Or install directly with Cargo:

```bash
cargo install --path .
```

## Setup

### 0. AppGate SDP

Make sure AppGate SDP is open and connected before running bk-check. The tool needs network access to both the GitHub API and the Buildkite API, which require AppGate SDP to be active.

### 1. Buildkite API Token (required)

Create a token at [Buildkite Personal Settings > API Access Tokens](https://buildkite.com/user/api-access-tokens).

Required scopes:
- **Read Builds** (`read_builds`)
- **Read Build Logs** (`read_build_logs`)

Set it as an environment variable:

```bash
export BUILDKITE_API_TOKEN="bkua_xxxxxxxxxxxxxxxxxxxx"
```

To persist it, add the export to your shell profile (`~/.zshrc`, `~/.bashrc`, etc.).

### 2. GitHub Token

bk-check needs a GitHub token to read PR metadata and commit statuses. It checks two sources in order:

1. **`GITHUB_TOKEN` environment variable** — if set, this is used directly
2. **GitHub CLI fallback** — if `GITHUB_TOKEN` is not set, it runs `gh auth token` to get the token from the [GitHub CLI](https://cli.github.com/)

**Option A: Use GitHub CLI (recommended)**

If you already have the `gh` CLI installed and authenticated, no extra setup is needed:

```bash
# Authenticate if you haven't already
gh auth login
```

**Option B: Set GITHUB_TOKEN manually**

Create a [Personal Access Token](https://github.com/settings/tokens) with `repo` scope (or `public_repo` for public repos only):

```bash
export GITHUB_TOKEN="ghp_xxxxxxxxxxxxxxxxxxxx"
```

## Usage

```
bk-check <PR_URL> [--max-log-lines <N>]
```

### Examples

Check a PR with failed CI:

```bash
bk-check https://github.com/ROKT/canal/pull/14908
```

Check a PR with all passing CI:

```bash
bk-check https://github.com/ROKT/canal/pull/14916
```

Include more log context for failed jobs:

```bash
bk-check https://github.com/ROKT/canal/pull/14908 --max-log-lines 200
```

### Options

| Flag | Default | Description |
|------|---------|-------------|
| `--max-log-lines` | `100` | Maximum number of log lines to include per failed job |

### Exit codes

| Code | Meaning |
|------|---------|
| `0` | All jobs passed |
| `1` | One or more jobs failed |
| `non-zero` | Error (missing token, API failure, invalid URL, etc.) |

## Output

bk-check outputs structured JSON to stdout:

```json
{
  "pr_url": "https://github.com/ROKT/canal/pull/14908",
  "branch": "feature-branch",
  "commit": "abc123def456",
  "overall_status": "failure",
  "build_url": "https://buildkite.com/rokt/catalog-ci-pipeline/builds/5939",
  "passed_jobs": [
    {
      "name": "lint",
      "state": "passed"
    }
  ],
  "failed_jobs": [
    {
      "name": ":pytest: Run pytest (shard 2/20)",
      "state": "failed",
      "exit_status": 1,
      "web_url": "https://buildkite.com/rokt/catalog-ci-pipeline/builds/5939#job-2",
      "failure_log": "Traceback (most recent call last):\n  File \"test.py\", line 10\nImportError: No module named 'catalog.service'"
    }
  ],
  "warnings": []
}
```

### Fields

| Field | Description |
|-------|-------------|
| `overall_status` | `"success"` if no failed jobs, `"failure"` otherwise |
| `passed_jobs` | Jobs that passed (including soft-failed jobs) |
| `failed_jobs` | Jobs that hard-failed, with cleaned failure logs |
| `warnings` | Soft-failed jobs, log fetch failures, etc. (omitted when empty) |

### Log cleaning

Raw Buildkite logs can be 1MB+ with ANSI escape codes and timestamps. bk-check cleans them by:

1. Stripping ANSI escape sequences
2. Removing leading timestamps (`[2026-02-07T19:39:15Z]`)
3. Searching for failure markers (`Traceback`, `FAILED`, `ImportError`, `Error:`, etc.)
4. Extracting lines around the failure marker
5. Falling back to the last N lines if no markers are found
6. Capping output at `--max-log-lines` (default 100)

## How it works

```
PR URL → GitHub API (get branch + commit SHA)
       → GitHub API (get commit status checks)
       → Extract Buildkite build URL from status
       → Buildkite API (get build details + jobs)
       → For each failed job: Buildkite API (fetch + clean log)
       → Output JSON report
```

## Development

### Run tests

```bash
cargo test
```

All API interactions are tested with [wiremock](https://crates.io/crates/wiremock) — no real API calls needed.

### Project structure

```
src/
  main.rs           Entry point: parse args, load tokens, call run(), print output
  lib.rs            run() orchestrator, module re-exports
  cli.rs            clap argument parsing
  parse.rs          URL parsing (GitHub PR URLs + Buildkite build URLs)
  github.rs         GitHub REST client + response types
  buildkite.rs      Buildkite REST client + response types
  models.rs         Output domain types (BuildReport, JobSummary, FailedJob)
  log_parser.rs     Strip ANSI codes, extract failure sections from raw logs
```
