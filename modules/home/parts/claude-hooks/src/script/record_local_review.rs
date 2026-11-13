//! `record-local-review.sh <PR の URL> <SHA> <high> <medium> <low>`: ローカルの /code-review の結果を、PR の head の
//! 記録としてコメントに残す（/gh-pr-review の Step 6）。SHA は /code-review を回したコミットで、PR の head と一致
//! しなければ記録しない（レビューの後に head が動いたなら、その head はレビューしていない）。件数は、その head で
//! 確かめて正しいと判断した指摘の数で、後に Copilot が出した数と比べるために残す。
//!
//! 記録はマージの hook（pre-merge-check）が Copilot のレビューの代わりに数えるので、差分を読んだだけのコミットは
//! 記録しない。記録を数えるのは書き込める人のコメントだけなので、push できないリポジトリ（上流の PR）には付けない

use super::pr_url;
use regex::Regex;
use std::sync::LazyLock;

const USAGE: &str =
    "Usage: record-local-review.sh <PR の URL> <レビューした SHA> <high> <medium> <low>";

static SHA: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[0-9a-f]{7,40}$").unwrap());
/// 読む側は `high=0 ` の字面で比べるので、先頭の 0 を許さない
static COUNT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(0|[1-9][0-9]*)$").unwrap());

pub fn main(args: &[String]) -> i32 {
    let [url, reviewed, counts @ ..] = args else {
        eprintln!("{USAGE}");
        return 1;
    };
    if counts.len() != 3 {
        eprintln!("{USAGE}");
        return 1;
    }
    let Some(pr) = pr_url::or_usage(pr_url::parse(url), USAGE) else {
        return 1;
    };
    if !SHA.is_match(reviewed) {
        eprintln!("{USAGE}（SHA は 7〜40 桁の 16 進: '{reviewed}'）");
        return 1;
    }
    let mut n = [0u64; 3];
    for (slot, count) in n.iter_mut().zip(counts) {
        match COUNT.is_match(count).then(|| count.parse().ok()).flatten() {
            Some(value) => *slot = value,
            None => {
                eprintln!("{USAGE}（件数は 0 以上の整数で、先頭に 0 を付けない: '{count}'）");
                return 1;
            }
        }
    }
    let [high, medium, low] = n;

    let head = match crate::gh::output_for_repo(
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
    ) {
        Ok(out) => String::from_utf8_lossy(&out)
            .trim_end_matches('\n')
            .to_string(),
        Err(_) => String::new(),
    };
    // 空の SHA は、記録の無い出力の空行と一致して、ローカルレビュー済みに見える
    if head.is_empty() {
        eprintln!(
            "ERROR: PR #{} の head を取得できませんでした（記録を付けていません）",
            pr.number
        );
        return 1;
    }
    let Ok(can_push) = crate::gh::output_for_repo(
        &pr.repo,
        &["api", "repos/{owner}/{repo}", "--jq", ".permissions.push"],
    ) else {
        eprintln!(
            "ERROR: {} の権限を取得できませんでした（記録を付けていません）",
            pr.repo
        );
        return 1;
    };
    if String::from_utf8_lossy(&can_push).trim_end_matches('\n') != "true" {
        eprintln!(
            "ERROR: {} に push できないので、記録を付けても数えられません（記録を付けていません）",
            pr.repo
        );
        return 1;
    }
    if !head.starts_with(reviewed.as_str()) {
        eprintln!(
            "ERROR: レビューした {} は PR #{} の head（{}）ではありません。head を push してからレビューし直してください（記録を付けていません）",
            short(reviewed),
            pr.number,
            short(&head)
        );
        return 1;
    }
    let body = format!(
        "{}\nローカルの /code-review で {} を確かめた: High {high} / Medium {medium} / Low {low}",
        crate::rules::local_review_mark(&head, high, medium, low),
        short(&head)
    );
    let code = crate::gh::run_for_repo(&pr.repo, &["pr", "comment", &pr.number, "--body", &body]);
    if code != 0 {
        return code;
    }
    println!("RECORDED: {head}");
    if high > 0 {
        println!(
            "NOTE: High の残る記録は、レビュー済みとして数えない。直して push し、回し直して記録する"
        );
    }
    0
}

fn short(sha: &str) -> &str {
    sha.get(..7).unwrap_or(sha)
}
