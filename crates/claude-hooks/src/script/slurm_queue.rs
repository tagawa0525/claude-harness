//! `slurm-queue.sh [--order [--first <番号>]]`: Slurm の待ち行列を、slurm-run.sh がコメントに入れた区分と目的つきで、
//! 走る順に並べる（ADR-0101。`~/.claude/scripts/slurm-queue.sh` が呼ぶ）。
//!
//! - 引数なし: 一覧
//! - `--order [--first <番号>]`: main を区分の順に並べる番号の列（slurm-run.sh が投げた後に `scontrol top` に渡す）
//!
//! 一覧（ジョブごとに 3 行。走る順）:
//!
//! ```text
//! <番号> <状態>[（動かせない）] <パーティション> <優先度> <区分> <見積もり> <開始の見込み> <名前> <参照>
//!   目的: <結果が何を決めるか>
//!   連絡先: <ブランチ、投げた場所、commit= 投げた時点のコミット（未コミットの変更があれば -dirty）、claude= セッション、shared-cpus=>
//! NOTE: <一覧にある印（-dirty、区分 -、（動かせない）、SUSPENDED）の意味。当たる行があるときだけ>
//! ORDER: main: <番号>（<区分>）が <番号>（<区分>）の後ろ、…で待っている。scontrol top <番号>,<番号>,…[（区分の無い …）]
//! NEXT: <ORDER: のコマンドの実行の仕方。ORDER: を出したときだけ>
//! ```
//!
//! 走る順は、パーティション timed（PriorityTier が高い）、main の順で、その中は走っているもの（走り始めた CONFIGURING
//! なども）、止められたもの、待っているもの（優先度の高い順、同じなら番号の順）。
//!
//! 区分の順（[`slurm_class::CLASSES`]）で並べ直すのはパーティション main の待っているジョブだけ。timed のジョブは、
//! 時間の記録と、走っているジョブの終わりを待てない数分の計算で、投げた順に実行する（区分の順で並べると、急ぐ数分の計算が
//! 長い測定の後ろに回る）。動かせないジョブ（優先度が 1 以下の保留のものと、配列の親ジョブ。scontrol top が受け付けない）と
//! 区分の無いジョブ（slurm-run.sh の外から投げたもの。何を決めるかが分からない）は、比べず、列にも入れない。
//!
//! 並べる列は、区分のある動かせる待っているジョブをすべて、区分の順、同じ区分は今の順に並べたもの（--first のジョブは
//! 自分の区分の先頭）。scontrol top は渡したジョブをその順に先頭へ出すので、1 つの列で渡す（ジョブごとに分けると最後に
//! 出したジョブが先頭になり、1 本だけ出すと、その前にいた上の区分のジョブも追い越す）。列が今の順と同じで、区分の無い
//! 待っているジョブが列のジョブより前にいなければ、一覧は ORDER: を出さず、--order は何も出さない。列に入れないジョブは
//! 列のジョブの後ろに回るので、区分の無い待っているジョブを ORDER: の行の末尾に挙げる。
//!
//! コメントの形は slurm-run.sh が書く「<区分> | <参照> | <目的> | <連絡先>」。参照と目的は | を含まないので、
//! 最初の 3 つの区切りまでを分け、残りを連絡先にする。
//!
//! 待ち行列は [`slurm::queued_jobs`] が読む。squeue が失敗したら、その終了コードで終わる。squeue の出力を解けなければ
//! 1、使い方の誤りも 1。
//!
//! squeue の無いホスト（Slurm の入っていない Windows の PC など）では、ssh で r995 の同じスクリプトを呼び、出力と
//! 終了コードをそのまま返す（ssh も無ければ、squeue の無いことを 127 で返す）。待ち行列を読むだけなので、どのホストから
//! 読んでも同じ一覧になる。

use super::slurm::{self, Partition, Phase, QueueError, QueuedJob};
use super::slurm_class;
use super::wait_step::quote;
use std::process::Command;

const USAGE: &str = "Usage: slurm-queue.sh [--order [--first <番号>]]";

const NOTE_DIRTY: &str = "NOTE: commit= の末尾が -dirty のジョブは、コミットしていない変更のある版で走り、結果は hash のコミットから再現できない";
const NOTE_UNCLASSED_BEHIND: &str = "NOTE: 区分が - のジョブは slurm-run.sh の外から投げたもので、区分と目的が無く、並べ直されずに区分のあるジョブの後ろに回る";
const NOTE_UNCLASSED: &str =
    "NOTE: 区分が - のジョブは slurm-run.sh の外から投げたもので、区分と目的が無い";
const NOTE_UNMOVABLE: &str = "NOTE: （動かせない）は保留（優先度が 1 以下）か配列の親ジョブで、scontrol top で並べ直せず、並べ直したジョブの後ろに回る";
const NOTE_SUSPENDED: &str =
    "NOTE: SUSPENDED は timed のジョブに止められた main のジョブで、それが終わると続きから再開する";
