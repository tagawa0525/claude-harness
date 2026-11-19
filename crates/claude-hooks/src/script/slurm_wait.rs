//! `slurm-wait.sh <番号>...`: 既にある Slurm のジョブの終わりを待ち、slurm-run.sh の終わりと同じ行を出す（ADR-0078 の
//! 2026-11-20 の変更。`~/.claude/scripts/slurm-wait.sh` が呼ぶ）。
//!
//! /delegate の担当が待ちの線を超えて終えた後に、担当が投げたジョブを親が番号から引き継いで待つ。番号を複数受け、すべてが
//! 終わってから 1 度だけ終わる（親の background の終わりの通知を、ジョブごとでなく 1 回にする）。出力の行と終了コードの意味は
//! `claude/scripts/slurm-wait.sh` の先頭に書く。
//!
//! 流れ:
//!
//! 1. 番号を確かめる。番号でなければ、理由を標準エラーに出して 1 で終える
//! 2. 制御のデーモンが覚えているジョブを SLURM_WAIT_INTERVAL 秒（既定 30）ごとに読み、どれもが終わるか覚えていなくなるまで
//!    待つ（sbatch --wait の `_job_wait` と同じ見方。読めないときは読み直し、終わりの終了コードもそれと同じ形）。初めて読めた
//!    ジョブの名前とログとコメントを控え、`JOB:`、`LOG:` を出す
//! 3. 番号の順に、終わりの処理（会計の記録の待ち、`TIME:` の行、ログの末尾、`RESULT:`、`EXIT:`）を slurm-run と同じ
//!    [`slurm_run::finish`] で出す。制御のデーモンが初めから覚えていなかったジョブは、会計の記録から名前とログを読む（どちらにも
//!    無ければ、そのジョブは 1）。`TIME:` の行の区分、参照、目的、-c のコア数は、slurm-run がジョブのコメントに入れたものを
//!    読む（[`labels`]）。終了コードは、番号の順で最初の 0 でないもの

use super::slurm;
use super::slurm_run::{self, Labels, Progress, lock};
use std::sync::{Arc, Mutex};

const USAGE: &str = "Usage: slurm-wait.sh <ジョブの番号>...";

/// 名前とコメントを読めなかったときの欄
const UNKNOWN: &str = "-";

/// 待つジョブ 1 本
struct Waited {
    job: String,
    /// 制御のデーモンから読んだ（名前, ログ, コメント）
    origin: Option<(String, String, String)>,
    /// 終わったら Some（中は制御のデーモンの終了コード。覚えていなかったら None）
    ended: Option<Option<i32>>,
    progress: Arc<Mutex<Progress>>,
    /// 続けて読めなかった回数
    failures: u32,
}

/// 続けて読めない回数の上限（既定の間隔の 30 秒で 10 分。slurmctld の再起動を越えて待つ）
const MAX_FAILURES: u32 = 20;

pub fn main(args: &[String]) -> i32 {
    let jobs = match parse_args(args) {
        Ok(jobs) => jobs,
        Err(message) => {
            eprintln!("{message}");
            Progress::default().report_unfinished(1);
            return 1;
        }
    };
    let mut waited: Vec<Waited> = jobs
        .into_iter()
        .map(|job| Waited {
            progress: {
                let mut progress = Progress::default();
                progress.job = Some(job.clone());
                Arc::new(Mutex::new(progress))
            },
            job,
            origin: None,
            ended: None,
            failures: 0,
        })
        .collect();
    let shared: Vec<_> = waited.iter().map(|w| Arc::clone(&w.progress)).collect();
    if let Err(e) = crate::interrupt::cleanup_on_interrupt(move || {
        for progress in &shared {
            lock(progress).report_unfinished(crate::interrupt::INTERRUPTED);
        }
    }) {
        eprintln!("ERROR: 中断のときの後片付けを登録できない（RESULT: の行を出せない）: {e}");
        for w in &waited {
            lock(&w.progress).report_unfinished(1);
        }
        return 1;
    }
    let code = run(&mut waited);
    for w in &waited {
        lock(&w.progress).report_unfinished(code);
    }
    code
}

