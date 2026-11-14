//! `resolve-thread.sh <PR の URL> <comment_id> [--allow-human]`: comment_id（スレッド内の任意のレビューコメントの数の
//! ID）を含むレビュースレッドを resolve する（/gh-pr-review の Step 5）。REST には resolve の API が無いので、GraphQL の
//! resolveReviewThread を使う。
//!
//! スレッドを閉じるのはレビュアーの権限。bot（Copilot 等）が起こしたスレッドは対応後に resolve するが、人間が起こした
//! スレッドは返信だけにして相手に委ねる。人間のスレッドは既定で拒み、ユーザーが明示的に求めたときだけ
//! --allow-human で resolve する。生の GraphQL でこの判定を迂回しないよう、PreToolUse の guard-gh-api ルールが
//! resolveReviewThread を拒む

use super::pr_url;
use serde_json::Value;

const USAGE: &str = "Usage: resolve-thread.sh <PR の URL> <comment_id> [--allow-human]";

/// スレッド 100 件、スレッド内のコメント 100 件まで
const THREADS_QUERY: &str = "
    query($owner: String!, $name: String!, $number: Int!) {
      repository(owner: $owner, name: $name) {
        pullRequest(number: $number) {
          reviewThreads(first: 100) {
            pageInfo { hasNextPage }
            nodes {
              id
              isResolved
              comments(first: 100) { nodes { databaseId author { __typename login } } }
            }
          }
        }
      }
    }";

const RESOLVE_MUTATION: &str = "
    mutation($id: ID!) {
      resolveReviewThread(input: { threadId: $id }) {
        thread { id isResolved }
      }
    }";

pub fn main(args: &[String]) -> i32 {
    let Some(pr) = pr_url::or_usage(
        pr_url::parse(args.first().map(String::as_str).unwrap_or("")),
        USAGE,
    ) else {
        return 1;
    };
    let (id, allow_human) = match args {
        [_, id] if !id.is_empty() => (id, false),
        [_, id, flag] if !id.is_empty() && flag == "--allow-human" => (id, true),
        [_, id, flag] if !id.is_empty() => {
            eprintln!("ERROR: 不明なオプション: {flag}");
            return 1;
        }
        _ => {
            eprintln!("{USAGE}");
            return 1;
        }
    };
    let Some(comment_id) = id
        .bytes()
        .all(|b| b.is_ascii_digit())
        .then(|| id.parse::<u64>().ok())
        .flatten()
    else {
        eprintln!(
            "ERROR: comment_id は数値で指定してください（URL の #discussion_r の後ろの数字）: '{id}'"
        );
        return 1;
    };

    let stdout = match crate::gh::output_for_repo(
        &pr.repo,
        &[
            "api",
            "graphql",
            "-F",
            "owner={owner}",
            "-F",
            "name={repo}",
            "-F",
            &format!("number={}", pr.number),
            "-f",
            &format!("query={THREADS_QUERY}"),
            "--jq",
            ".data.repository.pullRequest.reviewThreads",
        ],
    ) {
        Ok(stdout) => stdout,
        Err(code) => return code,
    };
    let threads: Value = serde_json::from_slice(&stdout).unwrap_or(Value::Null);
    let (Some(has_next), Some(nodes)) = (
        threads["pageInfo"]["hasNextPage"].as_bool(),
        threads["nodes"].as_array(),
    ) else {
        eprintln!(
            "ERROR: gh api graphql の出力がレビュースレッドの一覧でない: {}",
            String::from_utf8_lossy(&stdout)
        );
        return 1;
    };
    if has_next {
        eprintln!("ERROR: レビュースレッドが 100 件を超えています（未対応）");
        return 1;
    }
    let Some(thread) = nodes.iter().find(|thread| {
        comments(thread)
            .iter()
            .any(|c| c["databaseId"].as_u64() == Some(comment_id))
    }) else {
        eprintln!(
            "ERROR: コメント {comment_id} を含むスレッドが PR #{} に見つかりません",
            pr.number
        );
        return 1;
    };
    let thread_id = thread["id"].as_str().unwrap_or("");
    if thread["isResolved"].as_bool() == Some(true) {
        outln!("already resolved: {thread_id}");
        return 0;
    }
    // スレッドを起こした人（先頭のコメントの投稿者）が bot でなければ既定で拒む。消えたアカウント（author が null）も
    // bot と確かめられないので拒む
    let starter = &comments(thread)[0]["author"];
    let starter_type = starter["__typename"].as_str().unwrap_or("unknown");
    let starter_login = starter["login"].as_str().unwrap_or("unknown");
    if starter_type != "Bot" && !allow_human {
        eprintln!(
            "ERROR: スレッド {thread_id} は人間（{starter_login}）が起こしたものです。返信だけ行い、resolve はレビュアーに委ねてください"
        );
        eprintln!(
            "       ユーザーから明示的に resolve を求められた場合のみ --allow-human を付けて再実行してください"
        );
        return 1;
    }
    crate::gh::run_for_repo(
        &pr.repo,
        &[
            "api",
            "graphql",
            "-F",
            &format!("id={thread_id}"),
            "-f",
            &format!("query={RESOLVE_MUTATION}"),
            "--jq",
            r#""resolved: \(.data.resolveReviewThread.thread.id) (isResolved=\(.data.resolveReviewThread.thread.isResolved))""#,
        ],
    )
}

/// スレッドのコメント（見つけたスレッドは comment_id を含むので空でない）
fn comments(thread: &Value) -> &[Value] {
    thread["comments"]["nodes"]
        .as_array()
        .map_or(&[], Vec::as_slice)
}
