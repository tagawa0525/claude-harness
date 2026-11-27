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

/// 終わった後に出す、ログの末尾の行数
const TAIL_LINES: usize = 20;

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

/// RESULT: の行に要る、ここまでに分かったこと。中断を受けたスレッドと分け合う（slurm-wait も使う）
#[derive(Default)]
pub(super) struct Progress {
    pub(super) name: Option<String>,
    pub(super) job: Option<String>,
    reported: bool,
}

impl Progress {
    /// RESULT: の行を、まだ出していなければ出す
    fn report(&mut self, exit: i32, state: &str) {
        if !self.reported {
            println!(
                "{}",
                result_line(self.job.as_deref(), self.name.as_deref(), exit, state)
            );
            self.reported = true;
        }
    }

    /// 終わりの状態が分からないまま終えるときの RESULT: の行（投げる前なら NOT_SUBMITTED、投げた後なら unknown）
    pub(super) fn report_unfinished(&mut self, exit: i32) {
        let state = if self.job.is_some() {
            "unknown"
        } else {
            "NOT_SUBMITTED"
        };
        self.report(exit, state);
    }
}

pub(super) fn lock(progress: &Mutex<Progress>) -> std::sync::MutexGuard<'_, Progress> {
    progress.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn main(args: &[String]) -> i32 {
    let progress = Arc::new(Mutex::new(Progress::default()));
    let shared = Arc::clone(&progress);
    if let Err(e) = crate::interrupt::cleanup_on_interrupt(move || {
        lock(&shared).report_unfinished(crate::interrupt::INTERRUPTED)
    }) {
        eprintln!("ERROR: 中断のときの後片付けを登録できない（RESULT: の行を出せない）: {e}");
        lock(&progress).report_unfinished(1);
        return 1;
    }
    let code = run(args, &progress);
    lock(&progress).report_unfinished(code);
    code
}

