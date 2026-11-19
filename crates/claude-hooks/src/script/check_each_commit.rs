//! `check-each-commit.sh <base> -- <コマンド…>`: `<base>..HEAD` の各コミットを一時の worktree にチェックアウトし、検査の
//! コマンドを実行する（ADR-0101。`~/.claude/scripts/check-each-commit.sh` が呼ぶ）。
//!
//! 引数、出力の行、終了コードの意味は `claude/scripts/check-each-commit.sh` の先頭に書く。ここには、その形を作る流れを置く。
//!
//! 中断（INT、TERM、HUP）は、走っている子のプロセス（git と検査のコマンド）が終わるのを待ってから後片付けをして 130 で
//! 終える。sh の版の trap と同じく、子には中断を送らない（端末の Ctrl-C は、同じプロセスグループの子にも届く）。待つのは、
//! 子が一時の worktree の中で走っている間に worktree を消さないため。子を実行する間は [`Cleanup`] の Mutex を持ち、中断を
//! 受けたスレッドはそれを取ってから片付ける。

use std::ffi::OsStr;
use std::path::PathBuf;

/// 調べる範囲と検査のコマンド（引数を確かめた後）
#[derive(Debug, PartialEq)]
struct Request<'a> {
    base: &'a str,
    command: &'a [String],
}

/// `<base> -- <コマンド…>` を読む。形が違えば None
fn parse_args(_args: &[String]) -> Option<Request<'_>> {
    todo!("RED")
}

/// 一時のディレクトリを作る親
fn temp_root(_tmpdir: Option<&OsStr>) -> PathBuf {
    todo!("RED")
}

/// 検査のコマンドを起動できなかったときの終了コード
fn spawn_failure_code(_kind: std::io::ErrorKind) -> i32 {
    todo!("RED")
}

pub fn main(_args: &[String]) -> i32 {
    let _ = (parse_args, temp_root, spawn_failure_code);
    todo!("RED")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    #[ignore = "RED: 実装の前"]
    fn the_base_the_separator_and_a_command_are_read() {
        let args = strings(&["main", "--", "cargo", "build"]);
        let req = parse_args(&args).unwrap();
        assert_eq!(req.base, "main");
        assert_eq!(req.command, &strings(&["cargo", "build"])[..]);
    }

    #[test]
    #[ignore = "RED: 実装の前"]
    fn arguments_without_the_separator_or_a_command_are_a_usage_error() {
        assert_eq!(parse_args(&strings(&["main", "true"])), None);
        assert_eq!(parse_args(&strings(&["main", "x", "true"])), None);
        assert_eq!(parse_args(&strings(&["main", "--"])), None);
        assert_eq!(parse_args(&strings(&["main"])), None);
        assert_eq!(parse_args(&[]), None);
    }

    #[test]
    #[ignore = "RED: 実装の前"]
    fn a_second_separator_is_passed_to_the_command() {
        let args = strings(&["main", "--", "--", "x"]);
        assert_eq!(
            parse_args(&args).unwrap().command,
            &strings(&["--", "x"])[..]
        );
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "RED: 実装の前"]
    fn an_empty_or_missing_tmpdir_falls_back_to_tmp_like_sh() {
        assert_eq!(temp_root(None), PathBuf::from("/tmp"));
        assert_eq!(temp_root(Some(OsStr::new(""))), PathBuf::from("/tmp"));
        assert_eq!(
            temp_root(Some(OsStr::new("/var/t"))),
            PathBuf::from("/var/t")
        );
    }

    #[test]
    #[ignore = "RED: 実装の前"]
    fn a_command_that_cannot_be_started_gives_the_codes_of_sh() {
        assert_eq!(spawn_failure_code(std::io::ErrorKind::NotFound), 127);
        assert_eq!(
            spawn_failure_code(std::io::ErrorKind::PermissionDenied),
            126
        );
    }
}