const NEXT: &str = "NEXT: 上の scontrol top を、番号を分けずに 1 回で実行する（1 本ずつ分けると最後に出したジョブが先頭になる）。ほかのセッションのジョブを含んでいても実行してよい";

#[derive(Debug, PartialEq)]
enum Mode {
    List,
    /// first は、自分の区分の先頭に置くジョブの番号（無ければ空）
    Order {
        first: String,
    },
}

pub fn main(args: &[String]) -> i32 {
    let Some(mode) = parse_args(args) else {
        eprintln!("{USAGE}");
        return 1;
    };
    match slurm::queued_jobs() {
        Ok(queue) => {
            print!("{}", render(&queue, &mode));
            0
        }
        Err(QueueError::Spawn { code: 127, error }) => {
            from_r995(args).unwrap_or_else(|| report(QueueError::Spawn { code: 127, error }))
        }
        Err(e) => report(e),
    }
}

/// r995 の slurm-queue.sh を ssh で呼び、その終了コードを返す（出力はそのまま流す）。ssh を起動できなければ None。
/// BatchMode は、鍵の無いときに、スキルの読み込みでパスワードの入力を待って止まらないため。パスは ~ でなく $HOME で
/// 書く: Git for Windows の ssh（MSYS のプログラム）は、Windows のプロセスから起動されると引用符の無い引数を glob にかけ、
/// ~ を手元のホームに展開する（$HOME は展開しないので、r995 の側のシェルが展開する）
fn from_r995(args: &[String]) -> Option<i32> {
    let remote = std::iter::once("$HOME/.claude/scripts/slurm-queue.sh".to_string())
        .chain(args.iter().map(|a| quote(a)))
        .collect::<Vec<_>>()
        .join(" ");
    let status = Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=10",
            "r995",
            &remote,
        ])
        .status()
        .ok()?;
    Some(status.code().unwrap_or(1))
}

/// `--order [--first <first>]` と同じ、main を区分の順に並べる番号の列（要らなければ空）。slurm-run が投げた後に
/// `scontrol top` に渡す。待ち行列を読めなければ、slurm-queue.sh と同じ ERROR: を標準エラーに出して None
pub(super) fn order(first: &str) -> Option<String> {
    let queue = read_queue().ok()?;
    let mode = Mode::Order {
        first: first.to_string(),
    };
    Some(render(&queue, &mode))
}

/// 待ち行列のジョブ。読めなければ ERROR: を標準エラーに出し、終了コードを返す
fn read_queue() -> Result<Vec<QueuedJob>, i32> {
    slurm::queued_jobs().map_err(report)
}

/// 待ち行列を読めない理由を ERROR: で標準エラーに出し、終了コードを返す
fn report(e: QueueError) -> i32 {
    match e {
        QueueError::Spawn { code, error } => {
            eprintln!("squeue を起動できない: {error}");
            eprintln!("ERROR: squeue が失敗した（終了コード {code}）");
            code
        }
        QueueError::Failed(code) => {
            eprintln!("ERROR: squeue が失敗した（終了コード {code}）");
            code
        }
        QueueError::Unreadable(reason) => {
            eprintln!("ERROR: squeue の出力を読めない: {reason}");
            1
        }
    }
}

/// 引数を読む。使い方に合わなければ None
fn parse_args(args: &[String]) -> Option<Mode> {
    match args {
        [] => Some(Mode::List),
        [order, rest @ ..] if order == "--order" => match rest {
            [] => Some(Mode::Order {
                first: String::new(),
            }),
            [flag, first] if flag == "--first" => Some(Mode::Order {
                first: first.clone(),
            }),
            _ => None,
        },
        _ => None,
    }
}

/// 待ち行列のジョブから、標準出力に書く全文を作る
fn render(queue: &[QueuedJob], mode: &Mode) -> String {
    let (order, first) = match mode {
        Mode::List => (false, ""),
        Mode::Order { first } => (true, first.as_str()),
    };
    if queue.is_empty() {
        return if order {
            String::new()
        } else {
            "待ち行列にジョブが無い\n".to_string()
        };
    }
    let jobs = running_order(queue);
    let mut out = String::new();
    if !order {
        for job in &jobs {
            out.push_str(&job.listing());
        }
        for note in notes(&jobs) {
            out.push_str(note);
            out.push('\n');
        }
    }
    let plan = plan(&jobs, first);
    if plan.list == plan.now && !plan.unclassed_ahead {
        return out;
    }
    let list = plan.list.join(",");
    if order {
        out.push_str(&list);
        out.push('\n');
        return out;
    }
    let note = if plan.unclassed.is_empty() {
        String::new()
    } else {
        format!("（区分の無い {} は後ろに回る）", plan.unclassed.join(","))
    };
    out.push_str(&format!(
        "ORDER: main: {}で待っている。scontrol top {list}{note}\n",
        plan.behind.join("、")
    ));
    out.push_str(NEXT);
    out.push('\n');
    out
}

