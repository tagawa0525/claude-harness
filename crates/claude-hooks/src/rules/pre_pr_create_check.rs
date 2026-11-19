//! pre-pr-create-check: gh pr create の前提条件を検証する
//!
//! gh-pr-create スキルの手順のうち、コマンド文字列と git の状態だけで決定的に判定できるものを
//! ゲートにする（スキルを経由しない gh pr create にも効く）:
//! 1. --title がある（--fill は使わない）。fork でなければ 70 文字以内
//! 2. --body / --body-file がある。fork でなければ本文に ## Summary / ## Changes / ## Tests が
//!    揃っている（見出しは常に英語。本文の言語は問わない。--body を省くと gh がエディタを
//!    開き、Claude Code セッションでは固まる）
//! 3. 現在のブランチに上流があり、未プッシュのコミットがない（--head 指定時は見ない）。
//!    同じコマンドで gh pr create より前に、同じ場所で git push を連ねていれば、その push が満たすので見ない
//!    （`git push -u origin HEAD && gh pr create …`。ADR-0042 の普通に書く形。push の引数や `;` `||` は読まない）
//! 4. --web を使わない（ブラウザを開かず URL を報告する）
//! 5. ブランチが新しい ADR を 2 件以上加えていない（1 ブランチ 1 トピック。判定はマージの時点の
//!    pre-git-merge-check と共有する。レビューの周を回した後にマージで止まると、その周が無駄になる）。
//!    base は --base（`origin/<base>`）、無ければ gh と同じく head のブランチの gh-merge-base の設定、
//!    それも無ければ既定ブランチ。head は --head（`origin/<head>`）、
//!    無ければ HEAD。`owner:branch` の head と、-R / --repo / GH_REPO で origin と別のリポジトリに出す
//!    PR は、手元の origin のブランチと比べても数が合わないので数えない。
//!    base が手元に無いなど数えられなければ通す（ここは早めの知らせで、ゲートはマージの時点の
//!    pre-merge-check が GitHub の変更ファイルから数える。止めると docs/adr の無いリポジトリでも
//!    既定ブランチが見つからないだけで PR を作れなくなる。ADR-0056）
//!
//! 検査に通った gh pr create には、PR を作った後の次のステップ（レビューの待ち方、診断、対応、マージ）を
//! additionalContext で添える（呼び出しの結果と同じ時に届く。PR の URL は gh の出力にしかないので、置き場の印で示す）。
//! fork の PR には添えない。
//!
//! 本文の見出しは `--body` 引数ノードの原文（ヒアドキュメント本文を含む）だけを見る。
//! 別のヒアドキュメントに書かれた見出しの例でゲートを通すことはできない。
//! 1〜4 のエスケープは設けない（いずれも満たしてから実行し直せばよい）。5 は、1 つの決定を複数の
//! ADR に分けたときと、決定前の案（status: proposed）の ADR をまとめて加えるときだけ
//! ALLOW_MULTI_TOPIC=<理由> で外す
//!
//! `upstream` リモートのある clone（他人のプロジェクトの fork。上流に PR を出す worktree）では、
//! 件名の長さ、本文の見出し、新しい ADR の数はこちらの規約なので検査しない。上流の PR template と
//! CONTRIBUTING が正で、それに合わせた本文（Serena の Checklist 等）を通す。
//! --title と本文の存在、--web、未プッシュの検査は fork でも要る（gh が固まる・失敗する条件）。
//! git の pre-commit hook（nix/git-hooks.nix）と commit-msg hook（crate::git_hook）と同じ判定

use std::path::Path;

use super::pre_git_merge_check::{added_adrs, multi_topic_reason, read_at};
use super::{Rule, apply_escape_to_reasons};
use crate::git;
use crate::git_args::git_flag;
use crate::input::Input;
use crate::output::Finding;
use crate::shell::{Arg, Escape, Shell, asks_help, has_flag, opt_value};
use crate::unwrap::env_value;

