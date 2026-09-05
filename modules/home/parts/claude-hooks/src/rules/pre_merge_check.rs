//! pre-merge-check: gh pr merge 実行前にマージの前提条件を検証する
//!
//! CLAUDE.md の必須ゲートをツールレベルで強制する:
//! 1. マージ方式は --merge のみ（--squash / --rebase は禁止）
//! 2. --delete-branch でマージ後のブランチを削除する
//! 3. マージコミット本文に ## Why / ## What / ## Impact が揃っている
//! 4. CI チェックが未完了・失敗していない
//! 5. reviewDecision が CHANGES_REQUESTED / REVIEW_REQUIRED でない
//! 6. 未解決のレビュースレッドがない
//! 7. head が base（origin/main）より遅れていない（リベースしてからマージコミットする）
//!
//! 1〜3 はコマンド文字列だけで判定する。4〜7 は gh で GitHub に問い合わせ、
//! 問い合わせに失敗したら deny する（確認できない状態でマージさせない）。
//! gh の引数列は bash 版と同一に保つ（テストの偽 gh が引数の前方一致で応答する）

use super::Rule;
use super::pre_pr_create_check::{body_text, missing_headings};
use crate::gh;
use crate::input::Input;
use crate::output::Finding;
use crate::shell::{Arg, Shell, has_flag, opt_value};
use serde_json::Value;

pub struct PreMergeCheck;

/// 引数から PR 番号を探す。本文（--body / --subject）より前のトークンから、値を取るフラグの
/// 値を飛ばして最初の数値を採る（`gh pr merge -R owner/repo 12 …` の形にも対応）
fn pr_ref(args: &[Arg]) -> Option<String> {
    let mut skip = false;
    for a in args {
        let t = a.text.as_str();
        if skip {
            skip = false;
            continue;
        }
        if matches!(t, "--body" | "-b" | "--subject" | "-t")
            || t.starts_with("--body=")
            || t.starts_with("--subject=")
        {
            break;
        }
        match t {
            "-R" | "--repo" | "-F" | "--body-file" => skip = true,
            _ if t.starts_with("--repo=") || t.starts_with("--body-file=") => {}
            _ if t.starts_with('-') => {}
            _ if !t.is_empty() && t.chars().all(|c| c.is_ascii_digit()) => {
                return Some(t.to_string());
            }
            _ => {}
        }
    }
    None
}

/// jq `@uri` と同じ集合（A-Za-z0-9 -_.~）以外をパーセントエンコードする
fn uri_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// gh の `--jq` 出力（JSON 値の並び）を配列として読む
fn json_stream(s: &str) -> Option<Vec<Value>> {
    serde_json::Deserializer::from_str(s)
        .into_iter::<Value>()
        .collect::<Result<Vec<_>, _>>()
        .ok()
}

fn is_ok_conclusion(c: &Value) -> bool {
    matches!(c.as_str(), Some("success" | "neutral" | "skipped"))
}