/// 待ち行列のジョブと、コメントを分けたもの
struct Job<'a> {
    queued: &'a QueuedJob,
    /// 区分とその順（コメントが区分で始まる 4 つ以上の欄でなければ None）
    class: Option<(&'a str, usize)>,
    reference: &'a str,
    purpose: &'a str,
    contact: &'a str,
}

impl<'a> Job<'a> {
    fn new(queued: &'a QueuedJob) -> Job<'a> {
        let comment = queued.comment.as_str();
        let parts: Vec<&str> = comment.split(" | ").collect();
        let classed = match parts.as_slice() {
            [class, reference, purpose, _, ..] => {
                slurm_class::rank(class).map(|rank| ((*class, rank), *reference, *purpose))
            }
            _ => None,
        };
        let (class, reference, purpose, contact) = match classed {
            Some((class, reference, purpose)) => (
                Some(class),
                reference,
                purpose,
                drop_field(drop_field(drop_field(comment))),
            ),
            None => (None, "-", "（無い）", comment),
        };
        Job {
            queued,
            class,
            reference,
            purpose,
            contact,
        }
    }

    fn id(&self) -> &'a str {
        &self.queued.id
    }

    fn pending(&self) -> bool {
        self.queued.phase == Phase::Pending
    }

    /// 既定のパーティション（main）で待っている
    fn pending_main(&self) -> bool {
        self.pending() && self.queued.partition_kind == Partition::Default
    }

    /// 走る順の鍵（パーティション、段階、優先度の高い順、番号の先頭の数）
    fn sort_key(&self) -> (u8, u8, std::cmp::Reverse<u64>, u64) {
        let job = self.queued;
        let partition = match job.partition_kind {
            Partition::Uninterrupted => 0,
            Partition::Default | Partition::Other => 1,
        };
        let phase = match job.phase {
            Phase::Started => 0,
            Phase::Suspended => 1,
            Phase::Pending => 2,
        };
        (partition, phase, std::cmp::Reverse(job.priority), job.group)
    }

    /// 優先度 1 以下は保留（保留のジョブの優先度は 0 か 1 で、scontrol top は優先度 1 以下のジョブを動かせない）。
    /// 配列のまとめて待っているものも動かせない
    fn unmovable(&self) -> bool {
        self.pending() && (self.queued.priority <= 1 || self.queued.array)
    }

    /// 一覧の 3 行
    fn listing(&self) -> String {
        let job = self.queued;
        format!(
            "{} {}{} {} {} {} {} {} {} {}\n  目的: {}\n  連絡先: {}\n",
            job.id,
            job.state,
            if self.unmovable() {
                "（動かせない）"
            } else {
                ""
            },
            job.partition,
            job.priority,
            self.class.map_or("-", |(class, _)| class),
            job.limit,
            job.start,
            job.name,
            self.reference,
            self.purpose,
            self.contact,
        )
    }

    fn dirty(&self) -> bool {
        self.contact.split(' ').any(|w| {
            w.len() >= "commit=-dirty".len() && w.starts_with("commit=") && w.ends_with("-dirty")
        })
    }
}

/// コメントの先頭の 1 欄と区切りの ` | ` を外す。最初の | の前後が空白でなければ（参照か目的が | を含む）外さない。
/// その | を区切りとみなすと、連絡先が欄の分け方によって変わり、slurm-run.sh の外から投げた形の崩れたコメントで
/// どこまでが連絡先かを決められないため、区切りとみなさずにコメントの残りをそのまま連絡先にする
fn drop_field(text: &str) -> &str {
    let bytes = text.as_bytes();
    match text.find('|') {
        Some(p) if p >= 1 && bytes[p - 1] == b' ' && bytes.get(p + 1) == Some(&b' ') => {
            &text[p + 2..]
        }
        _ => text,
    }
}

/// 番号が --first と同じジョブか。どちらも数なら数で比べる（`--first 042` は 42 のジョブ）。ほかは文字列で比べる
/// （配列のジョブ `12_[1-3]` は 12 と別）
fn same_job(id: &str, first: &str) -> bool {
    match (id.parse::<u64>(), first.parse::<u64>()) {
        (Ok(a), Ok(b)) => a == b,
        _ => id == first,
    }
}

