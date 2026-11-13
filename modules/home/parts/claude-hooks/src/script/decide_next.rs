//! `decide-next.sh <PR の URL>`: レビューへの対応の後に、次に取る一手を 1 つに決める（/gh-pr-review の Step 6）。
//!
//! 出力（テキスト）:
//! - `ROUND: <n>`: 周回の数（情報。上限は無い）。Copilot へのレビューの要求の件数（PR を作ったときの自動の要求を含む）と
//!   Copilot のレビューの件数の多い方（要求のイベントを伴わずにレビューが付くリポジトリでも周回を見失わない）。
//!   reset-rounds の印があれば、最後の印より後の要求の件数
//! - `RESPONSE: review | none`: 直近の要求より後に提出された Copilot のレビューがあるか（要求が無ければ最初のレビューを
//!   対象にする）
//! - `HEAD` / `REVIEWED` / `HEAD_REVIEWED: yes|no`: head とレビューの対象のコミット。対応を push した後に要求を忘れて
//!   いないかはこれで決まる
//! - `UNRESOLVED: <n>`: 未解決のレビュースレッドの数
//! - `LOCAL_REVIEWED: yes|no`: head にローカルレビューの記録（High なし。最後の記録から rebase だけした head を含む）が
//!   あるか。判定は `claude-hooks local-review-covers`（pre-merge-check と同じ判定）で、rebase だけかは cwd の clone で比べる
//! - RESPONSE が review のとき: `REVIEW_ID`、`HEADLINE`、`INLINE_COMMENTS`、`SUPPRESSED_COMMENTS`、`SEVERITY`、v2 なら
//!   `REMAINING_HIGH` と `PREVIOUSLY_MISSED`（get-latest-review の値）
//! - `VERDICT: <判定>`（意味と次の一手は /gh-pr-review の Step 6 の表）
//! - Suppressed comments か Previously missed があれば、`--- suppressed ---` / `--- previously missed ---` の後に本文
//!
//! 「対応する」「push する」「要求する」を 1 つの判定に潰すと、どの段階にいるかがモデルの記憶頼りになり、要求を忘れる。
//! 段階ごとに VERDICT を分け、次の一手を 1 つに決める。Copilot の PR のコメントはレビューではないので、応答にも周回にも
//! 数えない

use super::{get_latest_review, get_review_comments, gh_review_requests, pr_url};
use std::collections::BTreeSet;

const USAGE: &str = "Usage: decide-next.sh <PR の URL>";
/// 印はリポジトリに書き込める人のものだけ受ける。形は reset-rounds が書くもの
const RESETS: &str = r#".[] | select((.body | startswith("<!-- review-rounds-reset -->"))
                     and (.author_association | IN("OWNER", "MEMBER", "COLLABORATOR")))
           | .created_at"#;

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
    match decide(&pr) {
        Ok(text) => {
            print!("{text}");
            0
        }
        Err(code) => code,
    }
}