/// 投げて待つ。RESULT: の行は、終わりの状態が分かったときだけここで出す（ほかは呼ぶ側が出す）
fn run(args: &[String], progress: &Mutex<Progress>) -> i32 {
    let req = match parse_args(args) {
        Ok(req) => req,
        Err(refusal) => {
            lock(progress).name = refusal.name;
            eprint!("{}", refusal.message);
            return 1;
        }
    };
    lock(progress).name = Some(req.name.clone());
    let (tries, interval) = match record_wait() {
        Ok(wait) => wait,
        Err(e) => {
            eprintln!("ERROR: {e}");
            return 1;
        }
    };
    let pwd = match working_dir() {
        Ok(pwd) => pwd,
        Err(e) => {
            eprintln!("ERROR: 作業ディレクトリを読めない: {e}");
            return 1;
        }
    };

    // ジョブの走る r995 の側のパスと、このホストから読むパス。r995 の ~/github と、ほかのホストの NFS の入口は同じ場所
    // （走っているジョブの作業ディレクトリを、このホストの書き方に直すのにも使う）
    let (remote_root, nfs_entry) = roots();
    let remote_logs = format!("{remote_root}/slurm-logs");
    let on_r995 = hostname() == "r995";
    let local_logs = if on_r995 {
        remote_logs.clone()
    } else {
        if !under(&pwd, &nfs_entry) {
            eprintln!(
                "ERROR: r995 以外では {nfs_entry} の下で投げる（ジョブは投げたときのパスで r995 の上で走る）: {pwd}"
            );
            // 手元の worktree を r995 の側のパスに読み替えて ssh で投げると、r995 の側の版（push していない変更を
            // 含まない）が黙って走るので、自動では回さず、r995 の worktree で投げる形を示す。入口があるかは stat しない
            // （ハードマウントの NFS は、r995 が落ちていると stat で止まる）
            eprintln!(
                "NEXT: {nfs_entry} のあるホストは、その下に cd して投げ直す。無いホストでは、ブランチを push し、r995 の \
                 ~/github の下にその worktree を用意して、r995 の slurm-run.sh を ssh で呼ぶ（run_in_background で）: \
                 ssh r995 'cd ~/github/<worktree> && ~/.claude/scripts/slurm-run.sh <同じ引数>'。元の引数の単一引用符は、\
                 外側の単一引用符を閉じるので '\\'' と書く。外側を二重引用符で囲むと、~ と $HOME が手元のパスに展開され、\
                 ジョブが r995 で失敗する。ジョブは r995 の ssh の環境で走るので、開発シェルの PATH が要るコマンドは \
                 nix develop -c で包み、連絡先に claude= を付けるには slurm-run.sh の前に \
                 CLAUDE_CODE_SESSION_ID=<この会話の ID> を置く"
            );
            return 1;
        }
        format!("{nfs_entry}/slurm-logs")
    };
    if let Err(e) = std::fs::create_dir_all(&local_logs) {
        eprintln!("ERROR: ログの置き場 {local_logs} を作れない: {e}");
        return 1;
    }

    let mut contact = contact(&pwd);
    if let Some(cpus) = &req.cpus {
        // 排他でないジョブが走っていると、排他のジョブが始まらない理由を待ち行列の一覧で読めるようにする
        contact.push_str(&format!(" shared-cpus={cpus}"));
    }

    if req.timed {
        let local = |dir: &str| local_path(dir, on_r995, &nfs_entry, &remote_root);
        if let Some(code) = refuse_same_worktree(&pwd, local) {
            return code;
        }
    }

    // 終わるまで戻らないので、子のプロセスで起動し、最初の行から番号を読む
    let comment = format!(
        "{} | {} | {} | {contact}",
        req.class, req.reference, req.purpose
    );
    let submission = Submission {
        cpus: req.cpus.as_deref(),
        uninterrupted: req.timed,
        name: &req.name,
        log_dir: &remote_logs,
        estimate: &req.estimate,
        comment: &comment,
        command: &req.command,
    };
    let mut child = match slurm::submit(&submission) {
        Ok(child) => child,
        Err(e) => {
            eprintln!("sbatch を起動できない: {e}");
            eprintln!("ERROR: sbatch がジョブを投げられなかった:");
            return 1;
        }
    };
    let mut submit_out = BufReader::new(child.stdout.take().expect("stdout はパイプ"));
    let mut first = Vec::new();
    let _ = submit_out.read_until(b'\n', &mut first);
    let Some(job) = slurm::submitted_job(&String::from_utf8_lossy(&first)) else {
        let _ = submit_out.read_to_end(&mut first);
        let _ = child.wait();
        eprintln!("ERROR: sbatch がジョブを投げられなかった:");
        let _ = std::io::stderr().write_all(&first);
        return 1;
    };
    lock(progress).job = Some(job.clone());
    println!("JOB: {job}");
    let log = format!("{local_logs}/{}", slurm::log_file(&req.name, &job));
    println!("LOG: {log}");
    // ジョブは投げたときの場所で走る。`cd d && a & b` の b のように、cd が届かずに別の場所から投げたものを、終わる前に見つける
    println!(
        "NEXT: ジョブは {pwd} で走る。意図した worktree でなければ scancel {job} で取り消し、cd <worktree> && で投げ直す"
    );

    // main は投げるたびに区分の順に並べ直す（並べ方は slurm-queue と同じ関数。--top のジョブは自分の区分の先頭、無ければ
    // 末尾）。timed は投げた順に実行し、--top のときだけ投げたジョブを先頭に出す。並べ替えに失敗しても、投げたジョブは
    // 待ち行列にあるので、終わりまで待つ
    if req.timed {
        if req.top && !slurm::move_to_front(&job) {
            eprintln!("WARN: ジョブ {job} を待ち行列の先頭に移せなかった。投げた順に走る");
        }
    } else {
        let first = if req.top { job.as_str() } else { "" };
        let warning = reorder(
            || slurm_queue::order(first),
            |list| {
                slurm::move_to_front(list);
            },
        );
        if let Some(warning) = warning {
            eprintln!("{warning}");
        }
    }

    // 残りの出力は読み捨て、閉じるまで読んでから終わりを受ける（パイプが埋まって子が止まらないように）
    let _ = std::io::copy(&mut submit_out, &mut std::io::sink());
    let status = match child.wait() {
        Ok(status) => crate::child::exit_code(status),
        Err(e) => {
            eprintln!("WARN: sbatch の終わりを受けられなかった: {e}");
            1
        }
    };

    finish(
        &job,
        &log,
        &req.labels(),
        status,
        (tries, interval),
        progress,
    )
}

/// TIME: の行の区分、参照、目的と、-c のコア数（投げたときの引数か、ジョブのコメントから）
#[derive(Debug, PartialEq)]
pub(super) struct Labels<'a> {
    pub(super) class: &'a str,
    pub(super) reference: &'a str,
    pub(super) purpose: &'a str,
    pub(super) cpus: Option<&'a str>,
}

impl Request {
    fn labels(&self) -> Labels<'_> {
        Labels {
            class: &self.class,
            reference: &self.reference,
            purpose: &self.purpose,
            cpus: self.cpus.as_deref(),
        }
    }
}