/// 走る順に並べたジョブ。鍵が同じ（配列か異種のジョブの、同じ番号の部分）なら番号の文字の順で、ほかの番号を先頭に
/// 含む番号を先に置く（`12_10` を `12_1` より先）。番号だけで比べないのは、`squeue --format` の一覧の行（番号の後ろが
/// 欄の区切りの `|`）をバイトの順で並べたときと同じ順にするため（ADR-0101 は slurm-queue.sh の出力の行を変えない）
fn running_order(queue: &[QueuedJob]) -> Vec<Job<'_>> {
    let mut jobs: Vec<Job> = queue.iter().map(Job::new).collect();
    jobs.sort_by_cached_key(|job| (job.sort_key(), format!("{}|", job.id())));
    jobs
}

/// 一覧の印のうち、当たる行があるものの意味
fn notes(jobs: &[Job]) -> Vec<&'static str> {
    let mut notes = Vec::new();
    if jobs.iter().any(Job::dirty) {
        notes.push(NOTE_DIRTY);
    }
    let unclassed: Vec<&Job> = jobs.iter().filter(|j| j.class.is_none()).collect();
    // 並べ直されずに後ろに回るのは、待っている main のジョブだけ
    if unclassed.iter().any(|j| j.pending_main()) {
        notes.push(NOTE_UNCLASSED_BEHIND);
    } else if !unclassed.is_empty() {
        notes.push(NOTE_UNCLASSED);
    }
    if jobs.iter().any(Job::unmovable) {
        notes.push(NOTE_UNMOVABLE);
    }
    if jobs.iter().any(|j| j.queued.phase == Phase::Suspended) {
        notes.push(NOTE_SUSPENDED);
    }
    notes
}

/// main の待っているジョブの並べ直し
#[derive(Debug, PartialEq)]
struct Plan<'a> {
    /// 区分のある動かせるジョブを、区分の順（同じ区分は今の順、--first のジョブは区分の先頭）に並べた列
    list: Vec<&'a str>,
    /// 同じジョブの今の順
    now: Vec<&'a str>,
    /// ジョブごとの「<番号>（<区分>）が <番号>（<区分>）の後ろ」（ORDER: の行の理由）
    behind: Vec<String>,
    /// 区分の無い動かせる待っているジョブ（列のジョブの後ろに回る）
    unclassed: Vec<&'a str>,
    /// 区分の無いジョブが、列のジョブより前にいる（列を渡すと列のジョブが前に出るので、並べ直しが要る）
    unclassed_ahead: bool,
}

/// jobs は走る順に並べたもの
fn plan<'a>(jobs: &[Job<'a>], first: &str) -> Plan<'a> {
    let mut pending: Vec<(&'a str, &'a str, usize)> = Vec::new();
    let mut behind = Vec::new();
    let mut unclassed = Vec::new();
    let mut unclassed_ahead = false;
    for job in jobs {
        if !job.pending_main() || job.unmovable() {
            continue;
        }
        let Some((class, rank)) = job.class else {
            unclassed.push(job.id());
            continue;
        };
        let last_unclassed = unclassed.last();
        if last_unclassed.is_some() {
            unclassed_ahead = true;
        }
        // 先に並ぶ、より下の区分のジョブのうち最も近いもの（無ければ、先に並ぶ区分の無いジョブ）
        if let Some((id, lower, _)) = pending.iter().rev().find(|(_, _, r)| *r > rank) {
            behind.push(format!("{}（{class}）が {id}（{lower}）の後ろ", job.id()));
        } else if let Some(id) = last_unclassed {
            behind.push(format!("{}（{class}）が区分の無い {id} の後ろ", job.id()));
        }
        pending.push((job.id(), class, rank));
    }
    let now: Vec<&str> = pending.iter().map(|(id, _, _)| *id).collect();
    let mut sorted: Vec<(usize, &(&str, &str, usize))> = pending.iter().enumerate().collect();
    // 安定な並べ替えなので、同じ区分は今の順のまま
    sorted.sort_by_key(|(i, (id, _, rank))| (*rank, if same_job(id, first) { 0 } else { i + 1 }));
    Plan {
        list: sorted.iter().map(|(_, (id, _, _))| *id).collect(),
        now,
        behind,
        unclassed,
        unclassed_ahead,
    }
}

#[cfg(test)]
mod tests {
    use super::{Mode, parse_args, render, same_job};
    use crate::script::slurm::{Partition, Phase, QueuedJob};

    const C: &str = "feat/x /home/u/github/xlc-feat-x claude=abc";
    const MAIN: Partition = Partition::Default;
    const TIMED: Partition = Partition::Uninterrupted;
    const PEND: Phase = Phase::Pending;
    const RUN: Phase = Phase::Started;
    const SUSP: Phase = Phase::Suspended;

    fn list(jobs: &[QueuedJob]) -> String {
        render(jobs, &Mode::List)
    }

    fn order(jobs: &[QueuedJob], first: &str) -> String {
        render(
            jobs,
            &Mode::Order {
                first: first.to_string(),
            },
        )
    }

