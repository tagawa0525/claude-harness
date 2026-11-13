//! `request-rereview.sh <PR の URL>`: Copilot にレビューを要求し、その応答を待つ（/gh-pr-review の Step 6。任意で、head
//! のローカルレビューの記録があればマージできる）。
//!
//! 1. head にローカルレビューの記録（最後の記録から rebase だけした head を含む。判定は pre-merge-check と同じ関数）が
//!    あるかを確かめる。無ければ要求しない（Copilot に出す前に、直した差分をローカルの /code-review で確かめる）
//! 2. requested_reviewers の API で copilot-pull-request-reviewer にレビューを要求する
//! 3. 要求が review_requested のイベントとして登録されたことを確かめ、その created_at（GitHub の側の時刻）を基準に
//!    gh-wait-review で待つ
//!
//! @copilot のメンションでは再レビューは走らない（応答する copilot-swe-agent はコメントを返すだけ）。要求に失敗しても
//! メンションなどに逃げない。逃げると、レビューが来ていないのに依頼できたように見えるので、止めてユーザーに判断させる。
//! 終了コードは待ちに従う（0 = レビューが到着、1 = 時間切れなど。gh-wait-review を参照）。要求そのものに失敗したら
//! 1 で、待ちに入らない

use super::{gh_review_requests, gh_wait_review, pr_url};

const USAGE: &str = "Usage: request-rereview.sh <PR の URL>";
const REVIEWER: &str = "copilot-pull-request-reviewer[bot]";

pub fn main(args: &[String]) -> i32 {
    let Some(pr) = pr_url::or_usage(
        pr_url::parse(args.first().map(String::as_str).unwrap_or("")),
        USAGE,
    ) else {
        return 1;
    };
    if args.len() != 1 {
        eprintln!("{USAGE}");
        return 1;
    }
    let canonical = format!("https://github.com/{}/pull/{}", pr.repo, pr.number);
    let scripts = super::claude_root()
        .join("skills/gh-pr-review/scripts")
        .display()
        .to_string()
        .replace('\\', "/");

    let Some(before) = latest_request(&pr) else {
        return 1;
    };
    let head = crate::gh::output_for_repo(
        &pr.repo,
        &[
            "pr",
            "view",
            &pr.number,
            "--json",
            "headRefOid",
            "-q",
            ".headRefOid",
        ],
    )
    .map(|out| String::from_utf8_lossy(&out).trim().to_string())
    .unwrap_or_default();
    if head.is_empty() {
        eprintln!(
            "ERROR: PR #{} の head を取得できませんでした（ローカルレビューの記録を確かめられません）",
            pr.number
        );
        return 1;
    }
    // 記録の後に rebase だけしたかは、pre-merge-check と同じく cwd の clone のコミットで比べる
    let covered = std::env::current_dir().map_err(|_| ()).and_then(|dir| {
        crate::rules::local_review_covers_for_pr(&dir, &pr.repo, &pr.number, &head)
    });
    for notice in crate::output::take_notices() {
        eprintln!("{notice}");
    }
    match covered {
        Err(()) => {
            eprintln!(
                "ERROR: PR #{} のローカルレビューの記録を確かめられませんでした",
                pr.number
            );
            return 1;
        }
        Ok(false) => {
            eprintln!(
                "ERROR: head（{}）に High の残らないローカルレビューの記録がありません。Copilot に出す前に、直した差分をローカルの /code-review で確かめてください",
                head.get(..7).unwrap_or(&head)
            );
            eprintln!(
                "次の一手: /gh-pr-review の「ローカルレビュー」の手順で /code-review を回したら {scripts}/record-local-review.sh {canonical} <SHA> <high> <medium> <low> で記録し（High が残れば直して回し直す）、もう一度 {scripts}/request-rereview.sh {canonical} で要求してください"
            );
            return 1;
        }
        Ok(true) => {}
    }

    let reviewer = format!("reviewers[]={REVIEWER}");
    let post = crate::gh::quiet_stdout_output_for_repo(
        &pr.repo,
        &[
            "api",
            "-X",
            "POST",
            &format!(
                "repos/{{owner}}/{{repo}}/pulls/{}/requested_reviewers",
                pr.number
            ),
            "-f",
            &reviewer,
        ],
    );
    if let Err(err) = post {
        eprint!("{err}");
        eprintln!(
            "ERROR: PR #{} に {REVIEWER} のレビューを要求できませんでした",
            pr.number
        );
        eprintln!(
            "HINT: fork からの upstream PR（push 権限なし）では 404 になります。その場合は再レビューを依頼できないので、対応内容を PR コメントで伝えてレビュアーの判断を待ってください"
        );
        return 1;
    }

    let Some(after) = latest_request(&pr) else {
        return 1;
    };
    if after.is_empty() || after == before {
        eprintln!(
            "ERROR: レビュー要求が登録されませんでした（review_requested イベントが増えていません）"
        );
        eprintln!(
            "HINT: 前回の要求が保留のまま（レビューがまだ来ていない）か、リポジトリで Copilot code review が無効です。gh pr view {canonical} --json reviewRequests と /gh-actions-check {canonical} で確認してください"
        );
        return 1;
    }
    println!("REQUESTED: {REVIEWER}");
    println!("SINCE: {after}");
    gh_wait_review::main(&[canonical, "--since".to_string(), after])
}

/// 最新の Copilot 宛てのレビューの要求の時刻（無ければ空）。取れなければ理由を出して None（空として続けると、要求が
/// 登録されているのに「登録されませんでした」と誤る）
fn latest_request(pr: &pr_url::Pr) -> Option<String> {
    match gh_review_requests::request_times(pr) {
        Ok(times) => Some(times.last().cloned().unwrap_or_default()),
        Err(_) => {
            eprintln!(
                "ERROR: PR #{} のレビュー要求を取得できませんでした（登録を確認できません）",
                pr.number
            );
            None
        }
    }
}
