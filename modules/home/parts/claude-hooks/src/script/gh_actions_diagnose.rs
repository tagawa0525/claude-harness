//! `gh-actions-diagnose.sh [PR の URL | PR 番号 | ブランチ名]`: CI の最新の結果を調べ、失敗の原因を分類する。省けば今の
//! ブランチ。PR なら head のブランチと head の SHA を解く。PR の URL なら、gh を URL のリポジトリに向け、NEXT の gh の
//! 手順に -R を付ける。PR 番号、ブランチ名、省いたときは cwd のリポジトリで読む。
//!
//! 見るもの: GitHub Actions の run（失敗ならジョブ、ステップ、ログから原因を分類する）と、head の SHA の check-run のうち
//! GitHub Actions でない App（外部の CI）と commit status（gh run では見えず、gh run rerun でも再実行できない）。
//!
//! 出力: `BRANCH:`、`PR:`、`RUNS:`（直近 5 件）、`RUN:`、`STATUS:`、`ATTEMPT:`、`FAILED_JOB:`、`FAILED_STEP:`、
//! `--- errors ---`（失敗したステップのログのエラーの行）、`EXTERNAL_CHECKS:`、`EXTERNAL_CHECK:`、`CAUSE:`、`NEXT:`。
//! CAUSE は NONE、IN_PROGRESS、PERMISSION、TRANSIENT_API、COPILOT_INTERNAL、EXTERNAL、CODE（意味は sh の説明）。
//!
//! 再実行の提案は attempt 1 のときだけ。既に再実行した（attempt 2 以上）同じ失敗は、一時の障害でない見込みが高く、
//! 何度再実行しても直らないので、報告に切り替える。PERMISSION は HttpError を含むことがあるので TRANSIENT_API より先に
//! 判定する

use super::pr_url;
use regex::Regex;
use serde_json::Value;
use std::process::Command;
use std::sync::LazyLock;

static ERROR_LINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"##\[error\]|error(\[|:)|Error:|FAILED|panicked").unwrap());
static PERMISSION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)not accessible by (integration|personal access token)|refusing to allow a GitHub App|permission denied|permission_denied|insufficient permission|does not have (the )?permission|HTTP 403|403 Forbidden|status code 403").unwrap()
});
static TRANSIENT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"HTTP 50[0-9]|Unexpected end of JSON input|RequestError|HttpError").unwrap()
});
static COPILOT_INTERNAL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"Download ccrcli|Download autofind cli").unwrap());

/// gh を起動する。PR の URL を渡されたら、そのリポジトリに向ける
struct Gh {
    repo: Option<String>,
}

impl Gh {
    fn command(&self, args: &[&str]) -> Command {
        let gh = crate::staged_checks::find_on_path("gh").unwrap_or_else(|| "gh".into());
        let mut cmd = Command::new(gh);
        cmd.args(args);
        if let Some(repo) = &self.repo {
            cmd.env("GH_REPO", repo).env("GH_HOST", "github.com");
        }
        cmd
    }