    /// 区分の無いジョブ（コメントが連絡先だけ）。見積もりは 10:00、開始の見込みは N/A、名前は j<番号>
    fn unclassed(id: &str, partition: Partition, phase: Phase, priority: u64) -> QueuedJob {
        QueuedJob {
            id: id.to_string(),
            group: id.split(['_', '+']).next().unwrap().parse().unwrap(),
            array: id.contains('['),
            partition: match partition {
                Partition::Uninterrupted => "timed",
                Partition::Default => "main",
                Partition::Other => "debug",
            }
            .to_string(),
            partition_kind: partition,
            name: format!("j{id}"),
            state: match phase {
                Phase::Started => "RUNNING",
                Phase::Suspended => "SUSPENDED",
                Phase::Pending => "PENDING",
            }
            .to_string(),
            phase,
            priority,
            limit: "10:00".to_string(),
            start: "N/A".to_string(),
            comment: C.to_string(),
            start_at: None,
            limit_minutes: Some(10),
        }
    }

    /// コメントが `<区分> | #1 | p | <連絡先>` のジョブ
    fn job(id: &str, partition: Partition, phase: Phase, priority: u64, class: &str) -> QueuedJob {
        commented(
            unclassed(id, partition, phase, priority),
            &format!("{class} | #1 | p | {C}"),
        )
    }

    fn commented(job: QueuedJob, comment: &str) -> QueuedJob {
        QueuedJob {
            comment: comment.to_string(),
            ..job
        }
    }

    /// 一覧のジョブの行の番号を、出た順に
    fn ids(out: &str) -> Vec<&str> {
        out.lines()
            .filter(|l| l.starts_with(|c: char| c.is_ascii_digit()))
            .map(|l| l.split(' ').next().unwrap())
            .collect()
    }

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    #[test]
    fn each_job_shows_its_fields_class_reference_purpose_and_contact() {
        let out = list(&[QueuedJob {
            name: "xlc-small-core".to_string(),
            limit: "2:30:00".to_string(),
            start: "2026-10-30T03:00:00".to_string(),
            ..commented(
                unclassed("601", MAIN, PEND, 10030),
                &format!(
                    "decision | #0200 | 平衡キセノンで振動が消えるかで、修正方法を決める | {C}"
                ),
            )
        }]);
        assert_eq!(
            out,
            format!(
                "601 PENDING main 10030 decision 2:30:00 2026-10-30T03:00:00 xlc-small-core #0200\n  目的: 平衡キセノンで振動が消えるかで、修正方法を決める\n  連絡先: {C}\n"
            )
        );
    }

    #[test]
    fn everything_after_the_third_separator_is_the_contact_even_with_a_bar() {
        let out = list(&[commented(
            unclassed("601", MAIN, PEND, 10030),
            "trial | - 環境の確認 | 確認だけ | feat/a|b /p",
        )]);
        assert!(
            out.contains("601 PENDING main 10030 trial 10:00 N/A j601 - 環境の確認\n"),
            "{out}"
        );
        assert!(out.contains("  連絡先: feat/a|b /p\n"), "{out}");
    }

    #[test]
    fn a_bar_inside_the_reference_keeps_the_rest_of_the_comment_as_the_contact() {
        // 最初の | の前後が空白でなければ区切りとみなさない（slurm-run.sh は参照と目的の | を拒む）
        let out = list(&[commented(
            unclassed("601", MAIN, RUN, 10030),
            "trial | #1|x | p | c",
        )]);
        assert!(out.contains(" j601 #1|x\n"), "{out}");
        assert!(out.contains("  連絡先: #1|x | p | c\n"), "{out}");
    }

    #[test]
    fn a_job_without_a_class_shows_dash_and_its_comment_as_the_contact() {
        let out = list(&[unclassed("590", MAIN, RUN, 10000)]);
        assert!(
            out.contains("590 RUNNING main 10000 - 10:00 N/A j590 -\n"),
            "{out}"
        );
        assert!(out.contains("  目的: （無い）\n"), "{out}");
        assert!(out.contains(&format!("  連絡先: {C}\n")), "{out}");
    }

    #[test]
    fn a_comment_starting_with_an_unknown_word_has_no_class() {
        let out = list(&[commented(
            unclassed("5", MAIN, RUN, 10),
            "other | #1 | p | c",
        )]);
        assert!(
            out.contains("5 RUNNING main 10 - 10:00 N/A j5 -\n"),
            "{out}"
        );
        assert!(out.contains("  連絡先: other | #1 | p | c\n"), "{out}");
    }