impl Rule for PreMergeCheck {
    fn name(&self) -> &'static str {
        "pre-merge-check"
    }

    fn check(&self, input: &Input, shell: &Shell) -> Vec<Finding> {
        for cmd in shell.commands() {
            let Some(args) = cmd.gh_subcommand(&["pr", "merge"]) else {
                continue;
            };
            let mut reasons: Vec<String> = Vec::new();

            let mut dir = shell.target_dir(&cmd, &input.cwd);
            if !dir.is_dir() {
                reasons.push(format!(
                    "コマンド中の cd 先に移動できません: {}",
                    dir.display()
                ));
                dir = input.cwd.clone();
            }

            let repo = opt_value(args, &["-R", "--repo"]).map(|a| {
                a.text
                    .strip_prefix("--repo=")
                    .or_else(|| a.text.strip_prefix("-R="))
                    .unwrap_or(&a.text)
                    .to_string()
            });
            let mut repo_args: Vec<&str> = Vec::new();
            if let Some(r) = repo.as_deref() {
                repo_args.extend(["-R", r]);
            }
            let pr_ref = pr_ref(args);

            // --- 1. マージ方式 ---
            if has_flag(args, &["--squash", "-s", "--rebase", "-r"]) {
                reasons.push(
                    "--squash / --rebase は禁止です。--merge（マージコミット方式）を使ってください"
                        .to_string(),
                );
            } else if !has_flag(args, &["--merge", "-m"]) {
                reasons.push(
                    "--merge を明示してください（マージ方式の既定値に依存しない）".to_string(),
                );
            }

            // --- 2. ブランチ削除 ---
            if !has_flag(args, &["--delete-branch", "-d"]) {
                reasons.push(
                    "--delete-branch を付けてください（マージ後のブランチは速やかに削除する）"
                        .to_string(),
                );
            }

            // --- 3. 本文形式 ---
            match body_text(args, &["--body", "-b"], &["--body-file", "-F"]) {
                Err(path) => reasons.push(format!("--body-file のファイルが読めません: {path}")),
                Ok(None) => reasons.push(
                    "--body でマージコミット本文を指定してください（## Why / ## What / ## Impact）"
                        .to_string(),
                ),
                Ok(Some(body)) => {
                    let missing = missing_headings(&body, &["## Why", "## What", "## Impact"]);
                    if !missing.is_empty() {
                        reasons.push(format!(
                            "マージコミット本文に見出しがありません: {}",
                            missing.join(" ")
                        ));
                    }
                }
            }

            // 対象リポジトリ（base）。check-run と review thread はどちらも base 側に紐づく
            let (owner, name) = match repo.as_deref() {
                Some(r) => {
                    let (o, n) = r.split_once('/').unwrap_or((r, ""));
                    (o.to_string(), n.trim_end_matches(".git").to_string())
                }
                None => (
                    gh::gh(
                        &dir,
                        &["repo", "view", "--json", "owner", "--jq", ".owner.login"],
                    )
                    .unwrap_or_default(),
                    gh::gh(&dir, &["repo", "view", "--json", "name", "--jq", ".name"])
                        .unwrap_or_default(),
                ),
            };

            // --- 4. CI チェック ---
            let mut view_args: Vec<&str> = vec!["pr", "view"];
            view_args.extend(repo_args.iter().copied());
            if let Some(r) = pr_ref.as_deref() {
                view_args.push(r);
            }
            view_args.extend(["--json", "number,headRefOid,reviewDecision,baseRefName"]);
            let pr_meta: Option<Value> = gh::gh(&dir, &view_args)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok());

            let mut head_sha = String::new();
            if pr_meta.is_none() || owner.is_empty() || name.is_empty() {
                reasons.push("PR 情報を取得できません（gh pr view が失敗。PR番号・認証・ネットワークを確認）".to_string());
            } else {
                let meta = pr_meta.as_ref().unwrap();
                head_sha = meta["headRefOid"].as_str().unwrap_or("").to_string();
                let check_runs = gh::gh(
                    &dir,
                    &[
                        "api",
                        "--paginate",
                        &format!("repos/{owner}/{name}/commits/{head_sha}/check-runs?per_page=100"),
                        "--jq",
                        ".check_runs[] | {name, status, conclusion}",
                    ],
                )
                .ok()
                .and_then(|s| json_stream(&s));
                let statuses = gh::gh(
                    &dir,
                    &[
                        "api",
                        &format!("repos/{owner}/{name}/commits/{head_sha}/status"),
                        "--jq",
                        "[.statuses[] | {name: .context, status: (if .state == \"pending\" then \"in_progress\" else \"completed\" end), conclusion: .state}]",
                    ],
                )
                .ok()
                .and_then(|s| serde_json::from_str::<Value>(&s).ok())
                .and_then(|v| v.as_array().cloned());
                match (check_runs, statuses) {
                    (Some(runs), Some(st)) => {
                        let all: Vec<Value> = runs.into_iter().chain(st).collect();
                        let pending: Vec<&Value> =
                            all.iter().filter(|c| c["status"] != "completed").collect();
                        if !pending.is_empty() {
                            let mut names: Vec<String> = pending
                                .iter()
                                .map(|c| c["name"].as_str().unwrap_or("").to_string())
                                .collect();
                            names.sort();
                            names.dedup();
                            reasons.push(format!(
                                "実行中/待機中のチェックがあります ({}件): {}",
                                pending.len(),
                                names.join(", ")
                            ));
                        }
                        let failed: Vec<&Value> = all
                            .iter()
                            .filter(|c| {
                                c["status"] == "completed" && !is_ok_conclusion(&c["conclusion"])
                            })
                            .collect();
                        if !failed.is_empty() {
                            let mut names: Vec<String> = failed
                                .iter()
                                .map(|c| {
                                    format!(
                                        "{} ({})",
                                        c["name"].as_str().unwrap_or(""),
                                        c["conclusion"].as_str().unwrap_or("null")
                                    )
                                })
                                .collect();
                            names.sort();
                            names.dedup();
                            reasons.push(format!(
                                "失敗したチェックがあります ({}件): {}",
                                failed.len(),
                                names.join(", ")
                            ));
                        }
                    }
                    _ => reasons.push(
                        "CI チェック状態を取得できません（check-runs / status API が失敗）"
                            .to_string(),
                    ),
                }
            }

            // --- 5. レビュー判定 ---
            let decision = pr_meta
                .as_ref()
                .and_then(|m| m["reviewDecision"].as_str())
                .unwrap_or("")
                .to_string();
            if decision == "CHANGES_REQUESTED" {
                reasons.push("レビューで変更が要求されています (CHANGES_REQUESTED)".to_string());
            }
            if decision == "REVIEW_REQUIRED" {
                reasons.push("必須レビューが未完了です (REVIEW_REQUIRED)".to_string());
            }

            // --- 6. 未解決レビュースレッド ---
            // Copilot のレビューは COMMENTED で提出され reviewDecision を変えないため、
            // 指摘への対応漏れはスレッドの resolve 状態で判定する
            let pr_number = pr_ref.clone().or_else(|| {
                pr_meta
                    .as_ref()
                    .and_then(|m| m["number"].as_u64())
                    .map(|n| n.to_string())
            });
            match pr_number {
                None => reasons.push("対象 PR を特定できません（PR番号を指定するか、PR のあるブランチで実行してください）".to_string()),
                Some(number) => {
                    let unresolved = gh::gh(
                        &dir,
                        &[
                            "api",
                            "graphql",
                            "--paginate",
                            "-F",
                            &format!("owner={owner}"),
                            "-F",
                            &format!("name={name}"),
                            "-F",
                            &format!("number={number}"),
                            "-f",
                            "query=\n    query($owner: String!, $name: String!, $number: Int!, $endCursor: String) {\n      repository(owner: $owner, name: $name) {\n        pullRequest(number: $number) {\n          reviewThreads(first: 100, after: $endCursor) {\n            pageInfo { hasNextPage endCursor }\n            nodes { isResolved path }\n          }\n        }\n      }\n    }",
                            "--jq",
                            "[.data.repository.pullRequest.reviewThreads.nodes[] | select(.isResolved | not) | .path]",
                        ],
                    )
                    .ok()
                    .and_then(|s| json_stream(&s))
                    .and_then(|pages| {
                        // ページごとの配列を 1 つにまとめる。配列以外が混じれば失敗扱い
                        let mut paths: Vec<String> = Vec::new();
                        for p in pages {
                            let arr = p.as_array()?;
                            paths.extend(arr.iter().filter_map(|v| v.as_str().map(str::to_string)));
                        }
                        Some(paths)
                    });
                    match unresolved {
                        None => reasons.push("未解決レビュースレッドを確認できませんでした（gh api graphql が失敗）".to_string()),
                        Some(paths) if !paths.is_empty() => {
                            let mut uniq = paths.clone();
                            uniq.sort();
                            uniq.dedup();
                            reasons.push(format!(
                                "未解決のレビュースレッドがあります ({}件): {}。対応して返信し、resolve-thread.sh で resolve してください",
                                paths.len(),
                                uniq.join(", ")
                            ));
                        }
                        Some(_) => {}
                    }
                }
            }

            // --- 7. base からの遅れ ---
            if let Some(meta) = pr_meta
                .as_ref()
                .filter(|_| !owner.is_empty() && !name.is_empty())
            {
                let base_ref = meta["baseRefName"].as_str().unwrap_or("").to_string();
                let behind = if base_ref.is_empty() || head_sha.is_empty() {
                    None
                } else {
                    gh::gh(
                        &dir,
                        &[
                            "api",
                            &format!(
                                "repos/{owner}/{name}/compare/{}...{head_sha}",
                                uri_encode(&base_ref)
                            ),
                            "--jq",
                            ".behind_by",
                        ],
                    )
                    .ok()
                    .and_then(|s| s.trim().parse::<u64>().ok())
                };
                match behind {
                    None => reasons.push("base ブランチとの差を確認できません（compare API が失敗）".to_string()),
                    Some(n) if n > 0 => reasons.push(format!(
                        "head が {base_ref} より {n} コミット遅れています。origin/{base_ref} にリベースして --force-with-lease で push し直してからマージしてください"
                    )),
                    Some(_) => {}
                }
            }

            if !reasons.is_empty() {
                return vec![Finding::Deny(format!(
                    "{}\n\n/gh-actions-check で状況を診断してください。",
                    reasons.join("\n")
                ))];
            }
        }
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::Shell;

    #[test]
    fn pr_ref_skips_option_values_and_stops_at_body() {
        let cs = Shell::parse("gh pr merge -R owner/repo 12 --merge --body \"PR 99\"").commands();
        assert_eq!(
            pr_ref(cs[0].gh_subcommand(&["pr", "merge"]).unwrap()).as_deref(),
            Some("12")
        );
        let cs = Shell::parse("gh pr merge --merge --subject 'x 7'").commands();
        assert_eq!(pr_ref(cs[0].gh_subcommand(&["pr", "merge"]).unwrap()), None);
    }

    #[test]
    fn uri_encode_matches_jq_uri() {
        assert_eq!(uri_encode("release/x"), "release%2Fx");
        assert_eq!(uri_encode("main"), "main");
    }

    #[test]
    fn json_stream_reads_concatenated_values() {
        let v = json_stream("{\"a\":1}\n{\"a\":2}\n").unwrap();
        assert_eq!(v.len(), 2);
        assert!(json_stream("not json").is_none());
    }
}
