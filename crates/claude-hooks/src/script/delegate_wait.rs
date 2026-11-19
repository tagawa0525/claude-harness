//! /delegate の担当が待つか、待たずに報告して終えるかの線（ADR-0078 の 2026-11-20 の変更）。
//!
//! 線は、投げる前の見込み（[`outlook`]。hook の guard-delegate-wait が使う）と、実際の待ち（wait-step）の両方で同じ
//! [`LIMIT_SECS`]。担当は待つ間 4 分 30 秒ごとに答えてキャッシュを保ち、1 時間で新しい担当の立ち上げに近い費用になるので、
//! その手前で切る（ADR-0078 の決定と理由の 1 項目）。

use super::slurm::{Phase, QueuedJob};

/// 担当が待ってよい長さ（秒）
pub const LIMIT_SECS: u64 = 55 * 60;

/// 投げるジョブの終わりの見込み
#[derive(Debug, PartialEq)]
pub enum Outlook {
    /// 分かる。時刻は UNIX の秒
    Known {
        now: i64,
        /// 待ち行列の最も遅い終わり（空か、どれも今より前に終わるなら今）
        queue_end: i64,
        /// queue_end のジョブ（待ち行列が空か、どれも今より前に終わるなら None）
        last_job: Option<String>,
        /// 投げるジョブの見積もりの合計（分）
        estimate_minutes: u64,
        /// queue_end に見積もりを足した、投げるジョブの終わりの見込み
        finish: i64,
    },
    /// 分からない（理由）
    Unknown(String),
}

impl Outlook {
    /// 終わりの見込みが、今から線の内か（ちょうど線の上も内）
    pub fn within_limit(&self) -> bool {
        match self {
            Outlook::Known { now, finish, .. } => finish - now <= LIMIT_SECS as i64,
            Outlook::Unknown(_) => false,
        }
    }
}

/// 待ち行列 queue の後に、見積もりの合計 estimate_minutes 分のジョブを投げたときの終わりの見込み。
///
/// 待ち行列のジョブの終わりは、開始の見込み（走り始めたものは走り始めた時刻）と見積もりの和で、その最も遅いものの後に
/// 投げるジョブが走るとみなす（区分の順で前に並ぶことと、排他でないジョブが並んで走ることは数えない。短く見積もらない）。
/// 待っているジョブの開始の見込みが過ぎていれば、今から走るとみなす。見込みが分からないのは、開始の見込みの無いジョブ、
/// 見積もりの無いジョブ（無制限、パーティションの上限に任せたもの）、止められたジョブ（止められていた間だけ終わりが延びる）、
/// 見積もりを超えて走っているジョブ（終わりが読めない）があるとき
pub fn outlook(now: i64, queue: &[QueuedJob], estimate_minutes: u64) -> Outlook {
    let mut queue_end = now;
    let mut last_job = None;
    for job in queue {
        let Some(start) = job.start_at else {
            return Outlook::Unknown(format!(
                "待ち行列のジョブ {} に開始の見込みが無い（{}）",
                job.id, job.start
            ));
        };
        let Some(limit) = job.limit_minutes else {
            return Outlook::Unknown(format!(
                "待ち行列のジョブ {} の見積もりが無い（{}）",
                job.id, job.limit
            ));
        };
        if job.phase == Phase::Suspended {
            return Outlook::Unknown(format!(
                "ジョブ {} が止められている（再開の時刻が読めない）",
                job.id
            ));
        }
        let span = (limit as i64).saturating_mul(60);
        let end = if job.phase == Phase::Pending {
            start.max(now).saturating_add(span)
        } else {
            let end = start.saturating_add(span);
            if end < now {
                return Outlook::Unknown(format!(
                    "走っているジョブ {} が見積もり（{}）を超えて走っている（終わりが読めない）",
                    job.id, job.limit
                ));
            }
            end
        };
        if end > queue_end {
            queue_end = end;
            last_job = Some(job.id.clone());
        }
    }
    Outlook::Known {
        now,
        queue_end,
        last_job,
        estimate_minutes,
        finish: queue_end.saturating_add((estimate_minutes as i64).saturating_mul(60)),
    }
}