    #[test]
    fn timed_first_then_running_suspended_pending_by_priority_then_number() {
        let out = list(&[
            job("12", MAIN, PEND, 10001, "trial"),
            job("11", MAIN, PEND, 10005, "trial"),
            job("13", MAIN, PEND, 10005, "trial"),
            job("10", MAIN, SUSP, 10000, "trial"),
            job("14", TIMED, PEND, 10000, "timed"),
            job("9", MAIN, RUN, 10000, "trial"),
            job("15", TIMED, RUN, 10000, "timed"),
        ]);
        assert_eq!(ids(&out), ["15", "14", "9", "10", "11", "13", "12"]);
    }

    #[test]
    fn other_partitions_run_with_main_after_timed() {
        let out = list(&[
            job("20", MAIN, PEND, 10010, "trial"),
            job("21", Partition::Other, PEND, 10020, "trial"),
            job("22", TIMED, PEND, 10000, "trial"),
        ]);
        assert_eq!(ids(&out), ["22", "21", "20"]);
    }

    #[test]
    fn jobs_that_started_like_configuring_come_before_pending_ones() {
        let started = |id, state: &str| QueuedJob {
            state: state.to_string(),
            ..job(id, MAIN, RUN, 10000, "trial")
        };
        let out = list(&[
            job("80", MAIN, PEND, 10050, "trial"),
            started("81", "CONFIGURING"),
            started("82", "COMPLETING"),
        ]);
        assert_eq!(ids(&out), ["81", "82", "80"]);
        assert!(out.contains("81 CONFIGURING main"), "{out}");
    }

    #[test]
    fn an_array_job_sorts_by_the_number_before_the_underscore() {
        let out = list(&[
            job("13", MAIN, PEND, 10030, "trial"),
            job("12_[1-3]", MAIN, PEND, 10030, "trial"),
        ]);
        assert_eq!(ids(&out), ["12_[1-3]", "13"]);
    }

    #[test]
    fn parts_of_one_job_with_the_same_key_put_an_id_that_extends_another_first() {
        let out = list(&[
            job("12_1", MAIN, RUN, 10030, "trial"),
            job("12_10", MAIN, RUN, 10030, "trial"),
            job("12_2", MAIN, RUN, 10030, "trial"),
            job("5+1", MAIN, RUN, 10030, "trial"),
            job("5+0", MAIN, RUN, 10030, "trial"),
        ]);
        assert_eq!(ids(&out), ["5+0", "5+1", "12_10", "12_1", "12_2"]);
    }

    #[test]
    fn a_higher_class_behind_a_lower_one_gets_an_order_line() {
        let out = list(&[
            job("20", MAIN, PEND, 10010, "trial"),
            job("21", MAIN, PEND, 10005, "record"),
        ]);
        assert!(
            out.contains(
                "ORDER: main: 21（record）が 20（trial）の後ろで待っている。scontrol top 21,20\n"
            ),
            "{out}"
        );
    }

    #[test]
    fn the_class_order_is_timed_record_decision_trial() {
        let out = list(&[
            job("30", MAIN, PEND, 10040, "trial"),
            job("31", MAIN, PEND, 10030, "decision"),
            job("32", MAIN, PEND, 10020, "record"),
            job("33", MAIN, PEND, 10010, "timed"),
        ]);
        assert!(
            out.contains("ORDER: main: 31（decision）が 30（trial）の後ろ、32（record）が 31（decision）の後ろ、33（timed）が 32（record）の後ろで待っている。scontrol top 33,32,31,30\n"),
            "{out}"
        );
        assert_eq!(out.matches("ORDER:").count(), 1);
    }

    #[test]
    fn the_order_does_not_let_a_job_overtake_a_higher_class_ahead_of_it() {
        // record が先頭で、その後ろの decision が trial の後ろにある。decision だけを前に出すと record を追い越す
        let out = list(&[
            job("70", MAIN, PEND, 10030, "record"),
            job("71", MAIN, PEND, 10020, "trial"),
            job("72", MAIN, PEND, 10010, "decision"),
        ]);
        assert!(
            out.contains("ORDER: main: 72（decision）が 71（trial）の後ろで待っている。scontrol top 70,72,71\n"),
            "{out}"
        );
    }

    #[test]
    fn with_the_same_priority_the_number_decides_who_is_behind() {
        let out = list(&[
            job("41", MAIN, PEND, 10005, "record"),
            job("40", MAIN, PEND, 10005, "trial"),
        ]);
        assert!(
            out.contains(
                "ORDER: main: 41（record）が 40（trial）の後ろで待っている。scontrol top 41,40\n"
            ),
            "{out}"
        );
    }

    #[test]
    fn jobs_already_in_class_order_get_no_order_line() {
        let out = list(&[
            job("50", MAIN, PEND, 10010, "record"),
            job("51", MAIN, PEND, 10005, "trial"),
        ]);
        assert!(!out.contains("ORDER:"), "{out}");
    }