/// 終わったジョブの結果を出す（slurm-run と slurm-wait の終わり）。会計の記録（終わった状態）が届くまで待ち
/// （[`wait_record`]）、届かなければ制御のデーモンの状態で確かめる。TIME: の行は、ログにまだ無ければ書き足す（slurm-run
/// が書いた後のジョブを slurm-wait が待っても重ねない）。ログの末尾、印の NEXT:、RESULT:、EXIT: を出し、終了コードを返す。
/// status は、待ったコマンドが受けたジョブの終了コード（sbatch --wait の終了コードと同じ形）
pub(super) fn finish(
    job: &str,
    log: &str,
    labels: &Labels,
    status: i32,
    (tries, interval): (u32, std::time::Duration),
    progress: &Mutex<Progress>,
) -> i32 {
    let record = wait_record(
        tries,
        || slurm::accounting_record(job),
        || std::thread::sleep(interval),
    );

    let mut marked = false;
    let state = match &record {
        None => {
            eprintln!(
                "WARN: ジョブ {job} の記録が sacct から読めなかった（{tries} 回。実際の時間は確かめられない）"
            );
            // 終わりの状態を確かめられないまま 0 を返すと、取り消されたジョブでも && の次が走る。終わった直後なら
            // 制御のデーモンが覚えているので、その状態で確かめる。これも読めなければ、成功とは言えないので失敗にする
            match slurm::controller_state(job).filter(|state| state.ended) {
                Some(state) => state,
                None => {
                    eprintln!(
                        "WARN: ジョブ {job} の終わりの State を sacct からも scontrol からも確かめられなかった"
                    );
                    JobState::unknown()
                }
            }
        }
        Some(record) => {
            let timing = time_line(job, record, labels);
            for warning in &timing.warnings {
                eprintln!("{warning}");
            }
            marked = timing.marked;
            if !has_time_line(log, job)
                && let Err(e) = append_line(log, &timing.line)
            {
                eprintln!("WARN: ジョブ {job} の実際の時間をログに書き足せなかった: {log}（{e}）");
                println!("{}", timing.line);
            }
            record.state.clone()
        }
    };
    let status = final_status(status, &state);
    if let Ok(text) = file_tail(log, TAIL_LINES) {
        let mut out = std::io::stdout().lock();
        let _ = out.write_all(tail_lines(&text, TAIL_LINES));
        let _ = out.flush();
    }
    if marked {
        println!("{NEXT_MARKED}");
    }
    lock(progress).report(status, &state.name);
    println!("{}", exit_line(status, &state));
    status
}

/// ログの末尾に、このジョブの TIME: の行があるか（読めなければ無いとみなす）
fn has_time_line(log: &str, job: &str) -> bool {
    let head = format!("TIME: job={job} ");
    file_tail(log, TAIL_LINES).is_ok_and(|text| {
        String::from_utf8_lossy(&text)
            .lines()
            .any(|line| line.starts_with(&head))
    })
}

/// 引数を読んで確かめる。確かめる順は、使い方の誤り、区分、参照、目的、-c、-t、コマンド、名前（先に当たった理由だけを出す）
fn parse_args(args: &[String]) -> Result<Request, Refusal> {
    let refuse = |name: Option<&str>, message: String| Refusal {
        name: name.map(str::to_string),
        message,
    };
    let wrong = || refuse(None, format!("{}\n", usage()));
    let missing = |flag: &str| {
        refuse(
            None,
            format!(
                "ERROR: {flag} が無い（待ち行列で順を確かめるため、区分、参照、目的は必須）\n{}\n",
                usage()
            ),
        )
    };
    let mut class = String::new();
    let mut reference = None;
    let mut purpose = None;
    let mut top = false;
    let mut timed = false;
    let mut estimate = String::new();
    let mut cpus = None;
    let mut i = 0;
    while i < args.len() {
        let value = args.get(i + 1).cloned();
        match args[i].as_str() {
            "--class" => class = value.ok_or_else(wrong)?,
            "--ref" => {
                // 囲まない #0200 は、# から後ろ（後ろの引数を含む）がシェルの注釈になり、ここには値の無い --ref だけが届く
                reference = Some(value.ok_or_else(|| refuse(None, format!(
                    "ERROR: --ref の値が無い。# で始まる参照は引用符で囲む（--ref '#0200'。囲まないと # から後ろがシェルの注釈になり、後ろの引数も消える）\n{}\n",
                    usage()
                )))?);
            }
            "--purpose" => purpose = Some(value.ok_or_else(wrong)?),
            "--top" => {
                top = true;
                i += 1;
                continue;
            }
            "--timed" => {
                timed = true;
                i += 1;
                continue;
            }
            "-t" => estimate = value.ok_or_else(wrong)?,
            "-c" => cpus = Some(value.ok_or_else(wrong)?),
            flag if flag.starts_with('-') => {
                return Err(refuse(
                    None,
                    format!(
                        "ERROR: 知らないオプション: {flag}（時間を記録するジョブと急ぐ計算は --timed）\n"
                    ),
                ));
            }
            _ => break,
        }
        i += 2;
    }
    if class.is_empty() {
        return Err(missing("--class"));
    }
    let Some(reference) = reference else {
        return Err(missing("--ref"));
    };
    let Some(purpose) = purpose else {
        return Err(missing("--purpose"));
    };
    let [name, command] = &args[i..] else {
        return Err(wrong());
    };
    if estimate.is_empty() {
        return Err(wrong());
    }
    let named = |message: String| Err(refuse(Some(name), message));

    match slurm_class::rank(&class) {
        Some(_) if class == "timed" => timed = true,
        Some(_) => {}
        None => {
            return named(format!(
                "ERROR: 知らない区分: '{class}'（{} のどれか）\n",
                slurm_class::CLASSES.join(" ")
            ));
        }
    }
    // 改行と | はコメントの区切りを壊すので先に除く
    if reference.contains(['\n', '|']) || !REFERENCE.is_match(&reference) {
        return named(format!(
            "ERROR: 参照は #NNNN か ADR-NNNN（複数はカンマで区切る）、無いときは「- <理由>」: '{reference}'\n"
        ));
    }
    if purpose.contains(['\n', '|']) || purpose.chars().all(char::is_whitespace) {
        return named(format!(
            "ERROR: 目的は結果が何を決めるかの 1 文で、空にせず、改行と | を含めない: '{purpose}'\n"
        ));
    }
    if timed && cpus.is_some() {
        return named("ERROR: --timed と区分 timed は -c と組めない（排他でないジョブの時間は記録に使えない）。時間を記録するなら -c を外し、記録しないなら --timed を外す（区分 timed なら区分を変える）\n".to_string());
    }
    if let Some(cpus) = cpus.as_deref().filter(|c| !CPUS.is_match(c)) {
        return named(format!("ERROR: -c のコア数は正の整数: '{cpus}'\n"));
    }
    // 投げてから拒まれると、理由がスケジューラの英語の 1 行だけになるので、先に確かめて修正方法を示す
    if !slurm::valid_time_limit(&estimate) {
        return named(format!(
            "ERROR: -t は分の数（-t 90）か H:MM:SS（-t 1:30:00）。25m や 1h の形は sbatch が受け付けず、1:30 は sbatch が 1 分 30 秒と読む: '{estimate}'\n"
        ));
    }
    if USR_BIN_TIME.is_match(command) {
        return named("ERROR: /usr/bin/time は r995 に無く、ジョブが 0 秒で失敗する。ジョブの時間とメモリは TIME: 行と sacct -j <番号>.batch -o Elapsed,MaxRSS で取る。プログラムだけの時間が要るなら、ジョブの PATH に GNU time があることを type -P time で確かめてから command time -o <ファイル> -f \"%e s %M KB\" <コマンド> と書く\n".to_string());
    }
    if !NAME.is_match(name) {
        return named(format!(
            "ERROR: 名前はファイル名に使える英数字と . _ - だけにする: {name}\n"
        ));
    }
    // 会計の記録を手で読むときに、名前で排他のジョブと見分ける（slurm-history.sh も古い jobcomp では名前で分ける）
    if cpus.is_some() && !name.ends_with("-shared") {
        return named(format!(
            "ERROR: -c のジョブの名前は末尾に -shared を付ける（{name}-shared）。sacct で排他のジョブと見分けるため: {name}\n"
        ));
    }
    Ok(Request {
        class,
        reference,
        purpose,
        top,
        timed,
        cpus,
        estimate,
        name: name.clone(),
        command: command.clone(),
    })
}

