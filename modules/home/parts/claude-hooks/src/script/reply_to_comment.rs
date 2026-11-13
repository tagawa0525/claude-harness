//! `reply-to-comment.sh <PR の URL> <comment_id> <body>`: レビューコメントに返信し、作った返信の id、html_url、body を
//! 出す（/gh-pr-review の Step 5）

use super::pr_url;

const USAGE: &str = "Usage: reply-to-comment.sh <PR の URL> <comment_id> <body>";

pub fn main(args: &[String]) -> i32 {
    let pr = match pr_url::parse(args.first().map(String::as_str).unwrap_or("")) {
        Ok(pr) => pr,
        Err(why) => {
            eprintln!("{why}");
            eprintln!("{USAGE}");
            return 1;
        }
    };
    // 本文を引用符で囲み忘れると語ごとの引数になり、最初の語だけが返信になるので、数をちょうど 3 つにする
    let [_, id, body] = args else {
        eprintln!("{USAGE}");
        return 1;
    };
    if id.is_empty() || body.is_empty() {
        eprintln!("{USAGE}");
        return 1;
    }
    // id は API のパスに埋めるので、数でない値（URL の断片、`..`、クエリ）は別のエンドポイントに当たる
    if !id.bytes().all(|b| b.is_ascii_digit()) {
        eprintln!("ERROR: comment_id は数で渡してください: '{id}'");
        return 1;
    }
    crate::gh::run_for_repo(
        &pr.repo,
        &[
            "api",
            &format!(
                "repos/{{owner}}/{{repo}}/pulls/{}/comments/{id}/replies",
                pr.number
            ),
            "-f",
            &format!("body={body}"),
            "--jq",
            "{id: .id, html_url: .html_url, body: .body}",
        ],
    )
}