    #[test]
    fn running_jobs_other_partitions_and_unclassed_jobs_behind_are_not_compared() {
        let out = list(&[
            job("60", MAIN, RUN, 10000, "trial"),
            job("61", MAIN, PEND, 10000, "record"),
            job("62", TIMED, PEND, 10050, "trial"),
            job("64", Partition::Other, PEND, 10040, "trial"),
            unclassed("63", MAIN, PEND, 9990),
        ]);
        assert!(!out.contains("ORDER:"), "{out}");
    }

    #[test]
    fn the_order_line_names_the_unclassed_pending_jobs_that_fall_behind() {
        let out = list(&[
            unclassed("90", MAIN, PEND, 10060),
            job("91", MAIN, PEND, 10050, "trial"),
            job("92", MAIN, PEND, 10040, "record"),
        ]);
        assert!(
            out.contains("ORDER: main: 91（trial）が区分の無い 90 の後ろ、92（record）が 91（trial）の後ろで待っている。scontrol top 92,91（区分の無い 90 は後ろに回る）\n"),
            "{out}"
        );
    }

    #[test]
    fn an_empty_queue_says_so_in_the_list_and_prints_nothing_for_order() {
        assert_eq!(list(&[]), "待ち行列にジョブが無い\n");
        assert_eq!(order(&[], ""), "");
    }

    #[test]
    fn timed_jobs_are_not_reordered_by_class() {
        let jobs = [
            job("20", TIMED, PEND, 10010, "decision"),
            job("21", TIMED, PEND, 10005, "timed"),
        ];
        assert!(!list(&jobs).contains("ORDER:"));
        assert_eq!(order(&jobs, ""), "");
    }

    #[test]
    fn held_and_array_parent_jobs_are_unmovable_and_left_out_of_the_order() {
        let out = list(&[
            job("10", MAIN, PEND, 10050, "trial"),
            job("11", MAIN, PEND, 10040, "record"),
            job("12_[1-3]", MAIN, PEND, 10030, "record"),
            job("13", MAIN, PEND, 0, "record"),
            job("14", MAIN, PEND, 1, "timed"),
            job("15", MAIN, PEND, 2, "timed"),
        ]);
        assert!(out.contains("12_[1-3] PENDING（動かせない） main"), "{out}");
        assert!(out.contains("13 PENDING（動かせない） main"), "{out}");
        assert!(out.contains("14 PENDING（動かせない） main"), "{out}");
        assert!(out.contains("15 PENDING main"), "{out}");
        assert!(out.contains("scontrol top 15,11,10\n"), "{out}");
    }

    #[test]
    fn only_unmovable_jobs_out_of_class_order_get_no_order_line() {
        let out = list(&[
            job("10", MAIN, PEND, 10050, "trial"),
            job("13", MAIN, PEND, 0, "record"),
        ]);
        assert!(!out.contains("ORDER:"), "{out}");
    }

    #[test]
    fn order_prints_the_list_only_when_main_needs_reordering() {
        let needs = [
            job("20", MAIN, PEND, 10010, "trial"),
            job("21", MAIN, PEND, 10005, "record"),
        ];
        assert_eq!(order(&needs, ""), "21,20\n");
        let sorted = [
            job("21", MAIN, PEND, 10010, "record"),
            job("20", MAIN, PEND, 10005, "trial"),
        ];
        assert_eq!(order(&sorted, ""), "");
    }

    #[test]
    fn order_first_puts_the_job_at_the_head_of_its_class_without_passing_higher_ones() {
        let jobs = [
            job("21", MAIN, PEND, 10030, "record"),
            job("20", MAIN, PEND, 10020, "trial"),
            job("22", MAIN, PEND, 10010, "trial"),
            job("42", MAIN, PEND, 10000, "trial"),
        ];
        assert_eq!(order(&jobs, "42"), "21,42,20,22\n");
        let already = [
            job("21", MAIN, PEND, 10030, "record"),
            job("42", MAIN, PEND, 10000, "trial"),
        ];
        assert_eq!(order(&already, "42"), "");
    }

    #[test]
    fn order_first_names_the_same_job_however_the_number_is_written() {
        let jobs = [
            job("20", MAIN, PEND, 10020, "trial"),
            job("42", MAIN, PEND, 10000, "trial"),
        ];
        assert_eq!(order(&jobs, "042"), "42,20\n");
    }

    #[test]
    fn an_unclassed_pending_job_ahead_of_a_classed_one_needs_the_order() {
        let ahead = [
            unclassed("90", MAIN, PEND, 10060),
            job("42", MAIN, PEND, 10000, "record"),
        ];
        assert_eq!(order(&ahead, "42"), "42\n");
        assert!(
            list(&ahead).contains("ORDER: main: 42（record）が区分の無い 90 の後ろで待っている。scontrol top 42（区分の無い 90 は後ろに回る）\n"),
            "{}",
            list(&ahead)
        );
        let behind = [
            job("42", MAIN, PEND, 10060, "record"),
            unclassed("90", MAIN, PEND, 10000),
        ];
        assert_eq!(order(&behind, ""), "");
    }

