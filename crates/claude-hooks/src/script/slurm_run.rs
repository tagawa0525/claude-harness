//! `slurm-run.sh --class <区分> --ref <参照> --purpose <目的> [--top] [--timed | -c <コア数>] -t <見積もり> <name> <command>`:
//! r995 の Slurm に重い計算を 1 本投げ、終わるまで待つ（ADR-0101。`~/.claude/scripts/slurm-run.sh` が呼ぶ）。
//!
//! 引数、出力の行、終了コードの意味は `claude/scripts/slurm-run.sh` の先頭に書く。ここには、その形を作る判定と流れを置く。
//! スケジューラのコマンドを呼ぶ所と出力を解く所は [`slurm`] にあり、ここの判定はそこが返す型だけを受ける。
//!
//! 流れ:
//!
//! 1. 引数を確かめる（[`parse_args`]）。投げられない引数は、理由を標準エラーに出して 1 で終える
//! 2. 投げる場所を確かめ（r995 か、NFS の入口の下か）、連絡先（ブランチ、場所、`commit=`、`claude=`、`shared-cpus=`）を作る
//! 3. timed のジョブは、走っている main のジョブと同じ worktree なら投げない（[`same_worktree_job`]）
//! 4. 終わるまで待つ形で投げ、最初の行から番号を読んで `JOB:`、`LOG:`、`NEXT:` を出す
//! 5. main は区分の順に並べ直す（[`reorder`]。列は slurm-queue と同じ [`slurm_queue::order`]）。timed は --top のときだけ先頭に出す
//! 6. 投げた子の終わりを待ち、会計の記録（終わった状態）が届くまで待つ（[`wait_record`]）。届かなければ制御のデーモンの状態
//! 7. `TIME:` の行をログに書き足し（[`time_line`]）、ログの末尾 20 行、`NEXT:`、`RESULT:`、`EXIT:` を出す
//!
//! `RESULT:` の行は、投げる前に止めたとき、投げられなかったとき、中断（INT、TERM、HUP）されたときも 1 行だけ出す
//! （[`Progress`]）。中断されたら終了コードは 130。投げた子の出力はパイプで読むので、一時ファイルを作らない。

use super::slurm::{self, JobRecord, JobState, Limit, RunningJob, Submission};
use super::{slurm_class, slurm_queue};
use regex::Regex;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::Command;
use std::sync::{Arc, LazyLock, Mutex};

/// r995 の ~/github の NFS の入口（ほかのホストから投げる場所）。テストは SLURM_RUN_NFS_ENTRY で置き換える
const NFS_ENTRY: &str = "/mnt/r995/github";

/// --ref の番号
static REFERENCE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(#[0-9]+|ADR-[0-9]+)(,(#[0-9]+|ADR-[0-9]+))*$|^- +\S").expect("正規表現は正しい")
});

/// ジョブは r995（NixOS）で走り、/usr/bin/time が無いので、包んだコマンドは 0 秒で失敗する（2026-10-24、ジョブ 538〜540）。
/// 標準エラーを自分のファイルに向けたコマンドではジョブのログにも原因が出ないので、投げる前に止める
static USR_BIN_TIME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(^|[^\p{Alphabetic}\p{Nd}_./-])/usr/bin/time($|[\s;|&)}<>"'])"#)
        .expect("正規表現は正しい")
});

/// ジョブの名前（ログのファイル名に使う）
static NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z0-9._-]+$").expect("正規表現は正しい"));

static CPUS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[1-9][0-9]*$").expect("正規表現は正しい"));

const NEXT_MARKED: &str = "NEXT: TIME 行に印があるので、この時間（elapsed も、プログラムが自分で測った時間も）は記録に使わない。時間を記録する計算なら、-c を付けずに区分 timed で投げ直して測る（時間を記録しない計算は投げ直さない）";

/// 投げるジョブ（引数を確かめた後）
#[derive(Debug, PartialEq)]
struct Request {
    class: String,
    reference: String,
    purpose: String,
    top: bool,
    /// 割り込まれないパーティションに投げる（--timed か、区分 timed）
    timed: bool,
    /// -c のコア数（排他にしない）
    cpus: Option<String>,
    estimate: String,
    name: String,
    command: String,
}

/// 投げずに止めた理由。name は RESULT: の行に書く名前（名前を読む前に止めたら None）、message は標準エラーに出す全文
#[derive(Debug, PartialEq)]
struct Refusal {
    name: Option<String>,
    message: String,
}

/// RESULT: の行に要る、ここまでに分かったこと。中断を受けたスレッドと分け合う
#[derive(Default)]
struct Progress {
    name: Option<String>,
    job: Option<String>,
    reported: bool,
}

