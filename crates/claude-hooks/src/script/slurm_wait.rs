//! `slurm-wait.sh <番号>`: 既にある Slurm のジョブの終わりを待ち、slurm-run.sh の終わりと同じ行を出す（ADR-0078 の
//! 2026-11-20 の変更。`~/.claude/scripts/slurm-wait.sh` が呼ぶ）。
//!
//! /delegate の担当が待ちの線を超えて終えた後に、担当が投げたジョブを親が番号から引き継いで待つ。出力の行と終了コードの意味は
//! `claude/scripts/slurm-wait.sh` の先頭に書く。
//!
//! 流れ:
//!
//! 1. 番号を確かめる。番号でなければ、理由を標準エラーに出して 1 で終える
//! 2. 制御のデーモンが覚えているジョブを SLURM_WAIT_INTERVAL 秒（既定 30）ごとに読み、終わるか覚えていなくなるまで待つ
//!    （sbatch --wait の `_job_wait` と同じ見方。終わりの終了コードもそれと同じ形）。初めに名前とログとコメントを読み、
//!    `JOB:`、`LOG:` を出す。制御のデーモンが初めから覚えていなければ、会計の記録から名前とログを読む（どちらにも無ければ 1）
//! 3. 終わりの処理（会計の記録の待ち、`TIME:` の行、ログの末尾、`RESULT:`、`EXIT:`）は slurm-run と同じ [`slurm_run::finish`]。
//!    `TIME:` の行の区分、参照、目的、-c のコア数は、slurm-run がジョブのコメントに入れたものを読む（[`labels`]）

use super::slurm;
use super::slurm_run::{self, Labels, Progress, lock};
use std::sync::{Arc, Mutex};

const USAGE: &str = "Usage: slurm-wait.sh <ジョブの番号>";

/// 名前とコメントを読めなかったときの欄
const UNKNOWN: &str = "-";

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

fn run(args: &[String], progress: &Mutex<Progress>) -> i32 {
    let [job] = args else {
        eprintln!("{USAGE}");
        return 1;
    };
    if job.is_empty() || !job.bytes().all(|b| b.is_ascii_digit()) {
        eprintln!("ERROR: ジョブの番号は数: '{job}'\n{USAGE}");
        return 1;
    }
    lock(progress).job = Some(job.clone());
    let (record_wait, interval) = match (slurm_run::record_wait(), poll_interval()) {
        (Ok(record_wait), Ok(interval)) => (record_wait, interval),
        (Err(e), _) | (_, Err(e)) => {
            eprintln!("ERROR: {e}");
            return 1;
        }
    };

    let mut origin: Option<(String, String, String)> = None;
    let status = loop {
        let Some(found) = slurm::controller_job(job) else {
            // 覚えていない（終わってから MinJobAge を過ぎた）。sbatch --wait は終了コードを読めずに 0 を返すが、ここは
            // 会計の記録の終了コードを使う（終わりの State で、成功以外は finish が 1 にする）
            break None;
        };
        if origin.is_none() {
            let first = (found.name.clone(), found.log.clone(), found.comment.clone());
            announce(job, &first.0, &first.1, progress);
            origin = Some(first);
        }
        if found.state.ended {
            break Some(found.exit.unwrap_or(0));
        }
        std::thread::sleep(interval);
    };

    let (_, log, comment) = match origin {
        Some(origin) => origin,
        None => {
            let Some(record) = slurm::accounting_record(job) else {
                eprintln!(
                    "ERROR: ジョブ {job} が制御のデーモンにも会計の記録にも無い（番号の誤りか、記録がまだ届いていない）"
                );
                return 1;
            };
            announce(job, &record.name, &record.log, progress);
            eprintln!(
                "WARN: ジョブ {job} は終わってから時間がたち、投げたときのコメントを読めない。TIME: の行の区分、参照、目的は {UNKNOWN}"
            );
            (record.name, record.log, String::new())
        }
    };
    let status = status
        .or_else(|| slurm::accounting_record(job).and_then(|r| r.exit))
        .unwrap_or(0);
    let labels = labels(&comment);
    slurm_run::finish(
        job,
        &local_log(&log),
        &labels,
        status,
        record_wait,
        progress,
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