/// 番号（重ねたものは 1 つに。先頭の 0 は外す）を数の順に。番号でないものがあれば使い方
fn parse_args(args: &[String]) -> Result<Vec<String>, String> {
    if args.is_empty() {
        return Err(USAGE.to_string());
    }
    if let Some(bad) = args
        .iter()
        .find(|a| a.is_empty() || !a.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err(format!("ERROR: ジョブの番号は数: '{bad}'\n{USAGE}"));
    }
    // scontrol と sacct の job_id は数で、`0962` の文字列とは一致しないので、数に直す
    let mut jobs: Vec<u64> = args
        .iter()
        .map(|a| {
            a.parse()
                .map_err(|_| format!("ERROR: ジョブの番号が大きすぎる: '{a}'\n{USAGE}"))
        })
        .collect::<Result<_, _>>()?;
    jobs.sort_unstable();
    jobs.dedup();
    Ok(jobs.into_iter().map(|j| j.to_string()).collect())
}

/// 待つ間隔（SLURM_WAIT_INTERVAL。テストが短くする）
fn poll_interval() -> Result<std::time::Duration, String> {
    match std::env::var("SLURM_WAIT_INTERVAL")
        .ok()
        .filter(|v| !v.is_empty())
    {
        None => Ok(std::time::Duration::from_secs(30)),
        Some(v) => v
            .parse::<f64>()
            .ok()
            .filter(|s| s.is_finite() && *s >= 0.0)
            .map(std::time::Duration::from_secs_f64)
            .ok_or_else(|| format!("SLURM_WAIT_INTERVAL は秒: '{v}'")),
    }
}

fn run(waited: &mut [Waited]) -> i32 {
    let (record_wait, interval) = match (slurm_run::record_wait(), poll_interval()) {
        (Ok(record_wait), Ok(interval)) => (record_wait, interval),
        (Err(e), _) | (_, Err(e)) => {
            eprintln!("ERROR: {e}");
            return 1;
        }
    };

    while waited.iter().any(|w| w.ended.is_none()) {
        for w in waited.iter_mut().filter(|w| w.ended.is_none()) {
            match slurm::controller_job(&w.job) {
                Ok(Some(found)) => {
                    w.failures = 0;
                    if w.origin.is_none() {
                        announce(&w.job, &found.name, &found.log, &w.progress);
                        w.origin = Some((found.name, found.log, found.comment));
                    }
                    if found.state.ended {
                        w.ended = Some(Some(found.exit.unwrap_or(0)));
                    }
                }
                // 覚えていない（終わってから MinJobAge を過ぎた）。sbatch --wait は終了コードを読めずに 0 を返すが、ここは
                // 会計の記録の終了コードを使う（終わりの State で、成功以外は finish が 1 にする）
                Ok(None) => w.ended = Some(None),
                // 一時の失敗は、sbatch --wait と同じく読み直す（覚えていないと読むと、走っているジョブを終わったことにする）
                Err(slurm::ControllerError::Failed(reason)) => {
                    w.failures += 1;
                    // 続けて読めないなら、一時の不調でないとみなして終える（終わらない background を残さない）
                    if w.failures >= MAX_FAILURES {
                        eprintln!(
                            "ERROR: ジョブ {} の状態を {MAX_FAILURES} 回続けて読めない: {reason}",
                            w.job
                        );
                        return 1;
                    }
                    eprintln!(
                        "WARN: ジョブ {} の状態を読めない。読み直す: {reason}",
                        w.job
                    );
                }
                // 起動できないものは読み直しても変わらないので、待たずに失敗する（終わらない background を残さない）
                Err(slurm::ControllerError::Spawn(reason)) => {
                    eprintln!("ERROR: {reason}");
                    return 1;
                }
            }
        }
        if waited.iter().any(|w| w.ended.is_none()) {
            std::thread::sleep(interval);
        }
    }

    let mut code = 0;
    for w in waited.iter_mut() {
        let status = finish_one(w, record_wait);
        if code == 0 {
            code = status;
        }
    }
    code
}

