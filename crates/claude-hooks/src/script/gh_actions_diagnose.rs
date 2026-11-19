//! `gh-actions-diagnose.sh [PR の URL | PR 番号 | ブランチ名]`: CI の最新の結果を調べ、失敗の原因を分類する。省けば今の
//! ブランチ。PR なら head のブランチと head の SHA を解く。PR の URL なら、gh を URL のリポジトリに向け、NEXT の gh の
//! 手順に -R を付ける。PR 番号、ブランチ名、省いたときは cwd のリポジトリで読む。
//!
//! 見るもの: head の SHA の GitHub Actions の run のうち、ワークフローと event ごとの最新（どれかが失敗なら、その 1 つの
//! ジョブ、ステップ、ログから原因を分類する。失敗が無く実行中があれば IN_PROGRESS）と、head の SHA の check-run のうち
//! GitHub Actions でない App（外部の CI）と commit status（gh run では見えず、gh run rerun でも再実行できない）。
//! head の SHA を読めなければ（未 push か API の失敗）、ブランチの最新の run の SHA で判定する。
//!
//! 出力: `BRANCH:`、`PR:`、`RUNS:`（直近 5 件）、`RUN:`、`STATUS:`、`ATTEMPT:`、`FAILED_JOB:`、`FAILED_STEP:`、
//! `--- errors ---`（失敗したステップのログのエラーの行）、`EXTERNAL_CHECKS:`、`EXTERNAL_CHECK:`、`CAUSE:`、`NEXT:`、
//! `--- report ---`（/gh-actions-check の報告の形にまとめた行）。
//! CAUSE は NONE、IN_PROGRESS、PERMISSION、TRANSIENT_API、COPILOT_INTERNAL、EXTERNAL、CODE（意味は sh の説明）。NEXT は
//! どの CAUSE でも出す。
//!
//! 再実行の提案は attempt 1 のときだけ。既に再実行した（attempt 2 以上）同じ失敗は、一時の障害でない見込みが高く、
//! 何度再実行しても直らないので、報告に切り替える。PERMISSION は HttpError を含むことがあるので TRANSIENT_API より先に
//! 判定する