fn usage() -> String {
    format!(
        "Usage: slurm-run.sh --class <{}> --ref <#NNNN|ADR-NNNN|- 理由> --purpose <結果が何を決めるか> [--top] [--timed | -c <コア数>] -t <見積もり> <name> <command>",
        slurm_class::CLASSES.join("|")
    )
}

/// 会計の記録を待つ回数と間隔（SLURM_RUN_SACCT_TRIES、SLURM_RUN_SACCT_INTERVAL。テストが短くする）
pub(super) fn record_wait() -> Result<(u32, std::time::Duration), String> {
    let var = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
    let tries = match var("SLURM_RUN_SACCT_TRIES") {
        None => 10,
        Some(v) => v
            .parse()
            .map_err(|_| format!("SLURM_RUN_SACCT_TRIES は回数: '{v}'"))?,
    };
    let interval = match var("SLURM_RUN_SACCT_INTERVAL") {
        None => 1.0,
        Some(v) => v
            .parse::<f64>()
            .ok()
            .filter(|s| s.is_finite() && *s >= 0.0)
            .ok_or_else(|| format!("SLURM_RUN_SACCT_INTERVAL は秒: '{v}'"))?,
    };
    Ok((tries, std::time::Duration::from_secs_f64(interval)))
}

/// 作業ディレクトリ。環境の PWD が今の場所を指していればそれ（シェルの $PWD と同じく、symlink をたどる前の書き方）、
/// 指していなければ OS の今の場所。連絡先とジョブの走る場所の案内は、利用者が cd したときの書き方で出す
fn working_dir() -> std::io::Result<String> {
    let cwd = std::env::current_dir()?;
    #[cfg(unix)]
    if let Some(pwd) = std::env::var_os("PWD").map(std::path::PathBuf::from) {
        use std::os::unix::fs::MetadataExt;
        let same =
            |a: &std::fs::Metadata, b: &std::fs::Metadata| a.dev() == b.dev() && a.ino() == b.ino();
        if pwd.is_absolute()
            && let (Ok(p), Ok(c)) = (std::fs::metadata(&pwd), std::fs::metadata(&cwd))
            && same(&p, &c)
        {
            return Ok(pwd.display().to_string());
        }
    }
    Ok(cwd.display().to_string())
}

pub(super) fn hostname() -> String {
    Command::new("hostname")
        .output()
        .map(|out| {
            String::from_utf8_lossy(&out.stdout)
                .trim_end_matches('\n')
                .to_string()
        })
        .unwrap_or_default()
}

/// r995 の ~/github と、ほかのホストから見たその NFS の入口
pub(super) fn roots() -> (String, String) {
    let remote_root = format!("{}/github", crate::shell::home().display());
    let nfs_entry = std::env::var("SLURM_RUN_NFS_ENTRY")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| NFS_ENTRY.to_string());
    (remote_root, nfs_entry)
}