impl Progress {
    /// RESULT: の行を、まだ出していなければ出す
    fn report(&mut self, exit: i32, state: &str) {
        todo!("RED")
    }

    /// 終わりの状態が分からないまま終えるときの RESULT: の行（投げる前なら NOT_SUBMITTED、投げた後なら unknown）
    fn report_unfinished(&mut self, exit: i32) {
        todo!("RED")
    }
}

fn lock(progress: &Mutex<Progress>) -> std::sync::MutexGuard<'_, Progress> {
    todo!("RED")
}

pub fn main(args: &[String]) -> i32 {
    todo!("RED")
}

/// 投げて待つ。RESULT: の行は、終わりの状態が分かったときだけここで出す（ほかは呼ぶ側が出す）
fn run(args: &[String], progress: &Mutex<Progress>) -> i32 {
    todo!("RED")
}

/// 終わりを確かめられなかった状態
fn unknown_state() -> JobState {
    todo!("RED")
}

/// 引数を読んで確かめる。確かめる順と理由の文は sh の版と同じ（使い方の誤り、区分、参照、目的、-c、-t、コマンド、名前）
fn parse_args(args: &[String]) -> Result<Request, Refusal> {
    todo!("RED")
}

fn usage() -> String {
    todo!("RED")
}

/// 会計の記録を待つ回数と間隔（SLURM_RUN_SACCT_TRIES、SLURM_RUN_SACCT_INTERVAL。テストが短くする）
fn record_wait() -> Result<(u32, std::time::Duration), String> {
    todo!("RED")
}

/// sh の $PWD と同じ作業ディレクトリ。環境の PWD が今の場所を指していればそれ（symlink をたどる前の書き方）、
/// 指していなければ OS の今の場所。連絡先とジョブの走る場所の案内は、利用者が cd したときの書き方で出す
fn working_dir() -> std::io::Result<String> {
    todo!("RED")
}

fn hostname() -> String {
    todo!("RED")
}

/// path が dir の下か（dir そのものを含む）
fn under(path: &str, dir: &str) -> bool {
    todo!("RED")
}

/// ジョブの作業ディレクトリ（r995 から投げたら ~/github の下、ほかのホストからなら NFS の入口の下）を、このホストの
/// 書き方に直す
fn local_path(path: &str, on_r995: bool, nfs_entry: &str, remote_root: &str) -> String {
    todo!("RED")
}

/// git を起動した結果（起動できなければ ok が false で、理由を stderr に入れる）
struct GitOut {
    ok: bool,
    stdout: String,
    stderr: String,
}

impl GitOut {
    fn first_error_line(&self) -> &str {
        self.stderr.lines().next().unwrap_or("")
    }
}

fn git(args: &[&str]) -> GitOut {
    todo!("RED")
}

fn run_git(cmd: &mut Command) -> GitOut {
    todo!("RED")
}

/// 連絡先（git のブランチ、投げた場所、commit=、claude=）。警告は標準エラーに出す。
///
/// 未コミットの変更はどのコミットにも無い版なので、hash だけでは結果を出した版を名指せない。印（-dirty）を付けて見分ける。
/// まだ追加していない新しいファイル（ソース、入力）もコミットに無いので数える。git が無視するもの（試行の置き場、ビルドの
/// 出力）は --porcelain に出ない。利用者の設定（status.showUntrackedFiles、diff.ignoreSubmodules、submodule.<名前>.ignore）で
/// 数えない変更が出ないよう、--untracked-files=normal と --ignore-submodules=none を明示する。--no-optional-locks は、同じ
/// worktree で走っている git の操作と index のロックを取り合わないため。
///
/// git の中でも、版を調べられなければ（index の破損、safe.directory の拒否など）印は付けずに投げる。印のための処理で
/// ジョブを止めない。黙って付けないと、付かない理由が分からないので、警告に出す。コミットの無いリポジトリは、
/// rev-parse -q が何も出さずに失敗するので警告しない
fn contact(pwd: &str) -> String {
    todo!("RED")
}

/// timed のジョブは走っている main のジョブを止め、止められたジョブは持っているロック（cargo のビルドディレクトリ、
/// DVC の .dvc/tmp/lock）を持ったまま止まる。同じ worktree の timed のジョブは同じロックを待って、どちらも進まない。
/// worktree によらないロック（$CARGO_HOME、nix の評価のキャッシュ）はコマンドから決められないので見ない。
/// 場所はジョブの作業ディレクトリ（投げたときの場所）で読む。コメントの連絡先と違い、slurm-run.sh の外から投げた
/// ジョブにもある。同じ worktree のジョブがあれば、理由を出して終了コード 1 を返す
fn refuse_same_worktree(pwd: &str, local: impl Fn(&str) -> String) -> Option<i32> {
    todo!("RED")
}