fn decide(pr: &pr_url::Pr) -> Result<String, i32> {
    let number = &pr.number;
    // 取れなければ空として続けない（周回の数を少なく見積もる）
    let Ok(mut requests) = gh_review_requests::request_times(pr) else {
        eprintln!(
            "ERROR: PR #{number} のレビュー要求を取得できませんでした（周回数が数えられません）"
        );
        return Err(1);
    };
    let last_request_at = requests.last().cloned().unwrap_or_default();
    // 周回を数え直した印（reset-rounds のコメント）。最後の印より後の要求だけを数える。取れなければ、印を見落とさない
    // よう止める
    let Ok(resets) = crate::gh::output_for_repo(
        &pr.repo,
        &[
            "api",
            "--paginate",
            &format!("repos/{{owner}}/{{repo}}/issues/{number}/comments?per_page=100"),
            "--jq",
            RESETS,
        ],
    ) else {
        eprintln!("ERROR: PR #{number} のコメントを取得できませんでした（周回数が数えられません）");
        return Err(1);
    };
    let last_reset_at = String::from_utf8_lossy(&resets)
        .lines()
        .filter(|l| !l.is_empty())
        .max()
        .map(str::to_string);
    if let Some(reset) = &last_reset_at {
        requests.retain(|at| at.as_str() > reset.as_str());
    }
    let request_count = requests.len() as u64;

    // 最新の Copilot のレビュー
    let latest = get_latest_review::summary(pr)?;
    let field = |name: &str| -> String {
        let prefix = format!("{name}: ");
        latest
            .lines()
            .find_map(|l| l.strip_prefix(&prefix))
            .unwrap_or("")
            .to_string()
    };
    let review_count: u64 = field("ROUND").parse().unwrap_or(0);
    let review_id = field("REVIEW_ID");
    let (mut last_review_at, mut reviewed_sha) = (String::new(), String::new());
    if review_count > 0
        && let Some(out) = crate::gh::quiet_output_for_repo(
            &pr.repo,
            &[
                "api",
                &format!("repos/{{owner}}/{{repo}}/pulls/{number}/reviews/{review_id}"),
                "--jq",
                r#""\(.submitted_at) \(.commit_id)""#,
            ],
        )
    {
        let line = String::from_utf8_lossy(&out);
        let mut words = line.split_whitespace();
        last_review_at = words.next().unwrap_or("").to_string();
        reviewed_sha = words.next().unwrap_or("").to_string();
    }

    let head = crate::gh::output_for_repo(
        &pr.repo,
        &[
            "pr",
            "view",
            number,
            "--json",
            "headRefOid",
            "-q",
            ".headRefOid",
        ],
    )
    .map(|out| String::from_utf8_lossy(&out).trim().to_string())
    .unwrap_or_default();
    // 空の SHA は、記録の無い出力の空行と一致して、ローカルレビュー済みに見える
    if head.is_empty() {
        eprintln!("ERROR: PR #{number} の head を取得できませんでした");
        return Err(1);
    }
    // コメントはスレッドの中の返信も平らに並ぶので、返信を数えないよう thread_id で畳む
    let Ok(comments) = get_review_comments::comments(pr, true) else {
        eprintln!("ERROR: PR #{number} の未解決スレッドを取得できませんでした");
        return Err(1);
    };
    let unresolved = comments
        .iter()
        .map(|c| c["thread_id"].to_string())
        .collect::<BTreeSet<_>>()
        .len();
    let Some(local_reviewed) = local_review_covers(pr, &head) else {
        eprintln!("ERROR: PR #{number} のローカルレビューの記録を確かめられませんでした");
        return Err(1);
    };

    // 数え直した後は要求だけで数える（レビューの件数は PR の全体の通算なので）
    let round = if last_reset_at.is_some() {
        request_count
    } else {
        request_count.max(review_count)
    };
    // 直近の要求より後に提出されたレビューだけを今の周の応答とみなす（ISO 8601 は文字列の順が時刻の順）
    let response = !last_review_at.is_empty() && last_review_at > last_request_at;
    let head_reviewed = !reviewed_sha.is_empty() && reviewed_sha == head;

    let mut out = String::new();
    let mut line = |s: String| {
        out.push_str(&s);
        out.push('\n');
    };
    line(format!("ROUND: {round}"));
    line(format!(
        "RESPONSE: {}",
        if response { "review" } else { "none" }
    ));
    line(format!("HEAD: {head}"));
    line(format!("REVIEWED: {reviewed_sha}"));
    line(format!("HEAD_REVIEWED: {}", yes_no(head_reviewed)));
    line(format!("UNRESOLVED: {unresolved}"));
    line(format!("LOCAL_REVIEWED: {}", yes_no(local_reviewed)));

    if !response {
        // head のローカルレビューの記録があれば、Copilot のレビューを待たない
        let verdict = match (local_reviewed, unresolved > 0) {
            (true, true) => "ACT",
            (true, false) => "STOP_LOCAL_REVIEWED",
            (false, _) => "WAITING",
        };
        line(format!("VERDICT: {verdict}"));
        return Ok(out);
    }

    let failed = field("REVIEW_FAILED") == "yes";
    let successful: u64 = field("SUCCESSFUL_REVIEWS").parse().unwrap_or(0);
    let inline: u64 = field("INLINE_COMMENTS").parse().unwrap_or(0);
    let suppressed: u64 = field("SUPPRESSED_COMMENTS").parse().unwrap_or(0);
    let severity = field("SEVERITY");
    let missed: u64 = field("PREVIOUSLY_MISSED").parse().unwrap_or(0);
    line(format!("REVIEW_ID: {review_id}"));
    line(format!("HEADLINE: {}", field("HEADLINE")));
    line(format!("INLINE_COMMENTS: {}", field("INLINE_COMMENTS")));
    line(format!(
        "SUPPRESSED_COMMENTS: {}",
        field("SUPPRESSED_COMMENTS")
    ));
    line(format!("SEVERITY: {severity}"));
    if severity == "v2" {
        line(format!("REMAINING_HIGH: {}", field("REMAINING_HIGH")));
        line(format!("PREVIOUSLY_MISSED: {}", field("PREVIOUSLY_MISSED")));
    }
    // 段階ごとに 1 つの次の一手へ落とす。順に意味がある: レビューの失敗 → 未対応 → ローカルレビューの記録 → 対応済み。
    // 一部だけ直して push した状態（未解決あり、head は未レビュー）では、記録より先に残りの対応を促す。文書だけの PR は、
    // 一度でも成功したレビューがあれば後のレビューの失敗で止めない（pre-merge-check と同じ条件）。最初のレビューが
    // 失敗しただけの PR は止める
    let verdict = if failed && !(successful > 0 && docs_only(pr)) {
        // head のローカルレビューの記録があれば、Copilot のレビューの失敗は止めない
        if !local_reviewed {
            "REVIEW_FAILED"
        } else if unresolved > 0 {
            "ACT"
        } else {
            "STOP_LOCAL_REVIEWED"
        }
    } else if unresolved > 0 {
        "ACT"
    } else if failed {
        "STOP_DOCS_REVIEWED"
    } else if !head_reviewed {
        if docs_only(pr) {
            "STOP_DOCS_REVIEWED"
        } else if !local_reviewed {
            "LOCAL_REVIEW_NEEDED"
        } else {
            "STOP_LOCAL_REVIEWED"
        }
    } else if missed > 0 {
        "STOP_PREVIOUSLY_MISSED"
    } else if inline > 0 {
        "STOP_DECLINED"
    } else if suppressed > 0 {
        "STOP_SUPPRESSED_ONLY"
    } else {
        "STOP_CLEAN"
    };
    line(format!("VERDICT: {verdict}"));
    if suppressed > 0 || missed > 0 {
        let mut on = false;
        for l in latest.lines() {
            on = on || l == "--- suppressed ---" || l == "--- previously missed ---";
            if on {
                line(l.to_string());
            }
        }
    }
    Ok(out)
}

