//! pre-pr-create-check: gh pr create の前提条件を検証する
//!
//! gh-pr-create スキルの手順のうち、コマンド文字列と git の状態だけで決定的に判定できるものを
//! ゲートにする（スキルを経由しない gh pr create にも効く）:
//! 1. --title がある（--fill は使わない）。70 文字以内
//! 2. --body / --body-file がある。本文に ## 概要 / ## 変更点 / ## テスト が揃っている
//!    （--body を省くと gh がエディタを開き、Claude Code セッションでは固まる）
//! 3. 現在のブランチに上流があり、未プッシュのコミットがない（--head 指定時は見ない）
//! 4. --web を使わない（ブラウザを開かず URL を報告する）
//!
//! 本文の見出しは `--body` 引数ノードの原文（ヒアドキュメント本文を含む）だけを見る。
//! 別のヒアドキュメントに書かれた見出しの例でゲートを通すことはできない。
//! エスケープは設けない（いずれも満たしてから実行し直せばよい）

use std::path::Path;

use super::Rule;
use crate::git;
use crate::input::Input;
use crate::output::Finding;
use crate::shell::{Arg, Shell, has_flag, opt_value};

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
            let mut dir = shell.target_dir(&cmd, &input.cwd);
            if !dir.is_dir() {
                reasons.push(format!(
                    "コマンド中の cd 先に移動できません: {}",
                    dir.display()
                ));
                dir = input.cwd.clone();
            }

            // --- 4. --web ---
            if has_flag(args, &["--web", "-w"]) {
                reasons.push("--web は使わないでください（ブラウザを開かず、gh pr create が出力した URL を報告する）".to_string());
            }

            // --- 1. タイトル ---
            match opt_value(args, &["--title", "-t"]) {
                Some(t) => {
                    let title = strip_opt_prefix(t, &["--title", "-t"]);
                    let n = title.chars().count();
                    if n > 70 {
                        reasons.push(format!("--title は 70 文字以内にしてください（現在 {n} 文字）"));
                    }
                }
                None => reasons.push("--title で PR タイトルを指定してください（--fill でコミットメッセージを流用しない）".to_string()),
            }

            // --- 2. 本文 ---
            match body_text(args, &["--body", "-b"], &["--body-file", "-F"]) {
                Err(path) => reasons.push(format!("--body-file のファイルが読めません: {path}")),
                Ok(None) => reasons.push("--body または --body-file で PR 本文を指定してください（## 概要 / ## 変更点 / ## テスト）".to_string()),
                Ok(Some(body)) => {
                    let missing = missing_headings(&body, &["## 概要", "## 変更点", "## テスト"]);
                    if !missing.is_empty() {
                        reasons.push(format!("PR 本文に見出しがありません: {}", missing.join(" ")));
                    }
                }
            }

            // --- 3. 未プッシュコミット ---
            if !has_flag(args, &["--head", "-H"]) {
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

            if !reasons.is_empty() {
                return vec![Finding::Deny(reasons.join("\n"))];
            }
        }
        Vec::new()
    }
}