/// 作業ディレクトリの worktree の直下（git の外ならそのディレクトリ）
fn worktree_of(dir: &str) -> String {
    todo!("RED")
}

/// running のうち、作業ディレクトリの worktree が here と同じ最初のジョブの番号
fn same_worktree_job<'a>(
    running: &'a [RunningJob],
    here: &str,
    worktree_of: impl Fn(&str) -> String,
) -> Option<&'a str> {
    todo!("RED")
}

/// 並べ直す。read は slurm-queue の --order と同じ列（要らなければ空、読めなければ None）、top は待ち行列の先頭に列を
/// 出す。警告があれば返す。
///
/// scontrol top は、走り始めたジョブや取り消したジョブを含む列を拒まない（終了コード 0。存在しない番号だけ拒む。r995 で
/// 2026-10-30 に確かめた）。同時に投げたほかの slurm-run の列が後から通ると、こちらの並べ直しを上書きする。なので
/// 成功しても待ち行列を読み直し、並べ直しが要らなくなるまで繰り返す（拒まれたときも、読み直せば消えた番号は列から外れる）。
/// 3 回で収まらなければ、待ち行列が動き続けているので投げた順に任せる
fn reorder(mut read: impl FnMut() -> Option<String>, mut top: impl FnMut(&str)) -> Option<String> {
    todo!("RED")
}

/// 終わりが届くまで、会計の記録を tries 回まで読む（間に pause）。終わった記録が無ければ None。
///
/// 制御のデーモンは終わったジョブを MinJobAge（300 秒）の間しか覚えていない。会計の記録が届くまで遅れがあり、その間は
/// 空か走っている記録が返るので、終わりの状態が見えるまで待つ
fn wait_record(
    tries: u32,
    mut read: impl FnMut() -> Option<JobRecord>,
    mut pause: impl FnMut(),
) -> Option<JobRecord> {
    todo!("RED")
}

/// 秒を [日-]時:分:秒 にする（sh の版が sacct の Elapsed から読んでいた書き方）
fn duration(secs: i64) -> String {
    todo!("RED")
}

/// 見積もりを sh の版が sacct の Timelimit から読んでいた書き方にする
fn limit_text(limit: &Option<Limit>) -> String {
    todo!("RED")
}

/// TIME: の行と、印が付いたか、行を作る前に出す警告
#[derive(Debug, PartialEq)]
struct Timing {
    line: String,
    marked: bool,
    warnings: Vec<String>,
}

/// 終わったジョブの TIME: の行。印は、-c のとき、割り込まれないパーティションで走らなかったとき、止められていたとき
/// （確かめられないときも）に付く
fn time_line(job: &str, record: &JobRecord, req: &Request) -> Timing {
    todo!("RED")
}

fn append_line(path: &str, line: &str) -> std::io::Result<()> {
    todo!("RED")
}

/// 終了コード。終わるまで待つ形の投げ方は、取り消したジョブでも 0 を返しうる。成功以外（取り消し、時間切れ、
/// 確かめられないものなど）で終わったジョブは失敗として扱い、&& の次を実行しない
fn final_status(status: i32, state: &JobState) -> i32 {
    todo!("RED")
}

/// & と wait で同時に待った複数のジョブは、EXIT: の行がどのジョブのものか分からないので、1 行で番号と名前を添える
fn result_line(job: Option<&str>, name: Option<&str>, exit: i32, state: &str) -> String {
    todo!("RED")
}

fn exit_line(status: i32, state: &JobState) -> String {
    todo!("RED")
}

/// text の最後の n 行（n は 1 以上。tail -n と同じく、末尾の改行の後は行に数えない）
fn tail_lines(text: &[u8], n: usize) -> &[u8] {
    todo!("RED")
}

#[cfg(test)]
mod tests {
    use super::{
        JobRecord, JobState, Limit, Refusal, Request, RunningJob, duration, exit_line,
        final_status, limit_text, local_path, parse_args, reorder, result_line, same_worktree_job,
        tail_lines, time_line, under, unknown_state, wait_record,
    };

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    /// 区分、参照、目的を付けた引数（目的と区分を確かめるテストのほかは、この組で投げる）
    fn with_purpose(rest: &[&str]) -> Vec<String> {
        let mut all = args(&["--class", "trial", "--ref", "#1", "--purpose", "試験の目的"]);
        all.extend(args(rest));
        all
    }

    fn parse(rest: &[&str]) -> Result<Request, Refusal> {
        parse_args(&with_purpose(rest))
    }

    fn refused(words: &[String]) -> Refusal {
        match parse_args(words) {
            Ok(req) => panic!("{words:?} を受けた: {req:?}"),
            Err(refusal) => refusal,
        }
    }

