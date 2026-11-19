//! require-pipefail: 成否を判断するコマンドを、pipefail なしで tail / head につなぐ形に pipefail を付けて通す
//!
//! パイプの終了コードは最後のステージ（tail / head）のものになり、`nix build | tail` のビルドの失敗を見落とす。
//! 対象は `nix build`、`nix flake check`、`cargo`、`run-checks.sh` を、後ろのステージに tail か head がある
//! パイプの途中に置いた形。コマンドに `set -o pipefail` / `bash -o pipefail` のシェルのオプションがあれば何もしない。
//! pipefail が実際に効く位置かは読まない（ADR-0042: 普通に書く形だけを見る）。対象の形は止めずに、command の前に
//! `set -o pipefail; ` を付けた入力で実行させ、付けたことを Claude に知らせる（ADR-0102。許可の判断は返さないので、
//! 許可の確認は書き換えた後のコマンドに対して残る。ほかのルールが止めるときは止める判定が勝つ）。tail / head への
//! パイプが `bash -c` や `ssh` の文字列の中にあれば、外に付けた pipefail が効かないので、書き換えずに止める

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

fn pipes_to_tail(stages: &[String]) -> bool {
    stages
        .iter()
        .any(|n| matches!(basename(n), "tail" | "head"))
}

/// tail / head へのパイプが、`bash -c` や `ssh` の文字列の中（外の `set -o pipefail` を受け継がない子のシェル）に
/// あるか。shell.rs は文字列の中のコマンドの piped_into を、文字列の中の後ろのステージに、文字列を実行するコマンドの
/// piped_into をつないで作るので、その長さの差が文字列の中のステージになる。`eval` は文字列を今のシェルで実行し、
/// `(…)` と `$(…)` は文字列のスコープでないので、どちらも外のオプションを受け継ぐ側に数える
fn piped_inside_child_shell(cmd: &Cmd, commands: &[Cmd]) -> bool {
    let mut current = cmd;
    loop {
        let spawner = current
            .scope
            .iter()
            .rev()
            .find_map(|id| commands.iter().find(|c| c.spawns == Some(*id)));
        let Some(spawner) = spawner else {
            return false;
        };
        let inner = current
            .piped_into
            .len()
            .saturating_sub(spawner.piped_into.len());
        if basename(&spawner.name) != "eval" && pipes_to_tail(&current.piped_into[..inner]) {
            return true;
        }
        current = spawner;
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
        let commands = shell.commands();
        let piped: Vec<&Cmd> = commands
            .iter()
            .filter(|cmd| judges_success(cmd) && pipes_to_tail(&cmd.piped_into))
            .collect();
        if piped.is_empty() {
            return Vec::new();
        }
        if piped
            .iter()
            .any(|cmd| piped_inside_child_shell(cmd, commands))
        {
            return vec![Finding::Deny(
                "`bash -c` や `ssh` の文字列の中で、ビルドやチェックの出力が tail / head につながっていて、パイプの終了コードが tail / head のものになり、失敗を見落とします。文字列で実行するシェルは外の `set -o pipefail` を受け継がないので、文字列の中の先頭に `set -o pipefail;` を付けるか、出力をファイルに書いて tail で読んでください".to_string(),
            )];
        }
        vec![Finding::Prepend {
            prefix: "set -o pipefail; ".to_string(),
            context: "[require-pipefail] ビルドやチェックの出力を tail / head につなぐ形なので、hook がコマンドの前に `set -o pipefail; ` を付けました（打ち直しは要りません）。終了コードは、パイプの途中のコマンドの失敗を映します。head などが先に読むのをやめると、前のコマンドが SIGPIPE で止まり、終了コードが 141 になることがあります。auto mode が、hook が入力を変えたことを理由にこの呼び出しを止めたら、`set -o pipefail; ` を付けた後のコマンドを打ってください（そのコマンドは書き換えません）".to_string(),
        }]
    }
}
