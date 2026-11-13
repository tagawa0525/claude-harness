//! `reset-rounds.sh <PR の URL> <理由>`: レビューの周回を 0 から数え直す印のコメントを PR に付ける（/gh-pr-review の
//! Step 6）。ユーザーの追加の要望に対応した変更は、それまでの指摘への対応とは別の仕事なので、新しい周回として数える。
//! decide-next.sh は最後の印より後のレビュー要求だけを数える。理由は PR に残り、いつ誰の要望で数え直したかを後から追える

use super::pr_url;

const USAGE: &str = "Usage: reset-rounds.sh <PR の URL> <理由>";
/// decide-next.sh が探す印。形を変えるときは decide-next.sh も合わせる
const MARK: &str = "<!-- review-rounds-reset -->";

pub fn main(args: &[String]) -> i32 {
    // 理由を引用符で囲み忘れると語ごとの引数になり、最初の語だけが PR に残るので、数をちょうど 2 つにする
    let [url, reason] = args else {
        eprintln!("{USAGE}");
        return 1;
    };
    let Some(pr) = pr_url::or_usage(pr_url::parse(url), USAGE) else {
        return 1;
    };
    if reason.is_empty() {
        eprintln!("{USAGE}");
        return 1;
    }
    crate::gh::run_for_repo(
        &pr.repo,
        &[
            "pr",
            "comment",
            &pr.number,
            "--body",
            &format!("{MARK}\nレビューの周回を数え直す: {reason}"),
        ],
    )
}