    fn request(rest: &[&str]) -> Request {
        parse(rest).unwrap_or_else(|r| panic!("{rest:?} を拒んだ: {}", r.message))
    }

    fn state(name: &str, ended: bool, completed: bool) -> JobState {
        JobState {
            name: name.to_string(),
            ended,
            completed,
        }
    }

    fn completed() -> JobState {
        state("COMPLETED", true, true)
    }

    /// 754 秒走り、見積もり 30 分の記録
    fn record(uninterrupted: Option<bool>, suspended: Option<i64>, state: JobState) -> JobRecord {
        JobRecord {
            elapsed: Some(754),
            limit: Some(Limit::Minutes(30)),
            uninterrupted,
            suspended,
            state,
        }
    }

    #[test]
    #[ignore = "RED: 実装の前"]
    fn the_class_reference_purpose_estimate_name_and_command_make_the_request() {
        assert_eq!(
            request(&["-t", "30", "xlc-2G", "cargo run --release"]),
            Request {
                class: "trial".to_string(),
                reference: "#1".to_string(),
                purpose: "試験の目的".to_string(),
                top: false,
                timed: false,
                cpus: None,
                estimate: "30".to_string(),
                name: "xlc-2G".to_string(),
                command: "cargo run --release".to_string(),
            }
        );
        let top = request(&["--top", "--timed", "-t", "5", "x", "true"]);
        assert!(top.top && top.timed, "{top:?}");
    }

    #[test]
    #[ignore = "RED: 実装の前"]
    fn a_missing_class_reference_or_purpose_prints_the_usage_without_a_name() {
        for missing in ["--class", "--ref", "--purpose"] {
            let mut given = Vec::new();
            for (flag, value) in [("--class", "trial"), ("--ref", "#1"), ("--purpose", "p")] {
                if flag != missing {
                    given.extend(args(&[flag, value]));
                }
            }
            given.extend(args(&["-t", "5", "xlc-2G", "true"]));
            let refusal = refused(&given);
            assert_eq!(refusal.name, None);
            assert!(
                refusal
                    .message
                    .starts_with(&format!("ERROR: {missing} が無い（")),
                "{}",
                refusal.message
            );
            assert!(refusal.message.contains("\nUsage: "), "{}", refusal.message);
        }
    }

    #[test]
    #[ignore = "RED: 実装の前"]
    fn a_ref_without_a_value_shows_how_to_quote_a_hash() {
        let refusal = refused(&args(&["--class", "trial", "--ref"]));
        assert!(
            refusal.message.starts_with("ERROR: --ref の値が無い"),
            "{}",
            refusal.message
        );
        assert!(
            refusal.message.contains("--ref '#0200'"),
            "{}",
            refusal.message
        );
        assert!(refusal.message.contains("Usage:"), "{}", refusal.message);
    }

    #[test]
    #[ignore = "RED: 実装の前"]
    fn a_missing_estimate_name_command_or_option_value_prints_the_usage() {
        for rest in [
            &["xlc-2G", "true"][..],
            &["-t", "1", "xlc-2G"],
            &["-t", "1", "xlc-2G", "true", "extra"],
            &["-t", "30", "-c"],
            &["-t"],
        ] {
            let refusal = refused(&with_purpose(rest));
            assert_eq!(refusal.name, None, "{rest:?}");
            assert!(
                refusal
                    .message
                    .starts_with("Usage: slurm-run.sh --class <timed|record|decision|trial>"),
                "{rest:?}: {}",
                refusal.message
            );
        }
        assert!(refused(&args(&["--class"])).message.starts_with("Usage:"));
        assert!(
            refused(&args(&["--class", "trial", "--ref", "#1", "--purpose"]))
                .message
                .starts_with("Usage:")
        );
    }

    #[test]
    #[ignore = "RED: 実装の前"]
    fn an_unknown_option_is_refused_by_its_name_without_the_usage() {
        for opt in ["--urgent", "-x"] {
            let refusal = refused(&with_purpose(&["-t", "5", opt, "xlc-quick", "true"]));
            assert_eq!(
                refusal.message,
                format!(
                    "ERROR: 知らないオプション: {opt}（時間を記録するジョブと急ぐ計算は --timed）\n"
                )
            );
        }
    }

    #[test]
    #[ignore = "RED: 実装の前"]
    fn an_unknown_class_lists_the_classes() {
        let refusal = refused(&args(&[
            "--class",
            "urgent",
            "--ref",
            "#1",
            "--purpose",
            "p",
            "-t",
            "5",
            "xlc-2G",
            "true",
        ]));
        assert_eq!(refusal.name.as_deref(), Some("xlc-2G"));
        assert_eq!(
            refusal.message,
            "ERROR: 知らない区分: 'urgent'（timed record decision trial のどれか）\n"
        );
    }

