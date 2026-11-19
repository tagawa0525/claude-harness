//! require-pipefail: 成否を判断するコマンドを、pipefail なしで tail / head につなぐ形に pipefail を付けて通す
//!
//! パイプの終了コードは最後のステージ（tail / head）のものになり、`nix build | tail` のビルドの失敗を見落とす。
//! 対象は `nix build`、`nix flake check`、`cargo`、`run-checks.sh` を、後ろのステージに tail か head がある
//! パイプの途中に置いた形。コマンドに `set -o pipefail` / `bash -o pipefail` のシェルのオプションがあれば何もしない。
//! pipefail が実際に効く位置かは読まない（ADR-0042: 普通に書く形だけを見る）。対象の形は止めずに、command の前に
//! `set -o pipefail; ` を付けた入力で実行させ、付けたことを Claude に知らせる（ADR-0102。許可の判断は返さないので、
//! 許可の確認は書き換えた後のコマンドに対して残る。ほかのルールが止めるときは止める判定が勝つ）

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
        vec![Finding::Prepend {
            prefix: "set -o pipefail; ".to_string(),
            context: "[require-pipefail] ビルドやチェックの出力を tail / head につなぐ形なので、hook がコマンドの前に `set -o pipefail; ` を付けました（打ち直しは要りません）。終了コードは、パイプの途中のコマンドの失敗を映します。head などが先に読むのをやめると、前のコマンドが SIGPIPE で止まり、終了コードが 141 になることがあります".to_string(),
        }]
    }
}
