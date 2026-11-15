//! require-pipefail: 成否を判断するコマンドを、pipefail なしで tail / head につなぐ形を止める
//!
//! パイプの終了コードは最後の段（tail / head）のものになり、`nix build | tail` のビルドの失敗を見落とす。
//! 対象は `nix build`、`nix flake check`、`cargo`、`run-checks.sh` を、後ろの段に tail か head がある
//! パイプの途中に置いた形。コマンドに `set -o pipefail` / `bash -o pipefail` のシェルのオプションがあれば通す。
//! pipefail が実際に効く位置かは読まない（ADR-0042: 普通に書く形だけを止める）。エスケープは設けない
//! （出力を絞りたいなら、ファイルに書いて tail で読むか、pipefail を付ける）

use super::Rule;
use crate::input::Input;
use crate::output::Finding;
use crate::shell::{Cmd, Shell};
use crate::unwrap::basename;

pub struct RequirePipefail;

/// `set -o pipefail`、`set -eo pipefail`、`bash -o pipefail` のように、シェルのオプションとして書かれているか。
/// 語がシェルのオプションの位置にあるかだけを見る。コメント（`# set -o pipefail`）や `echo "set -o pipefail"` の
/// 文字列の中の同じ形は、シェルのオプションと区別せず通す（ADR-0042: 普通に書く形だけを止める）
fn sets_pipefail(command: &str) -> bool {
    static CELL: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    CELL.get_or_init(|| {
        regex::Regex::new(
            r#"(^|[\s;&|(){`'"/])(set|bash|sh|zsh)\s+(-[A-Za-z]*o\s+\S+\s+|-\S+\s+)*-[A-Za-z]*o\s+pipefail\b"#,
        )
        .expect("re")
    })
    .is_match(command)
}

fn judges_success(cmd: &Cmd) -> bool {
    let first = cmd.args.first().map(|a| a.text.as_str());
    let second = cmd.args.get(1).map(|a| a.text.as_str());
    match basename(&cmd.name) {
        "cargo" | "run-checks.sh" => true,
        "nix" => first == Some("build") || (first == Some("flake") && second == Some("check")),
        _ => false,
    }
}

impl Rule for RequirePipefail {
    fn name(&self) -> &'static str {
        "require-pipefail"
    }

    fn check(&self, input: &Input, shell: &Shell) -> Vec<Finding> {
        if sets_pipefail(&input.command) {
            return Vec::new();
        }
        let piped = shell.commands().iter().any(|cmd| {
            judges_success(cmd)
                && cmd
                    .piped_into
                    .iter()
                    .any(|n| matches!(basename(n), "tail" | "head"))
        });
        if !piped {
            return Vec::new();
        }
        vec![Finding::Deny(
            "ビルドやチェックの出力を tail / head につなぐと、パイプの終了コードが tail / head のものになり、失敗を見落とします。`set -o pipefail;` を前に付けるか、出力をファイルに書いて tail で読んでください".to_string(),
        )]
    }
}