    #[test]
    #[ignore = "RED: 実装の前"]
    fn the_class_timed_goes_uninterrupted_and_other_classes_keep_their_class_with_timed() {
        let timed = parse_args(&args(&[
            "--class",
            "timed",
            "--ref",
            "#1",
            "--purpose",
            "測定",
            "-t",
            "5",
            "xlc-bench",
            "true",
        ]))
        .unwrap();
        assert!(timed.timed);
        let quick = parse_args(&args(&[
            "--class",
            "decision",
            "--ref",
            "#1",
            "--purpose",
            "急ぐ確認",
            "--timed",
            "-t",
            "5",
            "xlc-quick",
            "true",
        ]))
        .unwrap();
        assert!(quick.timed);
        assert_eq!(quick.class, "decision");
        assert!(!request(&["-t", "5", "x", "true"]).timed);
    }

    #[test]
    #[ignore = "RED: 実装の前"]
    fn references_are_numbers_adrs_their_comma_list_or_a_dash_and_a_reason() {
        for good in [
            "#0200",
            "ADR-0175",
            "#0200,ADR-0175",
            "- 環境の確認で、記録の番号が無い",
        ] {
            let words = args(&[
                "--class",
                "trial",
                "--ref",
                good,
                "--purpose",
                "p",
                "-t",
                "5",
                "x",
                "true",
            ]);
            assert!(parse_args(&words).is_ok(), "{good}");
        }
        for bad in [
            "#",
            "ADR-",
            "-",
            "- ",
            "0200",
            "issue 200",
            "#0200,",
            "#02a0",
            "#0200 | x",
            "- 理由\n2 行目",
            "",
        ] {
            let words = args(&[
                "--class",
                "trial",
                "--ref",
                bad,
                "--purpose",
                "p",
                "-t",
                "5",
                "x",
                "true",
            ]);
            let refusal = refused(&words);
            assert!(
                refusal
                    .message
                    .starts_with("ERROR: 参照は #NNNN か ADR-NNNN"),
                "{bad:?}: {}",
                refusal.message
            );
        }
    }

    #[test]
    #[ignore = "RED: 実装の前"]
    fn a_purpose_must_be_one_non_blank_line_without_a_bar() {
        for bad in ["", "   ", "1 行目\n2 行目", "a | b"] {
            let words = args(&[
                "--class",
                "trial",
                "--ref",
                "#1",
                "--purpose",
                bad,
                "-t",
                "5",
                "x",
                "true",
            ]);
            let refusal = refused(&words);
            assert!(
                refusal
                    .message
                    .starts_with("ERROR: 目的は結果が何を決めるかの 1 文で"),
                "{bad:?}: {}",
                refusal.message
            );
        }
    }

    #[test]
    #[ignore = "RED: 実装の前"]
    fn timed_and_the_class_timed_cannot_go_with_cores_and_the_error_says_which_to_drop() {
        let with_flag = refused(&with_purpose(&[
            "--timed",
            "-c",
            "4",
            "-t",
            "5",
            "xlc-bench-shared",
            "true",
        ]));
        let with_class = refused(&args(&[
            "--class",
            "timed",
            "--ref",
            "#1",
            "--purpose",
            "測定",
            "-c",
            "4",
            "-t",
            "5",
            "xlc-bench",
            "true",
        ]));
        for refusal in [with_flag, with_class] {
            assert!(
                refusal
                    .message
                    .starts_with("ERROR: --timed と区分 timed は -c と組めない")
            );
            assert!(
                refusal
                    .message
                    .contains("時間を記録するなら -c を外し、記録しないなら --timed を外す")
            );
        }
    }

    #[test]
    #[ignore = "RED: 実装の前"]
    fn cores_must_be_a_positive_integer() {
        for bad in ["0", "-1", "abc", "1.5", ""] {
            let refusal = refused(&with_purpose(&[
                "-c",
                bad,
                "-t",
                "30",
                "openmc-2G-shared",
                "true",
            ]));
            assert_eq!(
                refusal.message,
                format!("ERROR: -c のコア数は正の整数: '{bad}'\n")
            );
        }
        assert_eq!(
            request(&["-c", "8", "-t", "30", "openmc-2G-shared", "true"])
                .cpus
                .as_deref(),
            Some("8")
        );
    }

    #[test]
    #[ignore = "RED: 実装の前"]
    fn an_estimate_in_another_form_is_refused_with_the_forms_to_use() {
        for bad in ["25m", "1h", "1.5", "abc", "1:30", "30:00"] {
            let refusal = refused(&with_purpose(&["-t", bad, "xlc-2G", "true"]));
            assert!(
                refusal
                    .message
                    .starts_with("ERROR: -t は分の数（-t 90）か H:MM:SS（-t 1:30:00）"),
                "{bad}: {}",
                refusal.message
            );
        }
        for good in ["90", "1:30:00", "1-00", "1-12:00", "2-12:00:00"] {
            assert_eq!(request(&["-t", good, "xlc-2G", "true"]).estimate, good);
        }
    }

