//! `resolve-thread.sh <PR の URL> <comment_id | thread_id> [--allow-human]`: comment_id（スレッド内の任意のレビュー
//! コメントの数の ID）を含むか、thread_id（スレッドの node ID。`PRRT_…`）の一致するレビュースレッドを resolve する
//! （/gh-pr-review の Step 5）。どちらの ID も、URL の PR のスレッドの一覧から探す。REST には resolve の API が無いので、
//! GraphQL の resolveReviewThread を使う。
//!
//! スレッドを閉じるのはレビュアーの権限。bot（Copilot 等）が起こしたスレッドは対応後に resolve するが、人間が起こした
//! スレッドは返信だけにして相手に委ねる。人間のスレッドは既定で拒み、ユーザーが明示的に求めたときだけ
//! --allow-human で resolve する。生の GraphQL でこの判定を迂回しないよう、PreToolUse の guard-gh-api ルールが
//! resolveReviewThread を拒む

use super::pr_url;
use serde_json::Value;

const USAGE: &str = "Usage: resolve-thread.sh <PR の URL> <comment_id | thread_id> [--allow-human]";

/// 渡されたスレッドの探し方
enum Target {
    /// スレッド内のレビューコメントの数の ID（URL の #discussion_r の後ろ）
    Comment(u64),
    /// スレッドの node ID（get-review-comments.sh の thread_id）
    Thread(String),
}

/// スレッドの node ID の接頭辞
const THREAD_ID_PREFIX: &str = "PRRT_";

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
    let target = if id.bytes().all(|b| b.is_ascii_digit())
        && let Ok(comment_id) = id.parse::<u64>()
    {
        Target::Comment(comment_id)
    } else if id
        .strip_prefix(THREAD_ID_PREFIX)
        .is_some_and(|rest| !rest.is_empty())
    {
        Target::Thread(id.clone())
    } else {
        eprintln!(
            "ERROR: comment_id（数値。URL の #discussion_r の後ろの数字）か thread_id（PRRT_…）で指定してください: '{id}'"
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
    // スレッドの ID も URL の PR の一覧の中だけで探し、別の PR のスレッドを閉じない
    let found = nodes.iter().find(|thread| match &target {
        Target::Comment(comment_id) => comments(thread)
            .iter()
            .any(|c| c["databaseId"].as_u64() == Some(*comment_id)),
        Target::Thread(thread_id) => thread["id"].as_str() == Some(thread_id),
    });
    let Some(thread) = found else {
        match &target {
            Target::Comment(comment_id) => eprintln!(
                "ERROR: コメント {comment_id} を含むスレッドが PR #{} に見つかりません",
                pr.number
            ),
            Target::Thread(thread_id) => eprintln!(
                "ERROR: スレッド {thread_id} が PR #{} に見つかりません（ID の誤りか、別の PR のスレッド。別の PR のスレッドは resolve しません）",
                pr.number
            ),
        }
        return 1;
    };
    let thread_id = thread["id"].as_str().unwrap_or("");
    if thread["isResolved"].as_bool() == Some(true) {
        println!("already resolved: {thread_id}");
        return 0;
    }
    // スレッドを起こした人（先頭のコメントの投稿者）が bot でなければ既定で拒む。消えたアカウント（author が null）と、
    // スレッドの ID で見つけたコメントの無いスレッドも、bot と確かめられないので拒む
    let starter = comments(thread)
        .first()
        .map_or(&Value::Null, |c| &c["author"]);
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

/// スレッドのコメント
fn comments(thread: &Value) -> &[Value] {
    thread["comments"]["nodes"]
        .as_array()
        .map_or(&[], Vec::as_slice)
}
