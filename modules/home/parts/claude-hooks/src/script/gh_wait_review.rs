//! `gh-wait-review.sh <PR の URL> [--since <ISO 8601 の時刻>]`: PR の Copilot のレビューの到着を、間隔を延ばしながら
//! 待つ。--since が無ければ、最後の Copilot へのレビューの要求を基準にし、それより後に提出されたレビューを待つ。要求が
//! 1 件も無ければ、PR を作った直後の自動の要求の記録を少し待ったうえで止める（要求していないレビューは来ない）。
//! --since は request-rereview.sh が要求の created_at を渡す。
//!
//! 基準を要求に結ぶのは、起動したときの最新のレビューを基準にすると、来ないレビューを 10 分待つため。要求の後の
//! レビューが既にあればすぐ成功するので、続けて呼んでも害は無い。基準は GitHub の側の時刻で持つ（手元の時計の
//! ずれで取りこぼさない）。待つのは Copilot のレビューの提出（submittedAt）だけで、PR のコメントは数えない（@copilot
//! に応答する copilot-swe-agent はコメントを返すだけでレビューを提出しない）。
//!
//! 待つ間隔（秒）は GH_WAIT_INTERVALS、要求の記録を待つ間隔は GH_WAIT_REQUEST_INTERVALS で上書きできる（テスト用）。
//! メッセージは標準出力に出す（sh の版と同じ）。
//!
//! 終了コード: 0 = レビューが到着、1 = 時間切れ（約 10 分）、3 = PR が見つからない、4 = gh が使えない（無い、
//! 認証していない）、5 = 引数の誤り、6 = レビューの要求が無い（待っても来ない）か取れない、7 = レビューの実行が失敗
//! している（トークン切れ、内部エラーなど。待っても来ない）

use super::{gh_review_requests, pr_url};
use serde_json::Value;
use std::time::Duration;

/// 15+30+45+60+90+120+120+120 = 600 秒（約 10 分）
const INTERVALS: &str = "15 30 45 60 90 120 120 120";
/// PR を作ったときの自動の要求は、作った 1〜5 秒後に記録される（PR #239〜#241 で実測）
const REQUEST_INTERVALS: &str = "2 3 5";

pub fn main(args: &[String]) -> i32 {
    let mut url: Option<&str> = None;
    let mut since: Option<String> = None;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--since" => match rest.next().filter(|v| !v.is_empty()) {
                Some(v) => since = Some(v.clone()),
                None => {
                    println!("ERROR: --since には ISO 8601 の時刻が必要です");
                    return 5;
                }
            },
            a if a.starts_with('-') => {
                println!("ERROR: 不明なオプション: {a}");
                return 5;
            }
            a => {
                if let Some(first) = url {
                    println!("ERROR: PR の URL は 1 つだけ指定してください: '{first}' と '{a}'");
                    return 5;
                }
                url = Some(a);
            }
        }
    }
    let pr = match pr_url::parse(url.unwrap_or("")) {
        Ok(pr) => pr,
        Err(why) => {
            eprintln!("{why}");
            return 5;
        }
    };
    let canonical = format!("https://github.com/{}/pull/{}", pr.repo, pr.number);

    // PR の解決や待ちの失敗を「PR が無い」と混同しないよう、gh を使えるかを先に確かめる
    match crate::gh::quiet_status_for_repo(&pr.repo, &["auth", "status"]) {
        Err(_) => {
            println!("ERROR: gh コマンドが見つかりません");
            return 4;
        }
        Ok(status) if !status.success() => {
            println!("ERROR: gh が未認証です。gh auth login を実行してください");
            return 4;
        }
        Ok(_) => {}
    }
    // 待つ間の取得は gh の失敗を 0 件として扱うので、PR が在るかはここで 1 度だけ確かめ、「PR が無い」が時間切れに
    // 化けないようにする
    if crate::gh::quiet_output_for_repo(&pr.repo, &["pr", "view", &pr.number, "--json", "number"])
        .is_none()
    {
        println!(
            "ERROR: PR #{} が見つからないか、アクセスできません",
            pr.number
        );
        return 3;
    }

    let since = match since {
        Some(since) => {
            println!("INFO: {since} より新しいレビューの到着を待ちます");
            since
        }
        None => match last_request(&pr, &canonical) {
            Ok(since) => {
                println!("INFO: 最後のレビュー要求は {since}。それ以降のレビューの到着を待ちます");
                since
            }
            Err(code) => return code,
        },
    };

    let mut elapsed = 0u64;
    for interval in intervals("GH_WAIT_INTERVALS", INTERVALS) {
        if arrived(&pr, &since, elapsed) {
            return 0;
        }
        if copilot_run_failed(&pr) {
            println!(
                "ERROR: PR #{} の head でレビューの実行が失敗しています。待ってもレビューは来ません",
                pr.number
            );
            println!(
                "次の一手: /gh-actions-check {canonical} で原因を確認してください（レビュー用トークンの枯渇・内部エラーなど）"
            );
            return 7;
        }
        println!("待機中: レビュー未着（経過{elapsed}秒）。{interval}秒後に再確認します");
        std::thread::sleep(Duration::from_secs(interval));
        elapsed += interval;
    }
    if arrived(&pr, &since, elapsed) {
        return 0;
    }
    println!(
        "TIMEOUT: 約10分待ちましたがPR #{} に新しいレビューが来ていません",
        pr.number
    );
    println!("次の一手: /gh-actions-check {canonical} で Actions の状況を診断してください");
    1
}