    /// 標準出力（失敗すれば gh の終了コード。標準エラーは引き継ぐ）
    fn output(&self, args: &[&str]) -> Result<String, i32> {
        let out = self
            .command(args)
            .stderr(std::process::Stdio::inherit())
            .output()
            .map_err(|e| {
                eprintln!("ERROR: gh を起動できません: {e}");
                1
            })?;
        if !out.status.success() {
            return Err(out.status.code().unwrap_or(1));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// 標準出力（失敗すれば None。標準エラーは捨てる）
    fn quiet(&self, args: &[&str]) -> Option<String> {
        let out = self
            .command(args)
            .stderr(std::process::Stdio::null())
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    }
}

/// gh の出力の JSON。読めなければ理由を出して 1（空として扱うと、壊れた応答を健全な CI と言う）
fn json(what: &str, out: &str) -> Result<Value, i32> {
    serde_json::from_str(out).map_err(|e| {
        eprintln!(
            "ERROR: {what} の出力が JSON でない（{e}）: {}",
            out.trim_end()
        );
        1
    })
}

fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

pub fn main(args: &[String]) -> i32 {
    match diagnose(args.first().map(String::as_str).unwrap_or("")) {
        Ok(()) => 0,
        Err(code) => code,
    }
}

fn diagnose(target: &str) -> Result<(), i32> {
    let mut gh = Gh { repo: None };
    let mut target = target.to_string();
    // NEXT に書く gh の手順を、診断したのと同じリポジトリに向ける
    let mut repo_flag = String::new();
    // URL らしい引数は、解けなければブランチ名とみなさずに止める（cwd のリポジトリで空の結果を返さない）
    if target.contains("://") || target.contains("github.com/") {
        let pr = pr_url::parse_allowing_fragment(&target).map_err(|why| {
            eprintln!("{why}");
            1
        })?;
        repo_flag = format!(" -R {}", pr.repo);
        target = pr.number.clone();
        gh.repo = Some(pr.repo);
    }
    let mut pr_number = String::new();
    let mut head_sha = String::new();
    let branch = if target.is_empty() {
        let b = crate::git::current_branch(std::path::Path::new("."));
        if b.is_empty() {
            eprintln!(
                "ERROR: ブランチを特定できません。PR の URL、PR 番号、ブランチ名のどれかを指定してください"
            );
            return Err(1);
        }
        b
    } else if target.bytes().all(|b| b.is_ascii_digit()) {
        pr_number = target.clone();
        let meta = json(
            "gh pr view",
            &gh.output(&["pr", "view", &target, "--json", "headRefName,headRefOid"])?,
        )?;
        head_sha = meta["headRefOid"].as_str().unwrap_or("").to_string();
        text(&meta["headRefName"])
    } else {
        target.clone()
    };
    outln!("BRANCH: {branch}");
    if !pr_number.is_empty() {
        outln!("PR: {pr_number}");
    }

    // --- 1. GitHub Actions ---
    let mut cause = "NONE";
    let mut next = String::new();
    let runs = json(
        "gh run list",
        &gh.output(&[
            "run",
            "list",
            "--branch",
            &branch,
            "--limit",
            "5",
            "--json",
            "databaseId,name,status,conclusion,createdAt,url",
        ])?,
    )?;
    let Value::Array(runs) = runs else {
        eprintln!("ERROR: gh run list の出力が配列でない: {runs}");
        return Err(1);
    };
    if runs.is_empty() {
        outln!("RUNS: 0");
    } else {
        outln!("RUNS:");
        for r in &runs {
            outln!(
                "- {} {} {}/{} {} {}",
                text(&r["databaseId"]),
                text(&r["name"]),
                text(&r["status"]),
                text(&r["conclusion"]),
                text(&r["createdAt"]),
                text(&r["url"])
            );
        }
        // gh run list は新しい順
        let latest = &runs[0];
        let id = text(&latest["databaseId"]);
        let status = text(&latest["status"]);
        let conclusion = text(&latest["conclusion"]);
        outln!("RUN: {id}");
        outln!("STATUS: {status}/{conclusion}");
        if status != "completed" {
            cause = "IN_PROGRESS";
            next = "実行中です。完了を待ってから再確認してください".to_string();
        } else if !matches!(conclusion.as_str(), "success" | "skipped" | "neutral") {
            let view = json(
                "gh run view",
                &gh.output(&["run", "view", &id, "--json", "jobs,attempt"])?,
            )?;
            let attempt = view["attempt"].as_u64().unwrap_or(1);
            outln!("ATTEMPT: {attempt}");
            for job in view["jobs"].as_array().into_iter().flatten() {
                if job["conclusion"] == "failure" {
                    outln!("FAILED_JOB: {}", text(&job["name"]));
                    for step in job["steps"].as_array().into_iter().flatten() {
                        if step["conclusion"] == "failure" {
                            outln!("FAILED_STEP: {}", text(&step["name"]));
                        }
                    }
                }
            }
            // 失敗したステップのログだけを取る（ログの取得の失敗は空として扱い、分類は CODE に落ちる）
            let log = gh
                .command(&["run", "view", &id, "--log-failed"])
                .output()
                .map(|o| {
                    let mut s = String::from_utf8_lossy(&o.stdout).into_owned();
                    s.push_str(&String::from_utf8_lossy(&o.stderr));
                    s
                })
                .unwrap_or_default();
            outln!("--- errors ---");
            for line in log.lines().filter(|l| ERROR_LINE.is_match(l)).take(40) {
                outln!("{line}");
            }
            // 再実行を提案してよいのは初回の失敗だけ
            let rerun_or_report = |what: &str| {
                if attempt >= 2 {
                    format!(
                        "{what}ですが、既に {attempt} 回試行して失敗しています。これ以上再実行せず、ログ（gh run view {id}{repo_flag} --log-failed）を添えてユーザーに報告し判断を仰いでください"
                    )
                } else {
                    format!("{what}です。1 回だけ再実行してください: gh run rerun {id}{repo_flag}")
                }
            };
            if PERMISSION.is_match(&log) {
                cause = "PERMISSION";
                next = "権限不足です。ワークフローの permissions: に必要な権限を最小限で宣言してください（例: pull-requests: write）。リポジトリ設定の default_workflow_permissions を write に緩めて回避しないこと。GitHub App（Copilot 等）側の権限不足なら App のインストール設定を確認してください".to_string();
            } else if TRANSIENT.is_match(&log) {
                cause = "TRANSIENT_API";
                next = rerun_or_report("GitHub API 側の一時障害");
            } else if COPILOT_INTERNAL.is_match(&log) {
                cause = "COPILOT_INTERNAL";
                next = rerun_or_report("Copilot 側の一時的な問題");
            } else {
                cause = "CODE";
                next = format!(
                    "エラー行を読んで原因を修正してください（全文: gh run view {id}{repo_flag} --log-failed）"
                );
            }
        }
    }

    // --- 2. 外部の CI（GitHub Actions でない App の check-run と commit status） ---
    if head_sha.is_empty() {
        head_sha = gh
            .quiet(&[
                "api",
                &format!("repos/{{owner}}/{{repo}}/git/ref/heads/{branch}"),
                "--jq",
                ".object.sha",
            ])
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
    }
    let mut external: Option<Vec<Value>> = None;
    if !head_sha.is_empty() {
        // gh の成否で先に分ける（gh が失敗したのを「外部の CI なし」と取り違えない）
        let checks = gh
            .quiet(&[
                "api",
                "--paginate",
                &format!("repos/{{owner}}/{{repo}}/commits/{head_sha}/check-runs?per_page=100"),
                "--jq",
                r#".check_runs[] | select(.app.slug != "github-actions")
            | {name, status, conclusion, app: .app.slug, url: .details_url}"#,
            ])
            .and_then(|s| {
                serde_json::Deserializer::from_str(&s)
                    .into_iter::<Value>()
                    .collect::<Result<Vec<_>, _>>()
                    .ok()
            });
        let statuses = gh
            .quiet(&[
                "api",
                &format!("repos/{{owner}}/{{repo}}/commits/{head_sha}/status"),
                "--jq",
                r#"[.statuses[] | {name: .context,
                            status: (if .state == "pending" then "in_progress" else "completed" end),
                            conclusion: .state, app: "commit-status", url: .target_url}]"#,
            ])
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .and_then(|v| v.as_array().cloned());
        if let (Some(mut c), Some(s)) = (checks, statuses) {
            c.extend(s);
            external = Some(c);
        }
    }
    let ok = |c: &Value| {
        matches!(
            c["conclusion"].as_str(),
            Some("success" | "neutral" | "skipped")
        )
    };
    let (mut ext_failed, mut ext_pending) = (Vec::new(), 0);
    match &external {
        Some(ext) => {
            outln!("EXTERNAL_CHECKS: {}", ext.len());
            for c in ext {
                let url = if c["url"].is_null() {
                    "-".to_string()
                } else {
                    text(&c["url"])
                };
                outln!(
                    "EXTERNAL_CHECK: {} {}/{} {} {url}",
                    text(&c["name"]),
                    text(&c["status"]),
                    text(&c["conclusion"]),
                    text(&c["app"])
                );
                if c["status"] != "completed" {
                    ext_pending += 1;
                } else if !ok(c) {
                    ext_failed.push(format!("{} ({}): {url}", text(&c["name"]), text(&c["app"])));
                }
            }
        }
        None => {
            outln!("EXTERNAL_CHECKS: unavailable");
            outln!(
                "NOTE: head SHA の check-run を取得できません（ブランチが未 push か、API エラー）"
            );
        }
    }

    // --- 3. 総合の判定。GitHub Actions の側に原因があればそれを先にし、外部の CI は情報として添える ---
    if cause != "NONE" {
        outln!("CAUSE: {cause}");
        outln!("NEXT: {next}");
    } else if !ext_failed.is_empty() {
        outln!("CAUSE: EXTERNAL");
        outln!(
            "NEXT: 外部 CI の失敗です（GitHub Actions の run ではないため、ここからは再実行できません）。ログを details_url で確認し、一時的な失敗なら外部 CI 側で再実行してください: {}",
            ext_failed.join(", ")
        );
    } else if ext_pending > 0 {
        outln!("CAUSE: IN_PROGRESS");
        outln!("NEXT: 外部 CI が実行中です。完了を待ってから再確認してください");
    } else {
        outln!("CAUSE: NONE");
        if runs.is_empty() {
            outln!("NOTE: ワークフローが未設定か、このブランチではまだ実行されていません");
        }
    }
    Ok(())
}