    #[test]
    fn notes_follow_the_jobs_only_for_the_marks_that_appear() {
        let out = list(&[
            commented(
                unclassed("10", MAIN, SUSP, 10050),
                "trial | #1 | p | feat/x /p commit=abc1234-dirty",
            ),
            unclassed("11", MAIN, PEND, 10040),
            job("13", MAIN, PEND, 0, "record"),
        ]);
        let lines: Vec<&str> = out.lines().collect();
        // NOTE: は 3 行ずつのジョブの後ろに出す
        assert_eq!(
            lines[9..],
            [
                "NOTE: commit= の末尾が -dirty のジョブは、コミットしていない変更のある版で走り、結果は hash のコミットから再現できない",
                "NOTE: 区分が - のジョブは slurm-run.sh の外から投げたもので、区分と目的が無く、並べ直されずに区分のあるジョブの後ろに回る",
                "NOTE: （動かせない）は保留（優先度が 1 以下）か配列の親ジョブで、scontrol top で並べ直せず、並べ直したジョブの後ろに回る",
                "NOTE: SUSPENDED は timed のジョブに止められた main のジョブで、それが終わると続きから再開する",
            ]
        );
        let clean = list(&[commented(
            unclassed("601", MAIN, RUN, 10030),
            "trial | #1 | p | feat/x /p commit=abc1234",
        )]);
        assert!(!clean.contains("NOTE:"), "{clean}");
    }

    #[test]
    fn dirty_is_a_whole_commit_word_ending_in_dirty() {
        let dirty = |contact: &str| {
            list(&[commented(
                unclassed("1", MAIN, RUN, 10),
                &format!("trial | #1 | p | {contact}"),
            )])
            .contains("-dirty のジョブ")
        };
        assert!(dirty("commit=-dirty"));
        assert!(dirty("a commit=abc-dirty b"));
        assert!(!dirty("xcommit=abc-dirty"));
        assert!(!dirty("commit=abc-dirty2"));
        assert!(!dirty("commit=abc"));
    }

    #[test]
    fn order_prints_no_notes() {
        let out = order(
            &[
                unclassed("11", MAIN, PEND, 10040),
                commented(
                    unclassed("10", MAIN, SUSP, 10050),
                    "trial | #1 | p | feat/x /p commit=abc1234-dirty",
                ),
            ],
            "",
        );
        assert_eq!(out, "");
    }

    #[test]
    fn the_order_line_is_followed_by_how_to_run_it_at_once() {
        let out = list(&[
            job("20", MAIN, PEND, 10010, "trial"),
            job("21", MAIN, PEND, 10005, "record"),
        ]);
        let next = out.lines().skip_while(|l| !l.starts_with("ORDER:")).nth(1);
        assert_eq!(
            next,
            Some(
                "NEXT: 上の scontrol top を、番号を分けずに 1 回で実行する（1 本ずつ分けると最後に出したジョブが先頭になる）。ほかのセッションのジョブを含んでいても実行してよい"
            )
        );
    }

    #[test]
    fn the_unclassed_note_says_falling_behind_only_for_pending_main_jobs() {
        let out = list(&[unclassed("590", MAIN, RUN, 10000)]);
        assert!(
            out.contains(
                "NOTE: 区分が - のジョブは slurm-run.sh の外から投げたもので、区分と目的が無い\n"
            ),
            "{out}"
        );
        assert!(!out.contains("後ろに回る"), "{out}");
    }

    #[test]
    fn only_order_and_order_first_are_accepted() {
        assert_eq!(parse_args(&[]), Some(Mode::List));
        assert_eq!(
            parse_args(&args(&["--order"])),
            Some(Mode::Order {
                first: String::new()
            })
        );
        assert_eq!(
            parse_args(&args(&["--order", "--first", "42"])),
            Some(Mode::Order {
                first: "42".to_string()
            })
        );
        for bad in [
            &["--all"][..],
            &["--first", "42"],
            &["--order", "--first"],
            &["--order", "extra"],
            &["--order", "--first", "42", "x"],
        ] {
            assert_eq!(parse_args(&args(bad)), None, "{bad:?}");
        }
    }

    #[test]
    fn first_is_the_same_job_by_number_and_otherwise_by_the_whole_id() {
        assert!(same_job("42", "42"));
        assert!(same_job("42", "042"));
        assert!(!same_job("42", "43"));
        assert!(!same_job("12_[1-3]", "12"));
        assert!(same_job("12_[1-3]", "12_[1-3]"));
        assert!(!same_job("42", ""));
    }
}