fn yes_no(b: bool) -> &'static str {
    if b { "yes" } else { "no" }
}

/// PR の変更ファイルがすべて文書（docs/ の下）か。取れないか 1 件も無ければ、文書だけとみなさない（再レビューを求める
/// 側に倒す）。gh pr view --json files は先頭の 100 件で切れるので、ページをめくる REST の API で取る。移動は前の
/// パスも数える（src/ から docs/ への移動は filename だけだと文書だけに見える）
fn docs_only(pr: &pr_url::Pr) -> bool {
    let Some(out) = crate::gh::quiet_output_for_repo(
        &pr.repo,
        &[
            "api",
            "--paginate",
            &format!("repos/{{owner}}/{{repo}}/pulls/{}/files", pr.number),
            "--jq",
            ".[] | .filename, (.previous_filename // empty)",
        ],
    ) else {
        return false;
    };
    let files = String::from_utf8_lossy(&out);
    let mut files = files.lines().filter(|l| !l.is_empty()).peekable();
    files.peek().is_some() && files.all(|f| f.starts_with("docs/"))
}

/// head がローカルレビューの記録で覆われているか（`claude-hooks local-review-covers`。CLAUDE_HOOKS_BIN があればそれ、
/// 無ければ自分を起動する）。確かめられなければ、理由を出して None（記録が無いとみなして判定を進めない。Copilot に
/// 出すかどうかを誤るより、止めて知らせる）。
/// プロセスの中の関数（`rules::local_review_covers_for_pr`）を呼ばないのは、判定の表の sh のテストが CLAUDE_HOOKS_BIN
/// で判定を偽物に差し替えて、表の分かれ道を確かめるため（判定そのものは hooks.sh と pre_merge_check.rs が確かめる）
fn local_review_covers(pr: &pr_url::Pr, head: &str) -> Option<bool> {
    let bin = std::env::var_os("CLAUDE_HOOKS_BIN")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::current_exe().ok())?;
    let status = std::process::Command::new(&bin)
        .args(["local-review-covers", &pr.repo, &pr.number, head])
        .env("GH_REPO", &pr.repo)
        .env("GH_HOST", "github.com")
        .status();
    let install = "home-manager の rebuild（Windows は claude-harness の README の「Windows で使う」の cargo install）";
    match status.map(|s| s.code()) {
        Ok(Some(0)) => Some(true),
        Ok(Some(1)) => Some(false),
        Err(_) | Ok(Some(127)) => {
            eprintln!(
                "ERROR: {} がありません。{install}で入れてください（claude-sync はバイナリを入れない）",
                bin.display()
            );
            None
        }
        Ok(Some(2)) => {
            // claude-sync はスクリプトだけを入れ替え、~/.claude/bin のバイナリは入れ直すまで前の版のまま
            eprintln!(
                "ERROR: {} が使い方の誤り（終了コード 2）で止まりました。local-review-covers を知らない古いバイナリなら {install}で入れ直してください",
                bin.display()
            );
            None
        }
        Ok(code) => {
            eprintln!(
                "ERROR: ローカルレビューの記録を確かめられませんでした（{} local-review-covers の終了コード {}）",
                bin.display(),
                code.map_or("なし".to_string(), |c| c.to_string())
            );
            None
        }
    }
}