use super::pr_url;
use regex::Regex;
use serde_json::Value;
use std::collections::HashSet;
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
            return Err(crate::child::exit_code(out.status));
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
    println!("BRANCH: {branch}");
    if !pr_number.is_empty() {
        println!("PR: {pr_number}");
    }

    // --- 1. GitHub Actions ---
    let mut cause = "NONE";
    let mut next = String::new();
    let mut report = Report::default();
    let runs = json(
        "gh run list",
        &gh.output(&[
            "run",
            "list",
            "--branch",
            &branch,
            // RUNS に出すのは 5 件だが、head の SHA の run をワークフローごとに拾うために多めに読む（新しい順なので、
            // head の run は先頭に並ぶ）
            "--limit",
            "100",
            "--json",
            "databaseId,name,workflowDatabaseId,event,headSha,status,conclusion,createdAt,url",
        ])?,
    )?;
    let Value::Array(runs) = runs else {
        eprintln!("ERROR: gh run list の出力が配列でない: {runs}");
        return Err(1);
    };
    if runs.is_empty() {
        println!("RUNS: 0");
    } else {
        println!("RUNS:");
        for r in runs.iter().take(5) {
            println!(
                "- {} {} {}/{} {} {}",
                text(&r["databaseId"]),
                text(&r["name"]),
                text(&r["status"]),
                text(&r["conclusion"]),
                text(&r["createdAt"]),
                text(&r["url"])
            );
        }
    }
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
    let sha_of = |r: &Value| r["headSha"].as_str().unwrap_or("").to_string();
    // head を読めなければ（未 push か API の失敗）最新の run の commit で判定する。外部の CI は head_sha が空なら読まない
    let actions_sha = if head_sha.is_empty() {
        runs.first().map(sha_of).unwrap_or_default()
    } else {
        head_sha.clone()
    };
    // ワークフローと event ごとの最新の run（gh run list は新しい順）。同じワークフローでも push と pull_request の run は
    // 別々に走り、片方の成功で他方の失敗を隠さないように event でも分ける
    let mut seen = HashSet::new();
    let head_runs: Vec<&Value> = runs
        .iter()
        .filter(|r| sha_of(r) == actions_sha)
        .filter(|r| seen.insert((text(&r["workflowDatabaseId"]), text(&r["event"]))))
        .collect();
    let finished_ok = |r: &Value| {
        r["status"] == "completed" && not_failed(r["conclusion"].as_str().unwrap_or(""))
    };
    let unfinished_or_failed: Vec<&Value> = head_runs
        .iter()
        .copied()
        .filter(|r| !finished_ok(r))
        .collect();
    // 失敗を先に診断する（実行中の run を待っても、失敗した run の結果は変わらない）
    let target = unfinished_or_failed
        .iter()
        .find(|r| r["status"] == "completed")
        .or(unfinished_or_failed.first())
        .or(head_runs.first())
        .copied();
    if let Some(run) = target {
        let id = text(&run["databaseId"]);
        let status = text(&run["status"]);
        let conclusion = text(&run["conclusion"]);
        println!("RUN: {id}");
        println!("STATUS: {status}/{conclusion}");
        report.run = id.clone();
        report.status = format!("{status}/{conclusion}");
        if status != "completed" {
            cause = "IN_PROGRESS";
            next = "実行中です。完了を待ってから再確認してください".to_string();
        } else if !not_failed(&conclusion) {
            let view = json(
                "gh run view",
                &gh.output(&["run", "view", &id, "--json", "jobs,attempt"])?,
            )?;
            let attempt = view["attempt"].as_u64().unwrap_or(1);
            println!("ATTEMPT: {attempt}");
            report.attempt = Some(attempt);
            for job in view["jobs"].as_array().into_iter().flatten() {
                if job["conclusion"] == "failure" {
                    println!("FAILED_JOB: {}", text(&job["name"]));
                    report.failed_jobs.push(text(&job["name"]));
                    for step in job["steps"].as_array().into_iter().flatten() {
                        if step["conclusion"] == "failure" {
                            println!("FAILED_STEP: {}", text(&step["name"]));
                            report.failed_steps.push(text(&step["name"]));
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
            println!("--- errors ---");
            for line in log.lines().filter(|l| ERROR_LINE.is_match(l)).take(40) {
                println!("{line}");
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
            let others: Vec<String> = unfinished_or_failed
                .iter()
                .filter(|r| !std::ptr::eq(**r, run))
                .map(|r| {
                    format!(
                        "{} {} {}/{}",
                        text(&r["name"]),
                        text(&r["databaseId"]),
                        text(&r["status"]),
                        text(&r["conclusion"])
                    )
                })
                .collect();
            if !others.is_empty() {
                next.push_str(&format!(
                    "。ほかのワークフローにも失敗か実行中の run があります: {}",
                    others.join(", ")
                ));
            }
        }
    }

    // --- 2. 外部の CI（GitHub Actions でない App の check-run と commit status） ---
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
            println!("EXTERNAL_CHECKS: {}", ext.len());
            for c in ext {
                let url = if c["url"].is_null() {
                    "-".to_string()
                } else {
                    text(&c["url"])
                };
                let check = format!(
                    "{} {}/{} {} {url}",
                    text(&c["name"]),
                    text(&c["status"]),
                    text(&c["conclusion"]),
                    text(&c["app"])
                );
                println!("EXTERNAL_CHECK: {check}");
                report.external.push(check);
                if c["status"] != "completed" {
                    ext_pending += 1;
                } else if !ok(c) {
                    ext_failed.push(format!("{} ({}): {url}", text(&c["name"]), text(&c["app"])));
                }
            }
        }
        None => {
            report.external_unavailable = true;
            println!("EXTERNAL_CHECKS: unavailable");
            println!(
                "NOTE: head SHA の check-run を取得できません（ブランチが未 push か、API エラー）"
            );
        }
    }

    // --- 3. 総合の判定。GitHub Actions の側に原因があればそれを先にし、外部の CI は情報として添える ---
    if cause == "NONE" {
        if !ext_failed.is_empty() {
            cause = "EXTERNAL";
            next = format!(
                "外部 CI の失敗です（GitHub Actions の run ではないため、ここからは再実行できません）。ログを details_url で確認し、一時的な失敗なら外部 CI 側で再実行してください: {}",
                ext_failed.join(", ")
            );
        } else if ext_pending > 0 {
            cause = "IN_PROGRESS";
            next = "外部 CI が実行中です。完了を待ってから再確認してください".to_string();
        } else if runs.is_empty() && external.is_none() {
            next = "ワークフローが未設定か未実行で、外部 CI は確認できていないと報告してください"
                .to_string();
        } else if runs.is_empty() && external.as_ref().is_some_and(|e| !e.is_empty()) {
            next = "GitHub Actions の run は無く、外部 CI に失敗はありません。その 2 点を報告してください"
                .to_string();
        } else if runs.is_empty() {
            next = "ワークフローが未設定か未実行であると報告してください".to_string();
        } else {
            // skipped と neutral は成功と書かせない
            let not_success: Vec<String> = head_runs
                .iter()
                .filter(|r| r["conclusion"] != "success")
                .map(|r| format!("{}: {}", text(&r["name"]), text(&r["conclusion"])))
                .collect();
            let head = if head_runs.is_empty() {
                format!(
                    "head の SHA（{actions_sha}）の GitHub Actions の run はまだありません（RUNS はそれより前の commit の run）。起動を待って再確認し、それでも無ければ、この commit ではワークフローが走っていないと報告してください"
                )
            } else if not_success.is_empty() {
                format!(
                    "head の SHA（{actions_sha}）の GitHub Actions の run は、ワークフローごとの最新がすべて success です。そう報告してください"
                )
            } else {
                format!(
                    "head の SHA（{actions_sha}）の GitHub Actions の run に失敗と実行中はありません。success でない run（{}）を添え、成功とは書かずに報告してください",
                    not_success.join(", ")
                )
            };
            let unchecked = if external.is_none() {
                "。外部 CI は確認できていないと添えてください"
            } else {
                ""
            };
            next = format!("{head}{unchecked}");
        }
    }
    println!("CAUSE: {cause}");
    if cause == "NONE" && runs.is_empty() {
        println!("NOTE: ワークフローが未設定か、このブランチではまだ実行されていません");
    }
    println!("NEXT: {next}");
    report.print(cause, &next);
    Ok(())
}

/// 終わった GitHub Actions の run の結論が、診断する失敗でないか。cancelled は取り消しでログに失敗が無く、コードの誤りと
/// 診断しない（成功とも書かせず、NONE の NEXT に success でない run として並べる）
fn not_failed(conclusion: &str) -> bool {
    matches!(conclusion, "success" | "skipped" | "neutral" | "cancelled")
}

/// 報告の形（/gh-actions-check の報告）にまとめる値。読めなかったものは空のまま「なし」と出す
#[derive(Default)]
struct Report {
    run: String,
    status: String,
    attempt: Option<u64>,
    failed_jobs: Vec<String>,
    failed_steps: Vec<String>,
    /// EXTERNAL_CHECK の行と同じ形（URL まで）
    external: Vec<String>,
    external_unavailable: bool,
}

impl Report {
    fn print(&self, cause: &str, next: &str) {
        let or_none = |v: &[String]| {
            if v.is_empty() {
                "なし".to_string()
            } else {
                v.join(", ")
            }
        };
        let run = match (self.run.is_empty(), self.attempt) {
            (true, _) => "なし".to_string(),
            (false, Some(a)) => format!("{}（attempt {a}）", self.run),
            (false, None) => self.run.clone(),
        };
        let status = if self.status.is_empty() {
            "なし"
        } else {
            &self.status
        };
        let external = if self.external_unavailable {
            "確認できず".to_string()
        } else {
            or_none(&self.external)
        };
        println!("--- report ---");
        println!("GitHub Actions 状況:");
        println!("- Run: {run}");
        println!("- Status: {status}");
        println!("- 失敗ジョブ: {}", or_none(&self.failed_jobs));
        println!("- 失敗ステップ: {}", or_none(&self.failed_steps));
        println!("- 外部 CI: {external}");
        println!("- 原因: {cause}");
        println!("- 推奨対応: {next}");
    }
}