/// path が dir の下か（dir そのものを含む）
fn under(path: &str, dir: &str) -> bool {
    format!("{path}/").starts_with(&format!("{dir}/"))
}

/// ジョブの作業ディレクトリ（r995 から投げたら ~/github の下、ほかのホストからなら NFS の入口の下）を、このホストの
/// 書き方に直す
pub(super) fn local_path(path: &str, on_r995: bool, nfs_entry: &str, remote_root: &str) -> String {
    let (from, to) = if on_r995 {
        (nfs_entry, remote_root)
    } else {
        (remote_root, nfs_entry)
    };
    if under(path, from) {
        format!("{to}{}", &path[from.len()..])
    } else {
        path.to_string()
    }
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
    run_git(Command::new("git").args(args))
}

fn run_git(cmd: &mut Command) -> GitOut {
    match cmd.output() {
        Ok(out) => GitOut {
            ok: out.status.success(),
            stdout: String::from_utf8_lossy(&out.stdout)
                .trim_end_matches('\n')
                .to_string(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        },
        Err(e) => GitOut {
            ok: false,
            stdout: String::new(),
            stderr: format!("git を起動できない: {e}\n"),
        },
    }
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
    let mut contact = pwd.to_string();
    let branch = git(&["symbolic-ref", "--short", "-q", "HEAD"]);
    if branch.ok {
        contact = format!("{} {pwd}", branch.stdout);
    }
    // メッセージの言語に依らず「git の外」を見分けるため LC_ALL=C で読む。外なら黙って付けず、外でない失敗は警告する
    let inside = run_git(
        Command::new("git")
            .args(["rev-parse", "--is-inside-work-tree"])
            .env("LC_ALL", "C"),
    );
    if !inside.ok {
        if !inside.stderr.contains("not a git repository") {
            eprintln!(
                "WARN: commit= を付けません（git rev-parse --is-inside-work-tree が失敗）: {}",
                inside.first_error_line()
            );
        }
    } else if inside.stdout != "true" {
        eprintln!(
            "WARN: commit= を付けません（作業ツリーの外。bare リポジトリか .git の中）: {pwd}"
        );
    } else {
        let head = git(&["rev-parse", "-q", "--verify", "--short", "HEAD"]);
        if head.ok {
            let status = git(&[
                "--no-optional-locks",
                "status",
                "--porcelain",
                "--untracked-files=normal",
                "--ignore-submodules=none",
            ]);
            if status.ok {
                let dirty = if status.stdout.is_empty() {
                    ""
                } else {
                    "-dirty"
                };
                contact.push_str(&format!(" commit={}{dirty}", head.stdout));
            } else {
                eprintln!(
                    "WARN: commit= を付けません（git status が失敗）: {}",
                    status.first_error_line()
                );
            }
        } else if !head.stderr.is_empty() {
            eprintln!(
                "WARN: commit= を付けません（git rev-parse HEAD が失敗）: {}",
                head.first_error_line()
            );
        }
    }
    if let Some(session) = std::env::var("CLAUDE_CODE_SESSION_ID")
        .ok()
        .filter(|s| !s.is_empty())
    {
        contact.push_str(&format!(" claude={session}"));
    }
    contact
}

/// timed のジョブは走っている main のジョブを止め、止められたジョブは持っているロック（cargo のビルドディレクトリ、
/// DVC の .dvc/tmp/lock）を持ったまま止まる。同じ worktree の timed のジョブは同じロックを待って、どちらも進まない。
/// worktree によらないロック（$CARGO_HOME、nix の評価のキャッシュ）はコマンドから決められないので見ない。
/// 場所はジョブの作業ディレクトリ（投げたときの場所）で読む。コメントの連絡先と違い、slurm-run.sh の外から投げた
/// ジョブにもある。同じ worktree のジョブがあれば、理由を出して終了コード 1 を返す
fn refuse_same_worktree(pwd: &str, local: impl Fn(&str) -> String) -> Option<i32> {
    let running = match slurm::running_default_jobs() {
        Ok(running) => running,
        Err(reason) => {
            eprintln!(
                "WARN: 走っている main のジョブを読めず、同じ worktree で走っているかを確かめられなかった（squeue が失敗）: {reason}"
            );
            return None;
        }
    };
    let here = worktree_of(pwd);
    let id = same_worktree_job(&running, &here, |dir| worktree_of(&local(dir)))?;
    eprintln!(
        "ERROR: 走っている main のジョブ {id} が同じ worktree（{here}）で走っている。timed のジョブはそれを止め、止められたジョブはロック（cargo のビルドディレクトリ、DVC の lock）を持ったまま止まるので、同じロックを取るとどちらも進まない"
    );
    eprintln!(
        "NEXT: 別の worktree（git worktree add）から投げるか、ジョブ {id} が終わってから投げる"
    );
    Some(1)
}

/// 作業ディレクトリの worktree の直下（git の外ならそのディレクトリ）。crate::git::git は起動の失敗を、slurm-run が
/// 出さない通知の置き場に入れるので使わない
fn worktree_of(dir: &str) -> String {
    let out = git(&["-C", dir, "rev-parse", "--show-toplevel"]);
    if out.ok { out.stdout } else { dir.to_string() }
}

/// running のうち、作業ディレクトリの worktree が here と同じ最初のジョブの番号
fn same_worktree_job<'a>(
    running: &'a [RunningJob],
    here: &str,
    worktree_of: impl Fn(&str) -> String,
) -> Option<&'a str> {
    running
        .iter()
        .filter(|job| !job.id.is_empty() && !job.dir.is_empty())
        .find(|job| worktree_of(&job.dir) == here)
        .map(|job| job.id.as_str())
}

