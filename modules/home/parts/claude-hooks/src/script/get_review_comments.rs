//! `get-review-comments.sh <PR の URL> [--unresolved]`: PR のレビューコメントを JSON の配列で出す（/gh-pr-review の
//! Step 2）。1 要素が 1 コメント（スレッド内の返信も含み、作った順）で、id、node_id、path、line、body、user、
//! user_type（Bot / User。resolve の可否の判断に使う）、created_at、in_reply_to_id、html_url、thread_id
//! （resolve-thread.sh の対象）、is_resolved、is_outdated を持つ。REST の /pulls/{n}/comments はスレッドの resolve の
//! 状態を持たないので、GraphQL の reviewThreads から取る。--unresolved は resolve していないスレッドに限る

use super::pr_url;
use serde_json::{Value, json};

const USAGE: &str = "Usage: get-review-comments.sh <PR の URL> [--unresolved]";

/// スレッドは --paginate で全件。スレッド内のコメントは 100 件まで（ふつうは足りる。超えると末尾を取りこぼすので、
/// hasNextPage を見て警告する）
const QUERY: &str = "
    query($owner: String!, $name: String!, $number: Int!, $endCursor: String) {
      repository(owner: $owner, name: $name) {
        pullRequest(number: $number) {
          reviewThreads(first: 100, after: $endCursor) {
            pageInfo { hasNextPage endCursor }
            nodes {
              id
              isResolved
              isOutdated
              path
              line
              comments(first: 100) {
                pageInfo { hasNextPage }
                nodes {
                  databaseId
                  id
                  body
                  author { __typename login }
                  createdAt
                  url
                  replyTo { databaseId }
                }
              }
            }
          }
        }
      }
    }";

pub fn main(args: &[String]) -> i32 {
    let Some(pr) = pr_url::or_usage(
        pr_url::parse(args.first().map(String::as_str).unwrap_or("")),
        USAGE,
    ) else {
        return 1;
    };
    let unresolved_only = match args {
        [_] => false,
        [_, flag] if flag == "--unresolved" => true,
        [_, flag] => {
            eprintln!("ERROR: 不明なオプション: {flag}");
            return 1;
        }
        _ => {
            eprintln!("{USAGE}");
            return 1;
        }
    };
    match comments(&pr, unresolved_only) {
        Ok(comments) => {
            println!("{:#}", Value::Array(comments));
            0
        }
        Err(code) => code,
    }
}

/// PR のレビューコメント（モジュールの説明の要素の並び）。unresolved_only なら resolve していないスレッドに限る。
/// gh が失敗すれば、その終了コード（理由は標準エラーに出す）
pub fn comments(pr: &pr_url::Pr, unresolved_only: bool) -> Result<Vec<Value>, i32> {
    let stdout = crate::gh::output_for_repo(
        &pr.repo,
        &[
            "api",
            "graphql",
            "--paginate",
            "-F",
            "owner={owner}",
            "-F",
            "name={repo}",
            "-F",
            &format!("number={}", pr.number),
            "-f",
            &format!("query={QUERY}"),
            "--jq",
            ".data.repository.pullRequest.reviewThreads.nodes[]",
        ],
    )?;
    let threads: Result<Vec<Value>, _> = serde_json::Deserializer::from_slice(&stdout)
        .into_iter::<Value>()
        .collect();
    let Some(threads) = threads.ok().filter(|t| t.iter().all(is_thread)) else {
        eprintln!(
            "ERROR: gh api graphql の出力がレビュースレッドの並びでない: {}",
            String::from_utf8_lossy(&stdout)
        );
        return Err(1);
    };
    if threads
        .iter()
        .any(|t| t["comments"]["pageInfo"]["hasNextPage"].as_bool() == Some(true))
    {
        eprintln!("WARN: 100 件を超える返信を持つスレッドがあり、末尾の返信は省略されています");
    }
    let mut comments: Vec<Value> = threads
        .iter()
        .filter(|t| !unresolved_only || t["isResolved"].as_bool() != Some(true))
        .flat_map(|t| {
            t["comments"]["nodes"]
                .as_array()
                .into_iter()
                .flatten()
                .map(move |c| comment(t, c))
        })
        .collect();
    // 作った順（ISO 8601 の UTC の時刻なので文字列の順）。同じ時刻はスレッドとスレッド内の順を保つ
    comments.sort_by(|a, b| {
        a["created_at"]
            .as_str()
            .unwrap_or("")
            .cmp(b["created_at"].as_str().unwrap_or(""))
    });
    Ok(comments)
}

fn is_thread(thread: &Value) -> bool {
    thread["comments"]["nodes"].is_array()
}

fn comment(thread: &Value, c: &Value) -> Value {
    json!({
        "id": c["databaseId"],
        "node_id": c["id"],
        "path": thread["path"],
        "line": thread["line"],
        "body": c["body"],
        "user": c["author"]["login"],
        "user_type": c["author"]["__typename"],
        "created_at": c["createdAt"],
        "in_reply_to_id": c["replyTo"]["databaseId"],
        "html_url": c["url"],
        "thread_id": thread["id"],
        "is_resolved": thread["isResolved"],
        "is_outdated": thread["isOutdated"],
    })
}
