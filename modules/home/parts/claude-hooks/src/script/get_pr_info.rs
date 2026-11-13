//! `get-pr-info.sh <PR の URL | コメントの URL>`: PR の情報を JSON で出し、レビューコメントの URL
//! （`#discussion_r<id>`）ならその id を `comment_id` に、ほかは null を添える（/gh-pr-review の Step 1）

use super::pr_url;
use serde_json::Value;

const USAGE: &str = "Usage: get-pr-info.sh <PR の URL | コメントの URL>";
const FIELDS: &str = "number,title,url,state,reviewDecision,headRefName,baseRefName";

pub fn main(args: &[String]) -> i32 {
    let pr = match pr_url::parse_allowing_fragment(args.first().map(String::as_str).unwrap_or("")) {
        Ok(pr) => pr,
        Err(why) => {
            eprintln!("{why}");
            eprintln!("{USAGE}");
            return 1;
        }
    };
    let stdout =
        match crate::gh::output_for_repo(&pr.repo, &["pr", "view", &pr.number, "--json", FIELDS]) {
            Ok(stdout) => stdout,
            Err(code) => return code,
        };
    let mut info = match serde_json::from_slice::<Value>(&stdout) {
        Ok(Value::Object(info)) => info,
        _ => {
            eprintln!(
                "ERROR: gh pr view の出力が JSON のオブジェクトでない: {}",
                String::from_utf8_lossy(&stdout)
            );
            return 1;
        }
    };
    info.insert(
        "comment_id".to_string(),
        pr.fragment
            .as_deref()
            .and_then(review_comment_id)
            .map_or(Value::Null, Value::from),
    );
    println!("{:#}", Value::Object(info));
    0
}

/// レビューコメント（`discussion_r<id>`）の id。`#issuecomment-<id>` などは返信の API（pulls/comments）で返信できる
/// コメントでないので、PR 全体を指すとみなす
fn review_comment_id(fragment: &str) -> Option<u64> {
    let id = fragment.strip_prefix("discussion_r")?;
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    id.parse().ok()
}