/// 並べ直す。read は slurm-queue の --order と同じ列（要らなければ空、読めなければ None）、top は待ち行列の先頭に列を
/// 出す。警告があれば返す。
///
/// scontrol top は、走り始めたジョブや取り消したジョブを含む列を拒まない（終了コード 0。存在しない番号だけ拒む。r995 で
/// 2026-10-30 に確かめた）。同時に投げたほかの slurm-run の列が後から通ると、こちらの並べ直しを上書きする。なので
/// 成功しても待ち行列を読み直し、並べ直しが要らなくなるまで繰り返す（拒まれたときも、読み直せば消えた番号は列から外れる）。
/// 3 回で収まらなければ、待ち行列が動き続けているので投げた順に任せる
fn reorder(mut read: impl FnMut() -> Option<String>, mut top: impl FnMut(&str)) -> Option<String> {
    let mut tops = 0;
    loop {
        let Some(list) = read() else {
            return Some(
                "WARN: 待ち行列を読めず、区分の順に並べ直せなかった。投げた順に走る".to_string(),
            );
        };
        let list = list.trim_end_matches('\n');
        if list.is_empty() {
            return None;
        }
        if tops == 3 {
            return Some(format!(
                "WARN: 3 回並べ直しても区分の順（{list}）にならなかった。投げた順に走る"
            ));
        }
        top(list);
        tops += 1;
    }
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
    for attempt in 1..=tries {
        if let Some(record) = read().filter(|r| r.state.ended) {
            return Some(record);
        }
        if attempt != tries {
            pause();
        }
    }
    None
}

/// 秒を [日-]時:分:秒 にする（1 日に満たなければ時:分:秒。sacct の Elapsed と同じ書き方）
fn duration(secs: i64) -> String {
    if secs < 0 {
        return "INVALID".to_string();
    }
    let (days, hours, minutes, seconds) =
        (secs / 86400, secs / 3600 % 24, secs / 60 % 60, secs % 60);
    if days > 0 {
        format!("{days}-{hours:02}:{minutes:02}:{seconds:02}")
    } else {
        format!("{hours:02}:{minutes:02}:{seconds:02}")
    }
}

/// 見積もりを、TIME: の行の書き方（sacct の Timelimit と同じ）にする
fn limit_text(limit: &Option<Limit>) -> String {
    match limit {
        Some(Limit::Minutes(minutes)) => duration(minutes * 60),
        Some(Limit::Unlimited) => "UNLIMITED".to_string(),
        Some(Limit::Unset) => "Partition_Limit".to_string(),
        None => String::new(),
    }
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
fn time_line(job: &str, record: &JobRecord, labels: &Labels) -> Timing {
    let mut warnings = Vec::new();
    let plain = format!(
        "TIME: job={job} elapsed={} estimate={}",
        record.elapsed.map(duration).unwrap_or_default(),
        limit_text(&record.limit)
    );
    let mut line = plain.clone();
    if let Some(cpus) = labels.cpus {
        line.push_str(&format!(" shared (-c {cpus}): 時間は当てにならない"));
    }
    // 止められたかによらず付ける。止められなかったジョブも、割り込まれうる条件で走った値で、付け忘れた --timed を見分けるため。
    // flag でなく実際に走ったパーティションで決める（SBATCH_PARTITION などで変わりうる。slurm-history.sh も jobcomp の
    // Partition で決める）
    match record.uninterrupted {
        None => {
            warnings.push(format!(
                "WARN: ジョブ {job} の sacct の記録に Partition が無く、割り込まれうるかを確かめられなかった"
            ));
            line.push_str(
                " partition=unknown: 割り込まれうるかを確かめられないので時間は記録に使わない",
            );
        }
        Some(true) => {}
        Some(false) => line.push_str(
            " preemptible: 割り込まれうるので時間は記録に使わない（記録するなら排他の --timed）",
        ),
    }
    // 割り込まれないジョブに止められた印が付けば、設定か運用（scontrol suspend）の誤り。走った時間は止められていた時間を
    // 除く（End - Start = Elapsed + Suspended。r995 のジョブ 498、656 で確かめた）が、再開後のキャッシュの冷えと、
    // プログラムが自分で測る経過時間（止められていた間を含む）が残るので、止められていれば当てにならない
    match record.suspended {
        None => {
            warnings.push(format!(
                "WARN: ジョブ {job} の sacct の記録に Suspended が無く、止められていたかを確かめられなかった"
            ));
            line.push_str(
                " suspended=unknown: 止められたかを確かめられないので時間は当てにならない",
            );
        }
        Some(0) => {}
        Some(_) => line.push_str(" suspended: 止められていたので時間は当てにならない"),
    }
    let marked = line != plain;
    line.push_str(&format!(" state={}", record.state.name));
    // 目的は利用者の書く文なので、末尾に置く
    line.push_str(&format!(
        " class={} ref={} purpose={}",
        labels.class, labels.reference, labels.purpose
    ));
    Timing {
        line,
        marked,
        warnings,
    }
}

fn append_line(path: &str, line: &str) -> std::io::Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    file.write_all(format!("{line}\n").as_bytes())
}