pub struct PrePrCreateCheck;

/// 検査に通った gh pr create に添える、PR を作った後の次のステップ
const NEXT_STEPS: &str = "PR を作ったら、gh pr create が出力した URL を報告し（取り直すときは gh pr view --json url -q .url）、次のステップを示す:
- Copilot のレビューを待つ場合（任意。head のローカルレビューの記録があればマージできる） → ~/.claude/scripts/gh-wait-review.sh <PR の URL>（約 10 分。run_in_background=true で実行）
- Copilot のレビューか CI の状況を確かめる場合 → /gh-actions-check
- レビューコメントに対応する場合 → /gh-pr-review
- レビューの後にマージする場合 → /gh-pr-merge
- 状態を確かめる場合 → /git-info
/delegate の担当は、待ち方とマージをこの案内でなく、/delegate の受ける側の手順（~/.claude/skills/delegate/references/working.md の「作業中の規則」の待ちの項と、手順 10「許可の後にマージする」）に従う";

/// `--x=value` 形式なら value を、そうでなければそのまま
pub(super) fn strip_opt_prefix<'a>(arg: &'a Arg, names: &[&str]) -> &'a str {
    for n in names {
        if let Some(v) = arg.text.strip_prefix(&format!("{n}=")) {
            return v;
        }
    }
    &arg.text
}

/// `~` と Git Bash のドライブのパスを解いてファイルを読む。相対のパスは hook の cwd から読む
pub(super) fn read_body_file(path: &str) -> Option<String> {
    let home = crate::shell::home();
    // ホームが分からなければ、`~/x` を hook の cwd の `x` と読まずに、読めないものとして扱う
    if (path == "~" || path.starts_with("~/")) && home.as_os_str().is_empty() {
        return None;
    }
    let path = crate::shell::resolve(&home, Path::new(""), path);
    std::fs::read_to_string(path).ok()
}

/// 本文に見出しが揃っているか。欠けている見出しを返す
pub(super) fn missing_headings<'a>(body: &str, headings: &[&'a str]) -> Vec<&'a str> {
    headings
        .iter()
        .copied()
        .filter(|h| !body.contains(h))
        .collect()
}

/// `--body` 相当の本文テキスト。--body-file があればその内容（読めなければ Err にパス）
pub(super) fn body_text(
    args: &[Arg],
    body_flags: &[&str],
    file_flags: &[&str],
) -> Result<Option<String>, String> {
    if let Some(f) = opt_value(args, file_flags) {
        let path = strip_opt_prefix(f, file_flags).to_string();
        return match read_body_file(&path) {
            Some(text) => Ok(Some(text)),
            None => Err(path),
        };
    }
    Ok(opt_value(args, body_flags).map(|a| a.raw.clone()))
}

/// `--body-file` が読めないときの拒否の理由。hook は展開の前の字面のパスを読むので、修正方法まで示す
pub(super) fn unreadable_body_file(path: &str) -> String {
    format!(
        "--body-file のファイルが読めません: {path}\nhook はコマンドを実行する前に、変数を展開せずにパスを読みます。同じコマンドの中で書くファイルと、$S/… のような変数のパスは読めません。本文は別の呼び出しで先にファイルに書き、--body-file には絶対パスを書いてください"
    )
}

/// リポジトリの指定（`OWNER/REPO`、`HOST/OWNER/REPO`、URL、git のリモートの URL）を `owner/repo` にそろえる
fn repo_slug(spec: &str) -> Option<String> {
    let s = spec.trim().trim_end_matches('/');
    let s = s.strip_suffix(".git").unwrap_or(s);
    let mut parts = s.rsplit(['/', ':']);
    let repo = parts.next().filter(|p| !p.is_empty())?;
    let owner = parts.next().filter(|p| !p.is_empty())?;
    Some(format!("{owner}/{repo}").to_lowercase())
}