/// 最後の Copilot へのレビューの要求の時刻。要求が見つかるまで少し読み直す（gh pr create の直後に呼ばれても
/// 取りこぼさない）。見つからないか取れなければ、理由を出して終了コード（6）
fn last_request(pr: &pr_url::Pr, canonical: &str) -> Result<String, i32> {
    for interval in
        std::iter::once(0).chain(intervals("GH_WAIT_REQUEST_INTERVALS", REQUEST_INTERVALS))
    {
        std::thread::sleep(Duration::from_secs(interval));
        let Ok(times) = gh_review_requests::request_times(pr) else {
            println!(
                "ERROR: PR #{} のレビュー要求を取得できません（timeline API が失敗）",
                pr.number
            );
            return Err(6);
        };
        if let Some(last) = times.last() {
            return Ok(last.clone());
        }
    }
    println!(
        "ERROR: PR #{} には Copilot へのレビュー要求がありません。要求していないレビューは来ないので待ちません",
        pr.number
    );
    let review_scripts = super::claude_root()
        .join("skills/gh-pr-review/scripts")
        .display()
        .to_string()
        .replace('\\', "/");
    println!(
        "次の一手: {review_scripts}/request-rereview.sh {canonical} でレビューを要求できます（任意。head のローカルレビューの記録があればマージできる。要求の前に、/gh-pr-review の「ローカルレビュー」の手順で head に /code-review を回して {review_scripts}/record-local-review.sh で記録する）"
    );
    Err(6)
}

fn intervals(var: &str, default: &str) -> Vec<u64> {
    std::env::var(var)
        .unwrap_or_else(|_| default.to_string())
        .split_whitespace()
        .filter_map(|s| s.parse().ok())
        .collect()
}

/// PR のレビューの一覧。取れなければ空の一覧として、待ちを続ける
fn reviews(pr: &pr_url::Pr) -> Vec<Value> {
    crate::gh::quiet_output_for_repo(&pr.repo, &["pr", "view", &pr.number, "--json", "reviews"])
        .and_then(|out| serde_json::from_slice::<Value>(&out).ok())
        .and_then(|v| v["reviews"].as_array().cloned())
        .unwrap_or_default()
}

/// since より後に Copilot のレビューが提出されていれば、報告して true（ISO 8601 は文字列の順が時刻の順）。
/// 待っているのは要求した Copilot のレビューだけ。PR の作成者がインラインコメントに返信すると作成者の名義の
/// COMMENTED のレビューが作られるので、投稿者を問わずに見ると返信を到着と誤る。reviews の author には種別が無いので、
/// gh-review-requests と同じく login に copilot を含むかで見分ける。投稿者が消えたレビューは author が null
fn arrived(pr: &pr_url::Pr, since: &str, elapsed: u64) -> bool {
    let latest = reviews(pr)
        .iter()
        .filter(|r| {
            r["author"]["login"]
                .as_str()
                .is_some_and(|l| l.to_ascii_lowercase().contains("copilot"))
        })
        .filter_map(|r| r["submittedAt"].as_str().map(str::to_string))
        .max();
    let Some(latest) = latest.filter(|at| at.as_str() > since) else {
        return false;
    };
    println!(
        "OK: PR #{} に新しいレビューが到着しました（{latest}、待機{elapsed}秒）",
        pr.number
    );
    for review in reviews(pr) {
        println!(
            "- {}: {}",
            text(&review["author"]["login"]),
            text(&review["state"])
        );
    }
    true
}

fn text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// head の Copilot のチェックが失敗しているか（トークン切れ、内部エラーでレビューが走らなかった。待っても来ないので、
/// 10 分待たずに切り上げる）。head は待つ間の push で変わるので毎回取り直す（古い head の失敗で打ち切らない）。
/// 取れなければ失敗していないとして待ちを続ける（誤って打ち切らない）
fn copilot_run_failed(pr: &pr_url::Pr) -> bool {
    let Some(head) = crate::gh::quiet_output_for_repo(
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
    ) else {
        return false;
    };
    let head = String::from_utf8_lossy(&head).trim().to_string();
    if head.is_empty() {
        return false;
    }
    // チェックの多い PR で取りこぼさないよう全ページを見る。--paginate に配列を返す --jq を渡すと不正な JSON に
    // なるので、名前を 1 つずつ出させる
    crate::gh::quiet_output_for_repo(
        &pr.repo,
        &[
            "api",
            "--paginate",
            &format!("repos/{{owner}}/{{repo}}/commits/{head}/check-runs?per_page=100"),
            "--jq",
            r#".check_runs[]
          | select((.name | ascii_downcase | test("copilot"))
                   and .status == "completed"
                   and ((.conclusion // "") | IN("success", "neutral", "skipped") | not))
          | .name"#,
        ],
    )
    .is_some_and(|names| names.iter().any(|b| !b.is_ascii_whitespace()))
}