/// 終わったジョブ 1 本の終わりの行を出し、そのジョブの終了コードを返す
fn finish_one(w: &mut Waited, record_wait: (u32, std::time::Duration)) -> i32 {
    let job = w.job.as_str();
    // 会計の記録は、要るときに 1 度だけ読む（終わりの State と時間は、finish が届くまで読み直す）
    let mut record = None;
    let (log, comment) = match w.origin.take() {
        Some((_, log, comment)) => (log, comment),
        None => {
            let Some(found) = slurm::accounting_record(job) else {
                eprintln!(
                    "ERROR: ジョブ {job} が制御のデーモンにも会計の記録にも無い（番号の誤りか、記録がまだ届いていない）"
                );
                lock(&w.progress).report_unfinished(1);
                return 1;
            };
            announce(job, &found.name, &found.log, &w.progress);
            eprintln!(
                "WARN: ジョブ {job} は終わってから時間がたち、投げたときのコメントを読めない。TIME: の行の区分、参照、目的は {UNKNOWN}"
            );
            let log = found.log.clone();
            record = Some(found);
            (log, String::new())
        }
    };
    let status = w
        .ended
        .flatten()
        .or_else(|| {
            record
                .or_else(|| slurm::accounting_record(job))
                .and_then(|r| r.exit)
        })
        .unwrap_or(0);
    slurm_run::finish(
        job,
        &local_log(&log),
        &labels(&comment),
        status,
        record_wait,
        &w.progress,
    )
}

/// JOB: と LOG: の行を出し、RESULT: の行の名前を控える
fn announce(job: &str, name: &str, log: &str, progress: &Mutex<Progress>) {
    let name = if name.is_empty() { UNKNOWN } else { name };
    lock(progress).name = Some(name.to_string());
    println!("JOB: {job}");
    println!("LOG: {}", local_log(log));
}

/// ログのパスを、このホストの書き方に直す（slurm-run の LOG: と同じパス）
fn local_log(log: &str) -> String {
    let (remote_root, nfs_entry) = slurm_run::roots();
    slurm_run::local_path(
        log,
        slurm_run::hostname() == "r995",
        &nfs_entry,
        &remote_root,
    )
}

/// slurm-run がジョブのコメントに入れた `<区分> | <参照> | <目的> | <連絡先>` の、TIME: の行の欄。連絡先の
/// `shared-cpus=<数>` は -c のコア数。形が違えば、読めない欄は `-`
fn labels(comment: &str) -> Labels<'_> {
    let mut parts = comment.splitn(4, " | ");
    let mut field = || parts.next().filter(|p| !p.is_empty()).unwrap_or(UNKNOWN);
    let (class, reference, purpose) = (field(), field(), field());
    let contact = parts.next().unwrap_or("");
    Labels {
        class,
        reference,
        purpose,
        cpus: contact
            .split(' ')
            .find_map(|word| word.strip_prefix("shared-cpus=")),
    }
}

#[cfg(test)]
mod tests {
    use super::{Labels, labels};

    #[test]
    fn the_comment_gives_the_class_reference_purpose_and_shared_cpus() {
        assert_eq!(
            labels(
                "trial | #1 | 試験の目的 | feat/x /home/u/github/x commit=abc shared-cpus=8 claude=s"
            ),
            Labels {
                class: "trial",
                reference: "#1",
                purpose: "試験の目的",
                cpus: Some("8"),
            }
        );
        assert_eq!(
            labels("decision | ADR-1,#2 | p | /home/u/github/x").cpus,
            None
        );
    }

    #[test]
    fn a_comment_not_from_slurm_run_leaves_dashes() {
        assert_eq!(
            labels(""),
            Labels {
                class: "-",
                reference: "-",
                purpose: "-",
                cpus: None,
            }
        );
        assert_eq!(labels("free text").reference, "-");
    }
}