/// PR が加える ADR を数える範囲（`<base>...<head>`）。数えないとき、base が分からないときは None。
/// `target_repo` は -R / --repo / GH_REPO の指定
fn adr_range(dir: &Path, args: &[Arg], target_repo: Option<&str>) -> Option<String> {
    // origin と別のリポジトリへの PR は、手元の origin のブランチと比べても数が合わない
    if let Some(target) = target_repo {
        let target = repo_slug(target)?;
        let origin = git::git(dir, &["remote", "get-url", "origin"])?;
        if Some(target) != repo_slug(&origin) {
            return None;
        }
    }
    let head_branch =
        opt_value(args, &["--head", "-H"]).map(|h| strip_opt_prefix(h, &["--head", "-H"]));
    if head_branch.is_some_and(|h| h.contains(':')) {
        return None;
    }
    let head = head_branch.map_or_else(|| "HEAD".to_string(), |h| format!("origin/{h}"));
    // --base が無ければ、gh と同じく head のブランチの gh-merge-base、無ければ既定ブランチ
    let merge_base = || {
        let branch = head_branch.map_or_else(|| git::current_branch(dir), str::to_string);
        git::git(
            dir,
            &["config", "--get", &format!("branch.{branch}.gh-merge-base")],
        )
        .filter(|_| !branch.is_empty())
        .map(|b| format!("origin/{b}"))
    };
    let base = match opt_value(args, &["--base", "-B"]) {
        Some(b) => format!("origin/{}", strip_opt_prefix(b, &["--base", "-B"])),
        None => merge_base().or_else(|| git::default_branch(dir))?,
    };
    Some(format!("{base}...{head}"))
}