/// 見込みの値と内訳の 1 文
pub fn describe(outlook: &Outlook) -> String {
    match outlook {
        Outlook::Known {
            now,
            queue_end,
            last_job,
            estimate_minutes,
            finish,
        } => {
            let queue = match last_job {
                Some(id) => format!(
                    "待ち行列の最も遅い終わり {}（ジョブ {id}。今から {} 分）",
                    clock(*queue_end),
                    minutes_from(*now, *queue_end)
                ),
                None => "待ち行列の終わり 今（待っているジョブが無い）".to_string(),
            };
            format!(
                "終わりの見込みは {}（今 {} から {} 分）= {queue} + 見積もり（-t）の合計 {estimate_minutes} 分。線は {} 分",
                clock(*finish),
                clock(*now),
                minutes_from(*now, *finish),
                LIMIT_SECS / 60
            )
        }
        Outlook::Unknown(reason) => format!("終わりの見込みが分からない: {reason}"),
    }
}

/// 今から then までの分（切り上げ。線を超えた分を 55 分と書かない）
fn minutes_from(now: i64, then: i64) -> i64 {
    ((then - now).max(0) as u64).div_ceil(60) as i64
}

/// 今の UNIX の秒
pub fn now() -> i64 {
    super::time::now_micros() / 1_000_000
}

/// 時刻を JST の %FT%T で（squeue の一覧と同じ書き方。slurm の開始の時刻の書き方と同じ関数）
fn clock(secs: i64) -> String {
    super::slurm::clock_text(secs).unwrap_or_else(|| secs.to_string())
}

#[cfg(test)]
mod tests {
    use super::{LIMIT_SECS, Outlook, describe, outlook};
    use crate::script::slurm::{Partition, Phase, QueuedJob};

    /// 2026-11-20T00:00:00 JST
    const NOW: i64 = 1_795_100_400;

    fn job(id: &str, phase: Phase, start_at: Option<i64>, limit_minutes: Option<u64>) -> QueuedJob {
        QueuedJob {
            id: id.to_string(),
            group: id.parse().unwrap(),
            array: false,
            partition: "main".to_string(),
            partition_kind: Partition::Default,
            name: format!("j{id}"),
            state: format!("{phase:?}"),
            phase,
            priority: 10000,
            limit: "limit".to_string(),
            start: "start".to_string(),
            comment: String::new(),
            start_at,
            limit_minutes,
        }
    }

    fn finish(outlook: &Outlook) -> i64 {
        match outlook {
            Outlook::Known { finish, .. } => *finish,
            Outlook::Unknown(reason) => panic!("見込みが分からない: {reason}"),
        }
    }

    #[test]
    fn an_empty_queue_ends_now_so_the_job_ends_after_its_estimate() {
        let o = outlook(NOW, &[], 30);
        assert_eq!(finish(&o), NOW + 30 * 60);
        assert!(o.within_limit());
    }

    #[test]
    fn a_running_job_ends_at_its_start_plus_its_estimate() {
        // 10 分前に走り始めた 60 分のジョブは 50 分後に終わり、その後に 5 分のジョブが走る
        let o = outlook(
            NOW,
            &[job("1", Phase::Started, Some(NOW - 600), Some(60))],
            5,
        );
        assert_eq!(finish(&o), NOW + 55 * 60);
        assert!(o.within_limit());
    }

