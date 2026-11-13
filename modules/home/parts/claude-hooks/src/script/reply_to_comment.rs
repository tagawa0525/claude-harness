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
    let (Some(id), Some(body)) = (args.get(1), args.get(2)) else {
        eprintln!("{USAGE}");
        return 1;
    };
    if id.is_empty() || body.is_empty() {
        eprintln!("{USAGE}");
        return 1;
    }
    super::gh(
        &pr,
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
