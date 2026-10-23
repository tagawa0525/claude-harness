//! pre-pr-create-check: gh pr create の前提条件を検証する
//!
//! gh-pr-create スキルの手順のうち、コマンド文字列と git の状態だけで決定的に判定できるものを
//! ゲートにする（スキルを経由しない gh pr create にも効く）:
//! 1. --title がある（--fill は使わない）。fork でなければ 70 文字以内
//! 2. --body / --body-file がある。fork でなければ本文に ## Summary / ## Changes / ## Tests が
//!    揃っている（見出しは常に英語。本文の言語は問わない。--body を省くと gh がエディタを
//!    開き、Claude Code セッションでは固まる）
//! 3. 現在のブランチに上流があり、未プッシュのコミットがない（--head 指定時は見ない）。
//!    同じコマンドの前に連ねた git push は反映されない（実行前の状態を見る）
//! 4. --web を使わない（ブラウザを開かず URL を報告する）
//! 5. ブランチが新しい ADR を 2 件以上加えていない（1 ブランチ 1 トピック。判定はマージの時点の
//!    pre-git-merge-check と共有する。レビューの周を回した後にマージで止まると、その周が無駄になる）。
//!    base は --base（`origin/<base>`）、無ければ既定ブランチ。head は --head（`origin/<head>`）、
//!    無ければ HEAD。`owner:branch` の head は別のリポジトリのブランチで手元に無いので数えない。
//!    数えられなければ止める
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
//! git の pre-commit / commit-msg hook と同じ判定（modules/home/parts/git.nix）

use std::path::Path;

use super::pre_git_merge_check::{added_adrs, multi_topic_reason};
use super::{Rule, apply_escape_to_reasons};
use crate::git;
use crate::input::Input;
use crate::output::Finding;
use crate::shell::{Arg, Escape, Shell, has_flag, opt_value};

/// 検査は実行前のブランチの状態を見る。同じコマンドの前に連ねた git push は反映されず、
/// 連ねるとコマンド全体が止まって前半も実行されないので、分け方を deny の文言で示す。
const SPLIT_CALLS_HINT: &str = "同じコマンドの前に git push を連ねても検査には反映されません。push と gh pr create は別の呼び出しに分けてください";

pub struct PrePrCreateCheck;

/// `--x=value` 形式なら value を、そうでなければそのまま
pub(super) fn strip_opt_prefix<'a>(arg: &'a Arg, names: &[&str]) -> &'a str {
    for n in names {
        if let Some(v) = arg.text.strip_prefix(&format!("{n}=")) {
            return v;
        }
    }
    &arg.text
}

/// `~` を展開してファイルを読む
pub(super) fn read_body_file(path: &str) -> Option<String> {
    let path = if let Some(rest) = path.strip_prefix("~/") {
        std::env::var_os("HOME").map(|h| Path::new(&h).join(rest))
    } else {
        Some(Path::new(path).to_path_buf())
    }?;
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

/// PR が加える ADR を数える範囲（`<base>...<head>`）。数えないときは Ok(None)、base が分からなければ Err
fn adr_range(dir: &Path, args: &[Arg]) -> Result<Option<String>, String> {
    let head = match opt_value(args, &["--head", "-H"]) {
        Some(h) => {
            let h = strip_opt_prefix(h, &["--head", "-H"]);
            if h.contains(':') {
                return Ok(None);
            }
            format!("origin/{h}")
        }
        None => "HEAD".to_string(),
    };
    let base = match opt_value(args, &["--base", "-B"]) {
        Some(b) => format!("origin/{}", strip_opt_prefix(b, &["--base", "-B"])),
        None => git::default_branch(dir).ok_or_else(|| "既定ブランチ".to_string())?,
    };
    Ok(Some(format!("{base}...{head}")))
}

impl Rule for PrePrCreateCheck {
    fn name(&self) -> &'static str {
        "pre-pr-create-check"
    }

    fn check(&self, input: &Input, shell: &Shell) -> Vec<Finding> {
        for cmd in shell.commands() {
            let Some(args) = cmd.gh_subcommand(&["pr", "create"]) else {
                continue;
            };
            let mut reasons: Vec<String> = Vec::new();

            // gh / git の実行コンテキストをコマンドに合わせる（`cd <path> && gh pr create`）
            let mut dir = shell.target_dir(cmd, &input.cwd);
            if !dir.is_dir() {
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

            // fork ではこちらの規約（件名の長さ、本文の見出し）を検査しない
            let fork = git::is_fork(&dir);

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
                Err(path) => reasons.push(format!("--body-file のファイルが読めません: {path}")),
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
            if !has_flag(args, &["--head", "-H"]) {
                match git::git(&dir, &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"]) {
                    None => reasons.push(format!("現在のブランチに上流ブランチがありません。先に git push してください（gh が対話的に push 先を尋ねて固まるのを防ぐ）。{SPLIT_CALLS_HINT}")),
                    Some(upstream) => {
                        let ahead = git::git(&dir, &["rev-list", "--count", "@{u}..HEAD"])
                            .unwrap_or_else(|| "?".to_string());
                        if ahead != "0" {
                            // 件数が取れなかった（"?"）ときは原因が未プッシュとは限らないので、分け方は示さない
                            let hint = if ahead == "?" { String::new() } else { format!("。{SPLIT_CALLS_HINT}") };
                            reasons.push(format!(
                                "未プッシュのコミットが {ahead} 件あります（{upstream} より先）。先に git push してください{hint}"
                            ));
                        }
                    }
                }
            }

            // --- 5. トピックが 1 つか（新しい ADR の数）---
            let escape = shell.escape("ALLOW_MULTI_TOPIC");
            if !fork && escape != Escape::Given {
                let topic = match adr_range(&dir, args) {
                    Ok(None) => Vec::new(),
                    Ok(Some(range)) => match added_adrs(&dir, &range) {
                        Some(adrs) => multi_topic_reason(&adrs).into_iter().collect(),
                        None => vec![format!(
                            "新しい ADR の数を確認できません（git diff {range} が失敗）。base が手元に無ければ git fetch してください"
                        )],
                    },
                    Err(what) => vec![format!(
                        "新しい ADR の数を確認できません（比べる{what}が見つからない）"
                    )],
                };
                reasons.extend(apply_escape_to_reasons(escape, "ALLOW_MULTI_TOPIC", topic));
            }

            if !reasons.is_empty() {
                return vec![Finding::Deny(reasons.join("\n"))];
            }
        }
        Vec::new()
    }
}