/// 終了コード。終わるまで待つ形の投げ方は、取り消したジョブでも 0 を返しうる。成功以外（取り消し、時間切れ、
/// 確かめられないものなど）で終わったジョブは失敗として扱い、&& の次を実行しない
fn final_status(status: i32, state: &JobState) -> i32 {
    if !state.completed && status == 0 {
        1
    } else {
        status
    }
}

/// & と wait で同時に待った複数のジョブは、EXIT: の行がどのジョブのものか分からないので、1 行で番号と名前を添える
fn result_line(job: Option<&str>, name: Option<&str>, exit: i32, state: &str) -> String {
    format!(
        "RESULT: job={} name={} exit={exit} state={state}",
        job.unwrap_or("-"),
        name.unwrap_or("-")
    )
}

fn exit_line(status: i32, state: &JobState) -> String {
    if state.completed {
        format!("EXIT: {status}")
    } else {
        format!("EXIT: {status} (State={})", state.name)
    }
}

/// path の末尾の、n 行を含むだけのバイト（大きなログを全部読まないよう、後ろから塊で読む）。読む間にログが縮んだら、
/// 読めた所までを返す
fn file_tail(path: &str, n: usize) -> std::io::Result<Vec<u8>> {
    use std::io::{Seek, SeekFrom};
    const CHUNK: u64 = 64 * 1024;
    let mut file = std::fs::File::open(path)?;
    let mut end = file.seek(SeekFrom::End(0))?;
    let mut chunks = std::collections::VecDeque::new();
    let mut newlines = 0;
    // 末尾の改行の後は行に数えないので、改行が n 個を超えれば n 行がそろう
    while end > 0 && newlines <= n {
        let start = end.saturating_sub(CHUNK);
        let mut chunk = Vec::new();
        file.seek(SeekFrom::Start(start))?;
        (&mut file).take(end - start).read_to_end(&mut chunk)?;
        newlines += chunk.iter().filter(|b| **b == b'\n').count();
        chunks.push_front(chunk);
        end = start;
    }
    Ok(chunks.into_iter().flatten().collect())
}

/// text の最後の n 行（n は 1 以上。tail -n と同じく、末尾の改行の後は行に数えない）
fn tail_lines(text: &[u8], n: usize) -> &[u8] {
    let body = text.strip_suffix(b"\n").unwrap_or(text);
    let start = body
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, b)| **b == b'\n')
        .nth(n - 1)
        .map_or(0, |(i, _)| i + 1);
    &text[start..]
}