    #[test]
    fn the_latest_end_of_running_and_pending_jobs_comes_before_the_job() {
        let queue = [
            job("1", Phase::Started, Some(NOW - 600), Some(30)),
            job("2", Phase::Pending, Some(NOW + 20 * 60), Some(40)),
            job("3", Phase::Pending, Some(NOW + 10 * 60), Some(10)),
        ];
        let o = outlook(NOW, &queue, 1);
        assert_eq!(finish(&o), NOW + 61 * 60);
        assert!(!o.within_limit());
        match o {
            Outlook::Known { last_job, .. } => assert_eq!(last_job.as_deref(), Some("2")),
            Outlook::Unknown(_) => unreachable!(),
        }
    }

    #[test]
    fn a_pending_job_whose_start_estimate_passed_starts_now() {
        let o = outlook(
            NOW,
            &[job("2", Phase::Pending, Some(NOW - 600), Some(20))],
            10,
        );
        assert_eq!(finish(&o), NOW + 30 * 60);
    }

    #[test]
    fn a_job_without_a_start_estimate_makes_the_outlook_unknown() {
        let o = outlook(NOW, &[job("2", Phase::Pending, None, Some(20))], 10);
        assert!(
            matches!(&o, Outlook::Unknown(r) if r.contains("ジョブ 2") && r.contains("開始の見込み"))
        );
        assert!(!o.within_limit());
    }

    #[test]
    fn an_unlimited_or_unset_estimate_makes_the_outlook_unknown() {
        let o = outlook(NOW, &[job("1", Phase::Started, Some(NOW - 60), None)], 10);
        assert!(
            matches!(&o, Outlook::Unknown(r) if r.contains("ジョブ 1") && r.contains("見積もりが無い"))
        );
        assert!(!o.within_limit());
    }

    #[test]
    fn a_job_running_past_its_estimate_makes_the_outlook_unknown() {
        let o = outlook(
            NOW,
            &[job("1", Phase::Started, Some(NOW - 3600), Some(30))],
            1,
        );
        assert!(matches!(&o, Outlook::Unknown(r) if r.contains("超えて")));
    }

    #[test]
    fn a_suspended_job_makes_the_outlook_unknown() {
        // 止められていた間だけ終わりが延びるので、見積もりの内でも終わりは読めない
        let o = outlook(
            NOW,
            &[job("1", Phase::Suspended, Some(NOW - 600), Some(60))],
            1,
        );
        assert!(matches!(&o, Outlook::Unknown(r) if r.contains("止められている")));
    }

    #[test]
    fn exactly_55_minutes_is_within_the_limit_and_one_second_more_is_not() {
        let at = |secs: i64| {
            outlook(
                NOW,
                &[job("1", Phase::Started, Some(NOW - 3300 + secs), Some(60))],
                50,
            )
        };
        // 走っているジョブの残りが 5 分（+ secs 秒）、見積もり 50 分
        assert_eq!(finish(&at(0)) - NOW, LIMIT_SECS as i64);
        assert!(at(0).within_limit());
        assert!(at(-1).within_limit());
        assert!(!at(1).within_limit());
    }

    #[test]
    fn the_description_shows_the_finish_and_its_breakdown() {
        let o = outlook(
            NOW,
            &[job("962", Phase::Started, Some(NOW - 600), Some(70))],
            30,
        );
        assert_eq!(
            describe(&o),
            "終わりの見込みは 2026-11-20T01:30:00（今 2026-11-20T00:00:00 から 90 分）= 待ち行列の最も遅い終わり 2026-11-20T01:00:00（ジョブ 962。今から 60 分） + 見積もり（-t）の合計 30 分。線は 55 分"
        );
        assert_eq!(
            describe(&outlook(NOW, &[], 5)),
            "終わりの見込みは 2026-11-20T00:05:00（今 2026-11-20T00:00:00 から 5 分）= 待ち行列の終わり 今（待っているジョブが無い） + 見積もり（-t）の合計 5 分。線は 55 分"
        );
        assert_eq!(
            describe(&Outlook::Unknown("x".to_string())),
            "終わりの見込みが分からない: x"
        );
    }
}