    #[test]
    #[ignore = "RED: 実装の前"]
    fn a_command_running_usr_bin_time_is_refused_with_how_to_take_the_time() {
        for cmd in [
            "/usr/bin/time -v xs run.toml",
            "cd sub && /usr/bin/time xs run.toml",
            "env A=1 /usr/bin/time -f %e xs",
            "/usr/bin/time>t.txt xs",
            "{ /usr/bin/time; }",
            "/usr/bin/time<in xs",
            "/usr/bin/time",
        ] {
            let refusal = refused(&with_purpose(&["-t", "5", "xlc-2G", cmd]));
            assert!(refusal.message.starts_with("ERROR: /usr/bin/time"), "{cmd}");
            assert!(
                refusal
                    .message
                    .contains("sacct -j <番号>.batch -o Elapsed,MaxRSS")
            );
            assert!(
                refusal
                    .message
                    .contains(r#"command time -o <ファイル> -f "%e s %M KB""#)
            );
        }
        for cmd in [
            "command time -o t.txt xs",
            "time xs",
            "/usr/bin/timeout 5 xs",
            "ls /usr/bin/time-tools",
            "ls x/usr/bin/time",
        ] {
            assert!(parse(&["-t", "5", "xlc-2G", cmd]).is_ok(), "{cmd}");
        }
    }

    #[test]
    #[ignore = "RED: 実装の前"]
    fn the_name_is_file_name_characters_and_a_shared_job_ends_with_shared() {
        let refusal = refused(&with_purpose(&["-t", "1", "xlc 2G/a", "true"]));
        assert!(
            refusal
                .message
                .starts_with("ERROR: 名前はファイル名に使える英数字と . _ - だけにする")
        );
        let refusal = refused(&with_purpose(&["-c", "8", "-t", "30", "openmc-2G", "true"]));
        assert!(
            refusal.message.starts_with(
                "ERROR: -c のジョブの名前は末尾に -shared を付ける（openmc-2G-shared）"
            ),
            "{}",
            refusal.message
        );
    }

    #[test]
    #[ignore = "RED: 実装の前"]
    fn refusals_after_the_name_is_read_carry_the_name_for_the_result_line() {
        assert_eq!(
            refused(&with_purpose(&["-t", "25m", "xlc-early", "true"]))
                .name
                .as_deref(),
            Some("xlc-early")
        );
        assert_eq!(
            refused(&with_purpose(&["-t", "5", "xlc-early", "/usr/bin/time xs"]))
                .name
                .as_deref(),
            Some("xlc-early")
        );
    }

    #[test]
    #[ignore = "RED: 実装の前"]
    fn the_time_line_has_the_job_elapsed_estimate_state_class_reference_and_purpose() {
        let req = request(&["--timed", "-t", "30", "xlc-bench", "true"]);
        let timing = time_line("42", &record(Some(true), Some(0), completed()), &req);
        assert_eq!(
            timing.line,
            "TIME: job=42 elapsed=00:12:34 estimate=00:30:00 state=COMPLETED class=trial ref=#1 purpose=試験の目的"
        );
        assert!(!timing.marked);
        assert!(timing.warnings.is_empty());
    }

    #[test]
    #[ignore = "RED: 実装の前"]
    fn a_job_that_ran_interruptible_is_marked_preemptible_whatever_the_flag() {
        let req = request(&["--timed", "-t", "30", "xlc-bench", "true"]);
        let timing = time_line("42", &record(Some(false), Some(0), completed()), &req);
        assert!(
            timing.line.contains(" estimate=00:30:00 preemptible: 割り込まれうるので時間は記録に使わない（記録するなら排他の --timed） state="),
            "{}",
            timing.line
        );
        assert!(timing.marked);
        let main = request(&["-t", "30", "xlc-2G", "true"]);
        assert!(
            !time_line("42", &record(Some(true), Some(0), completed()), &main)
                .line
                .contains("preemptible")
        );
    }

    #[test]
    #[ignore = "RED: 実装の前"]
    fn shared_suspended_and_preemptible_marks_add_up_in_order() {
        let req = request(&["-c", "8", "-t", "30", "openmc-2G-shared", "true"]);
        let timing = time_line("42", &record(Some(false), Some(600), completed()), &req);
        assert!(
            timing.line.starts_with("TIME: job=42 elapsed=00:12:34 estimate=00:30:00 shared (-c 8): 時間は当てにならない preemptible: 割り込まれうるので時間は記録に使わない（記録するなら排他の --timed） suspended: 止められていたので時間は当てにならない state=COMPLETED"),
            "{}",
            timing.line
        );
        assert!(timing.marked);
        let timed = request(&["--timed", "-t", "30", "xlc-bench", "true"]);
        let suspended = time_line("42", &record(Some(true), Some(1), completed()), &timed);
        assert!(
            suspended
                .line
                .contains(" suspended: 止められていたので時間は当てにならない")
        );
        assert!(suspended.marked);
    }

    #[test]
    #[ignore = "RED: 実装の前"]
    fn a_record_without_the_partition_or_the_suspended_time_is_marked_unknown_with_warnings() {
        let req = request(&["-t", "30", "xlc-2G", "true"]);
        let timing = time_line("42", &record(None, None, completed()), &req);
        assert!(timing.line.contains(
            " partition=unknown: 割り込まれうるかを確かめられないので時間は記録に使わない"
        ));
        assert!(
            timing.line.contains(
                " suspended=unknown: 止められたかを確かめられないので時間は当てにならない"
            )
        );
        assert!(timing.marked);
        assert_eq!(
            timing.warnings,
            [
                "WARN: ジョブ 42 の sacct の記録に Partition が無く、割り込まれうるかを確かめられなかった",
                "WARN: ジョブ 42 の sacct の記録に Suspended が無く、止められていたかを確かめられなかった",
            ]
        );
    }

    #[test]
    #[ignore = "RED: 実装の前"]
    fn a_record_without_times_leaves_them_empty() {
        let req = request(&["-t", "30", "xlc-2G", "true"]);
        let mut bare = record(Some(true), Some(0), state("CANCELLED", true, false));
        bare.elapsed = None;
        bare.limit = None;
        assert!(
            time_line("42", &bare, &req)
                .line
                .starts_with("TIME: job=42 elapsed= estimate= state=CANCELLED class=")
        );
    }

    #[test]
    #[ignore = "RED: 実装の前"]
    fn durations_are_written_with_days_when_over_a_day() {
        assert_eq!(duration(754), "00:12:34");
        assert_eq!(duration(0), "00:00:00");
        assert_eq!(duration(90061), "1-01:01:01");
        assert_eq!(limit_text(&Some(Limit::Minutes(30))), "00:30:00");
        assert_eq!(limit_text(&Some(Limit::Minutes(2160))), "1-12:00:00");
        assert_eq!(limit_text(&Some(Limit::Unlimited)), "UNLIMITED");
        assert_eq!(limit_text(&Some(Limit::Unset)), "Partition_Limit");
        assert_eq!(limit_text(&None), "");
    }

    #[test]
    #[ignore = "RED: 実装の前"]
    fn the_wait_reads_the_record_until_an_ended_state_arrives() {
        // 2 回は記録が届かず、1 回は走っていて、4 回目に終わった
        let answers = [None, None, Some(false), Some(true)];
        let (mut reads, mut pauses) = (0, 0);
        let got = wait_record(
            10,
            || {
                reads += 1;
                answers[reads - 1]
                    .map(|ended| record(Some(false), Some(0), state("X", ended, ended)))
            },
            || pauses += 1,
        );
        assert!(got.is_some_and(|r| r.state.ended));
        assert_eq!((reads, pauses), (4, 3));
    }

    #[test]
    #[ignore = "RED: 実装の前"]
    fn the_wait_gives_up_after_the_tries_without_pausing_after_the_last() {
        let (mut reads, mut pauses) = (0, 0);
        let got = wait_record(
            3,
            || {
                reads += 1;
                Some(record(
                    Some(false),
                    Some(0),
                    state("CONFIGURING", false, false),
                ))
            },
            || pauses += 1,
        );
        assert_eq!(got, None);
        assert_eq!((reads, pauses), (3, 2));
    }

    #[test]
    #[ignore = "RED: 実装の前"]
    fn a_job_that_did_not_complete_fails_even_when_submission_returned_zero() {
        assert_eq!(final_status(0, &completed()), 0);
        assert_eq!(final_status(0, &state("CANCELLED", true, false)), 1);
        assert_eq!(final_status(0, &unknown_state()), 1);
        assert_eq!(final_status(3, &state("FAILED", true, false)), 3);
        assert_eq!(final_status(3, &unknown_state()), 3);
    }

    #[test]
    #[ignore = "RED: 実装の前"]
    fn the_exit_line_adds_the_state_unless_completed_and_the_result_line_names_the_job() {
        assert_eq!(exit_line(0, &completed()), "EXIT: 0");
        assert_eq!(
            exit_line(1, &state("CANCELLED", true, false)),
            "EXIT: 1 (State=CANCELLED)"
        );
        assert_eq!(exit_line(1, &unknown_state()), "EXIT: 1 (State=unknown)");
        assert_eq!(
            result_line(Some("42"), Some("xlc-2G"), 3, "FAILED"),
            "RESULT: job=42 name=xlc-2G exit=3 state=FAILED"
        );
        assert_eq!(
            result_line(None, None, 1, "NOT_SUBMITTED"),
            "RESULT: job=- name=- exit=1 state=NOT_SUBMITTED"
        );
    }

    /// reorder に、読むたびに answers を順に返させ、先頭に出した列を集める
    fn reorder_with(answers: &[Option<&str>]) -> (Option<String>, Vec<String>) {
        let mut reads = answers.iter();
        let mut tops = Vec::new();
        let warning = reorder(
            || reads.next().copied().flatten().map(str::to_string),
            |list| tops.push(list.to_string()),
        );
        (warning, tops)
    }

    #[test]
    #[ignore = "RED: 実装の前"]
    fn a_queue_already_in_class_order_is_not_touched() {
        assert_eq!(reorder_with(&[Some("")]), (None, vec![]));
    }

    #[test]
    #[ignore = "RED: 実装の前"]
    fn the_queue_is_read_again_after_each_top_until_no_reordering_is_needed() {
        // 並べ直しが拒まれても、同時に投げたジョブが列を上書きしても、読み直して並べ直す
        assert_eq!(
            reorder_with(&[Some("21,42,20\n"), Some("21,42,23\n"), Some("")]),
            (None, vec!["21,42,20".to_string(), "21,42,23".to_string()])
        );
    }

    #[test]
    #[ignore = "RED: 実装の前"]
    fn after_three_tops_the_order_is_left_to_the_queue_with_a_warning() {
        let (warning, tops) = reorder_with(&[Some("42,20"); 5]);
        assert_eq!(tops.len(), 3);
        assert_eq!(
            warning.as_deref(),
            Some("WARN: 3 回並べ直しても区分の順（42,20）にならなかった。投げた順に走る")
        );
    }

    #[test]
    #[ignore = "RED: 実装の前"]
    fn an_unreadable_queue_warns_and_does_not_reorder() {
        let (warning, tops) = reorder_with(&[None]);
        assert!(tops.is_empty());
        assert_eq!(
            warning.as_deref(),
            Some("WARN: 待ち行列を読めず、区分の順に並べ直せなかった。投げた順に走る")
        );
    }

    #[test]
    #[ignore = "RED: 実装の前"]
    fn a_running_job_in_the_same_worktree_is_found_through_the_worktree_of_its_directory() {
        let job = |id: &str, dir: &str| RunningJob {
            id: id.to_string(),
            dir: dir.to_string(),
        };
        let running = vec![
            job("700", "/r/other"),
            job("", "/r/proj"),
            job("701", ""),
            job("702", "/r/proj/sub"),
        ];
        let top = |dir: &str| dir.strip_suffix("/sub").unwrap_or(dir).to_string();
        assert_eq!(same_worktree_job(&running, "/r/proj", top), Some("702"));
        assert_eq!(same_worktree_job(&running, "/r/none", top), None);
    }

    #[test]
    #[ignore = "RED: 実装の前"]
    fn paths_are_rewritten_between_the_r995_home_and_the_nfs_entry() {
        let (nfs, root) = ("/mnt/r995/github", "/home/u/github");
        assert_eq!(
            local_path("/mnt/r995/github/proj", true, nfs, root),
            "/home/u/github/proj"
        );
        assert_eq!(
            local_path("/home/u/github/proj", false, nfs, root),
            "/mnt/r995/github/proj"
        );
        assert_eq!(
            local_path("/home/u/github/proj", true, nfs, root),
            "/home/u/github/proj"
        );
        assert_eq!(
            local_path("/home/u/githubx", false, nfs, root),
            "/home/u/githubx"
        );
        assert_eq!(
            local_path("/home/u/github", false, nfs, root),
            "/mnt/r995/github"
        );
        assert!(under("/mnt/r995/github", "/mnt/r995/github"));
        assert!(!under("/mnt/r995/github2/x", "/mnt/r995/github"));
    }

    #[test]
    #[ignore = "RED: 実装の前"]
    fn the_tail_is_the_last_lines_like_tail_n() {
        assert_eq!(tail_lines(b"a\nb\nc\n", 2), b"b\nc\n");
        assert_eq!(tail_lines(b"a\nb\nc", 1), b"c");
        assert_eq!(tail_lines(b"a\nb\n", 20), b"a\nb\n");
        assert_eq!(tail_lines(b"", 20), b"");
        assert_eq!(tail_lines(b"\n\n", 1), b"\n");
    }
}