#[cfg(test)]
mod tests {
    use super::{
        JobRecord, JobState, Limit, Refusal, Request, RunningJob, duration, exit_line, file_tail,
        final_status, limit_text, local_path, parse_args, reorder, result_line, same_worktree_job,
        tail_lines, time_line, under, wait_record,
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
            name: "xlc-2G".to_string(),
            log: String::new(),
            exit: Some(0),
        }
    }

    #[test]
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
    fn the_time_line_has_the_job_elapsed_estimate_state_class_reference_and_purpose() {
        let req = request(&["--timed", "-t", "30", "xlc-bench", "true"]);
        let timing = time_line(
            "42",
            &record(Some(true), Some(0), completed()),
            &req.labels(),
        );
        assert_eq!(
            timing.line,
            "TIME: job=42 elapsed=00:12:34 estimate=00:30:00 state=COMPLETED class=trial ref=#1 purpose=試験の目的"
        );
        assert!(!timing.marked);
        assert!(timing.warnings.is_empty());
    }

    #[test]
    fn a_job_that_ran_interruptible_is_marked_preemptible_whatever_the_flag() {
        let req = request(&["--timed", "-t", "30", "xlc-bench", "true"]);
        let timing = time_line(
            "42",
            &record(Some(false), Some(0), completed()),
            &req.labels(),
        );
        assert!(
            timing.line.contains(" estimate=00:30:00 preemptible: 割り込まれうるので時間は記録に使わない（記録するなら排他の --timed） state="),
            "{}",
            timing.line
        );
        assert!(timing.marked);
        let main = request(&["-t", "30", "xlc-2G", "true"]);
        assert!(
            !time_line(
                "42",
                &record(Some(true), Some(0), completed()),
                &main.labels()
            )
            .line
            .contains("preemptible")
        );
    }

    #[test]
    fn shared_suspended_and_preemptible_marks_add_up_in_order() {
        let req = request(&["-c", "8", "-t", "30", "openmc-2G-shared", "true"]);
        let timing = time_line(
            "42",
            &record(Some(false), Some(600), completed()),
            &req.labels(),
        );
        assert!(
            timing.line.starts_with("TIME: job=42 elapsed=00:12:34 estimate=00:30:00 shared (-c 8): 時間は当てにならない preemptible: 割り込まれうるので時間は記録に使わない（記録するなら排他の --timed） suspended: 止められていたので時間は当てにならない state=COMPLETED"),
            "{}",
            timing.line
        );
        assert!(timing.marked);
        let timed = request(&["--timed", "-t", "30", "xlc-bench", "true"]);
        let suspended = time_line(
            "42",
            &record(Some(true), Some(1), completed()),
            &timed.labels(),
        );
        assert!(
            suspended
                .line
                .contains(" suspended: 止められていたので時間は当てにならない")
        );
        assert!(suspended.marked);
    }

    #[test]
    fn a_record_without_the_partition_or_the_suspended_time_is_marked_unknown_with_warnings() {
        let req = request(&["-t", "30", "xlc-2G", "true"]);
        let timing = time_line("42", &record(None, None, completed()), &req.labels());
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
    fn a_record_without_times_leaves_them_empty() {
        let req = request(&["-t", "30", "xlc-2G", "true"]);
        let mut bare = record(Some(true), Some(0), state("CANCELLED", true, false));
        bare.elapsed = None;
        bare.limit = None;
        assert!(
            time_line("42", &bare, &req.labels())
                .line
                .starts_with("TIME: job=42 elapsed= estimate= state=CANCELLED class=")
        );
    }

    #[test]
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
    fn a_job_that_did_not_complete_fails_even_when_submission_returned_zero() {
        assert_eq!(final_status(0, &completed()), 0);
        assert_eq!(final_status(0, &state("CANCELLED", true, false)), 1);
        assert_eq!(final_status(0, &JobState::unknown()), 1);
        assert_eq!(final_status(3, &state("FAILED", true, false)), 3);
        assert_eq!(final_status(3, &JobState::unknown()), 3);
    }

    #[test]
    fn the_exit_line_adds_the_state_unless_completed_and_the_result_line_names_the_job() {
        assert_eq!(exit_line(0, &completed()), "EXIT: 0");
        assert_eq!(
            exit_line(1, &state("CANCELLED", true, false)),
            "EXIT: 1 (State=CANCELLED)"
        );
        assert_eq!(
            exit_line(1, &JobState::unknown()),
            "EXIT: 1 (State=unknown)"
        );
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
    fn a_queue_already_in_class_order_is_not_touched() {
        assert_eq!(reorder_with(&[Some("")]), (None, vec![]));
    }

    #[test]
    fn the_queue_is_read_again_after_each_top_until_no_reordering_is_needed() {
        // 並べ直しが拒まれても、同時に投げたジョブが列を上書きしても、読み直して並べ直す
        assert_eq!(
            reorder_with(&[Some("21,42,20\n"), Some("21,42,23\n"), Some("")]),
            (None, vec!["21,42,20".to_string(), "21,42,23".to_string()])
        );
    }

    #[test]
    fn after_three_tops_the_order_is_left_to_the_queue_with_a_warning() {
        let (warning, tops) = reorder_with(&[Some("42,20"); 5]);
        assert_eq!(tops.len(), 3);
        assert_eq!(
            warning.as_deref(),
            Some("WARN: 3 回並べ直しても区分の順（42,20）にならなかった。投げた順に走る")
        );
    }

    #[test]
    fn an_unreadable_queue_warns_and_does_not_reorder() {
        let (warning, tops) = reorder_with(&[None]);
        assert!(tops.is_empty());
        assert_eq!(
            warning.as_deref(),
            Some("WARN: 待ち行列を読めず、区分の順に並べ直せなかった。投げた順に走る")
        );
    }

    #[test]
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
    fn the_tail_is_the_last_lines_like_tail_n() {
        assert_eq!(tail_lines(b"a\nb\nc\n", 2), b"b\nc\n");
        assert_eq!(tail_lines(b"a\nb\nc", 1), b"c");
        assert_eq!(tail_lines(b"a\nb\n", 20), b"a\nb\n");
        assert_eq!(tail_lines(b"", 20), b"");
        assert_eq!(tail_lines(b"\n\n", 1), b"\n");
    }

    #[test]
    fn the_file_tail_reads_back_from_the_end_until_it_has_the_lines() {
        let path = std::env::temp_dir().join(format!("slurm-run-tail-{}", std::process::id()));
        // 64 KiB の塊を何度もまたぐ長さ
        let text: String = (0..30000).map(|i| format!("line {i}\n")).collect();
        std::fs::write(&path, &text).unwrap();
        let tail = file_tail(path.to_str().unwrap(), 20).unwrap();
        assert!(tail.len() < text.len());
        assert_eq!(tail_lines(&tail, 20), tail_lines(text.as_bytes(), 20));
        std::fs::write(&path, "a\nb").unwrap();
        assert_eq!(file_tail(path.to_str().unwrap(), 20).unwrap(), b"a\nb");
        let _ = std::fs::remove_file(&path);
    }
}