impl Rule for PrePrCreateCheck {
    fn name(&self) -> &'static str {
        "pre-pr-create-check"
    }

    fn check(&self, input: &Input, shell: &Shell) -> Vec<Finding> {
        // fork から上流に出す PR は、レビューとマージが上流の側の手順なので、案内を添えない
        let mut creates = false;
        for cmd in shell.commands() {
            let Some(args) = cmd.gh_subcommand(&["pr", "create"]) else {
                continue;
            };
            if asks_help(args) {
                continue;
            }
            let mut reasons: Vec<String> = Vec::new();

            // gh / git の実行コンテキストをコマンドに合わせる（`cd <path> && gh pr create`）
            let mut dir = shell.target_dir(cmd, &input.cwd);
            let in_target = dir.is_dir();
            if !in_target {
                // 手元の cwd に戻して続ける検査は、接続先や対象の利用者の PR に当たらないので混ぜない
                if shell.runs_under_ssh_or_login(cmd) {
                    return vec![Finding::Deny(
                        "ssh や sudo -i の中で走る gh は、手元の場所で検査できないため止めます"
                            .to_string(),
                    )];
                }
                reasons.push(format!(
                    "コマンド中の cd 先に移動できません: {}",
                    dir.display()
                ));
                dir = input.cwd.clone();
            }

            // fork ではこちらの規約（件名の長さ、本文の見出し、新しい ADR の数）を検査しない
            let fork = git::is_fork(&dir);
            creates |= !fork;

            // --- 4. --web ---
            if has_flag(args, &["--web", "-w"]) {
                reasons.push("--web は使わないでください（ブラウザを開かず、gh pr create が出力した URL を報告する）".to_string());
            }

            // --- 1. タイトル ---
            match opt_value(args, &["--title", "-t"]) {
                Some(t) => {
                    let title = strip_opt_prefix(t, &["--title", "-t"]);
                    let n = title.chars().count();
                    if !fork && n > 70 {
                        reasons.push(format!("--title は 70 文字以内にしてください（現在 {n} 文字）"));
                    }
                }
                None => reasons.push("--title で PR タイトルを指定してください（--fill でコミットメッセージを流用しない）".to_string()),
            }

            // --- 2. 本文 ---
            match body_text(args, &["--body", "-b"], &["--body-file", "-F"]) {
                Err(path) => reasons.push(unreadable_body_file(&path)),
                Ok(None) if fork => reasons.push("--body または --body-file で PR 本文を指定してください（上流の PR template に合わせる。省くと gh がエディタを開いて固まる）".to_string()),
                Ok(None) => reasons.push("--body または --body-file で PR 本文を指定してください（## Summary / ## Changes / ## Tests）".to_string()),
                Ok(Some(body)) if !fork => {
                    let missing = missing_headings(&body, &["## Summary", "## Changes", "## Tests"]);
                    if !missing.is_empty() {
                        reasons.push(format!("PR 本文に見出しがありません: {}", missing.join(" ")));
                    }
                }
                Ok(Some(_)) => {}
            }

            // --- 3. 未プッシュコミット ---
            let pushed_before = shell.commands_before(cmd).any(|c| {
                matches!(c.git_subcommand(), Some(("push", a))
                    if !git_flag("push", a, Some('n'), &["--dry-run", "--delete", "--tags"]))
                    && shell.target_dir(c, &input.cwd) == dir
            });
            if !has_flag(args, &["--head", "-H"]) && !pushed_before {
                match git::git(&dir, &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"]) {
                    None => reasons.push("現在のブランチに上流ブランチがありません。先に git push してください（gh が対話的に push 先を尋ねて固まるのを防ぐ）".to_string()),
                    Some(upstream) => {
                        let ahead = git::git(&dir, &["rev-list", "--count", "@{u}..HEAD"])
                            .unwrap_or_else(|| "?".to_string());
                        if ahead != "0" {
                            reasons.push(format!(
                                "未プッシュのコミットが {ahead} 件あります（{upstream} より先）。先に git push してください"
                            ));
                        }
                    }
                }
            }

            // --- 5. トピックが 1 つか（新しい ADR の数）---
            // cwd に戻したときは、cwd のブランチの ADR が目的の PR に当たらないので数えない
            let escape = shell.escape("ALLOW_MULTI_TOPIC");
            if in_target && !fork && escape != Escape::Given {
                // GH_REPO は gh に渡る値（前置きの代入、env -u を当てた環境）だけを見る
                let inherited = std::env::var("GH_REPO").ok();
                let target_repo = opt_value(args, &["-R", "--repo"])
                    .map(|a| strip_opt_prefix(a, &["-R", "--repo"]))
                    .or_else(|| env_value(&cmd.env, "GH_REPO", inherited.as_deref()))
                    .filter(|r| !r.is_empty());
                let topic: Vec<String> = adr_range(&dir, args, target_repo)
                    .and_then(|range| {
                        let adrs = added_adrs(&dir, &range)?;
                        let head = range.rsplit_once("...").map_or("HEAD", |(_, h)| h);
                        multi_topic_reason(&adrs, read_at(&dir, head))
                    })
                    .into_iter()
                    .collect();
                reasons.extend(apply_escape_to_reasons(escape, "ALLOW_MULTI_TOPIC", topic));
            }

            if !reasons.is_empty() {
                return vec![Finding::Deny(reasons.join("\n"))];
            }
        }
        if creates {
            return vec![Finding::Context(NEXT_STEPS.to_string())];
        }
        Vec::new()
    }
}

#[cfg(all(test, windows))]
mod tests {
    #[test]
    fn body_file_is_read_by_a_git_bash_drive_path() {
        let path =
            std::env::temp_dir().join(format!("claude-hooks-body-{}.md", std::process::id()));
        std::fs::write(&path, "## Summary\n").unwrap();
        let body = super::read_body_file(&crate::shell::to_msys(&path));
        let _ = std::fs::remove_file(&path);
        assert_eq!(body.as_deref(), Some("## Summary\n"));
    }
}
