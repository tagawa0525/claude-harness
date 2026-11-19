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
//! squeue が失敗したら、その終了コードで終わる。使い方の誤りは 1。

use super::slurm_class;
use std::process::{Command, Stdio};

const USAGE: &str = "Usage: slurm-queue.sh [--order [--first <番号>]]";

/// squeue の欄。名前は slurm-run.sh が英数字と . _ - に限り、ほかの固定の欄も | を含まないので、最後の欄（コメント）の
/// 前までを | で分けられる
const FORMAT: &str = "%i|%P|%j|%T|%Q|%l|%S|%k";

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
    match read_queue() {
        Ok(queue) => {
            print!("{}", render(&queue, &mode));
            0
        }
        Err(code) => code,
    }
}

/// `--order [--first <first>]` と同じ、main を区分の順に並べる番号の列（要らなければ空）。slurm-run が投げた後に
/// `scontrol top` に渡す。squeue が失敗したら、slurm-queue.sh と同じ ERROR: を標準エラーに出して None
pub(super) fn order(first: &str) -> Option<String> {
    let queue = read_queue().ok()?;
    let mode = Mode::Order {
        first: first.to_string(),
    };
    Some(render(&queue, &mode))
}

/// squeue の出力（[`FORMAT`] の行）。失敗したら ERROR: を標準エラーに出し、終了コードを返す
fn read_queue() -> Result<String, i32> {
    let out = match Command::new("squeue")
        .args(["--noheader", "--format", FORMAT])
        .stderr(Stdio::inherit())
        .output()
    {
        Ok(out) => out,
        Err(e) => {
            // sh と同じく、無いコマンドは 127、起動できないコマンドは 126 とする
            let code = if e.kind() == std::io::ErrorKind::NotFound {
                127
            } else {
                126
            };
            eprintln!("squeue を起動できない: {e}");
            eprintln!("ERROR: squeue が失敗した（終了コード {code}）");
            return Err(code);
        }
    };
    if !out.status.success() {
        let code = crate::child::exit_code(out.status);
        eprintln!("ERROR: squeue が失敗した（終了コード {code}）");
        return Err(code);
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
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

/// squeue の出力（[`FORMAT`] の行）から、標準出力に書く全文を作る
fn render(squeue_out: &str, mode: &Mode) -> String {
    let (order, first) = match mode {
        Mode::List => (false, ""),
        Mode::Order { first } => (true, first.as_str()),
    };
    // sh のコマンド置換と同じく、末尾の改行だけを落とす（途中の空行も 1 行として読む）
    let lines = squeue_out.trim_end_matches('\n');
    if lines.is_empty() {
        return if order {
            String::new()
        } else {
            "待ち行列にジョブが無い\n".to_string()
        };
    }
    let jobs = running_order(lines);
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

/// squeue の 1 行
struct Job<'a> {
    /// 並べ替えで鍵が同じときに比べる、元の行
    line: &'a str,
    id: &'a str,
    partition: &'a str,
    name: &'a str,
    state: &'a str,
    priority: &'a str,
    limit: &'a str,
    start: &'a str,
    /// 区分とその順（コメントが区分で始まる 4 つ以上の欄でなければ None）
    class: Option<(&'a str, usize)>,
    reference: &'a str,
    purpose: &'a str,
    contact: &'a str,
}

impl<'a> Job<'a> {
    fn parse(line: &'a str) -> Job<'a> {
        let mut fields = line.splitn(8, '|');
        let mut next = || fields.next().unwrap_or("");
        let (id, partition, name, state, priority, limit, start) =
            (next(), next(), next(), next(), next(), next(), next());
        let comment = next();
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
            line,
            id,
            partition,
            name,
            state,
            priority,
            limit,
            start,
            class,
            reference,
            purpose,
            contact,
        }
    }

    /// 走る順の鍵（パーティション、状態、優先度の高い順、番号）。優先度と番号は、数として読んだ値の整数の部分で比べる
    fn sort_key(&self) -> (i64, i64, i64, i64) {
        let partition = if self.partition == "timed" { 0 } else { 1 };
        let state = match self.state {
            "PENDING" => 2,
            "SUSPENDED" => 1,
            _ => 0,
        };
        (
            partition,
            state,
            // 符号を変えてから整数の部分を取る（優先度 1.5 の鍵は -1）
            (-number(self.priority).0) as i64,
            number(self.id).0 as i64,
        )
    }

    /// 優先度 1 以下は保留（保留のジョブの優先度は 0 か 1 で、scontrol top は優先度 1 以下のジョブを動かせない）、
    /// [ のある番号は配列の親ジョブ
    fn unmovable(&self) -> bool {
        self.state == "PENDING" && (number(self.priority).0 <= 1.0 || self.id.contains('['))
    }

    /// 一覧の 3 行
    fn listing(&self) -> String {
        format!(
            "{} {}{} {} {} {} {} {} {} {}\n  目的: {}\n  連絡先: {}\n",
            self.id,
            self.state,
            if self.unmovable() {
                "（動かせない）"
            } else {
                ""
            },
            self.partition,
            self.priority,
            self.class.map_or("-", |(class, _)| class),
            self.limit,
            self.start,
            self.name,
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

/// 文字列の先頭の 10 進の数（先頭の空白を飛ばし、符号、小数、指数を含む）と、その数が文字列の空白を除いた全体か。
/// 数が無ければ 0。配列のジョブの番号 `12_[1-3]` は 12 で、番号の順に並べる
fn number(text: &str) -> (f64, bool) {
    let text = text.trim_start_matches([' ', '\t', '\n']);
    let bytes = text.as_bytes();
    let digits_from = |mut i: usize| {
        while bytes.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        i
    };
    let mut end = usize::from(matches!(bytes.first(), Some(b'+' | b'-')));
    let int_end = digits_from(end);
    let mut has_digits = int_end > end;
    end = int_end;
    if bytes.get(end) == Some(&b'.') {
        let frac_end = digits_from(end + 1);
        has_digits |= frac_end > end + 1;
        end = frac_end;
    }
    if !has_digits {
        return (0.0, false);
    }
    if matches!(bytes.get(end), Some(b'e' | b'E')) {
        let sign = usize::from(matches!(bytes.get(end + 1), Some(b'+' | b'-')));
        let exp_end = digits_from(end + 1 + sign);
        if exp_end > end + 1 + sign {
            end = exp_end;
        }
    }
    let whole = text[end..].trim_end_matches([' ', '\t', '\n']).is_empty();
    (text[..end].parse().unwrap_or(0.0), whole)
}

/// 番号が --first と同じジョブか。どちらも全体が数なら数で比べる（`--first 042` は 42 のジョブ。slurm-run.sh が
/// 渡す番号の書き方によらず同じジョブを指す）。ほかは文字列で比べる（配列のジョブ `12_[1-3]` は 12 と別）
fn same_job(id: &str, first: &str) -> bool {
    match (number(id), number(first)) {
        ((a, true), (b, true)) => a == b,
        _ => id == first,
    }
}

/// 走る順に並べたジョブ。鍵が同じなら元の行のバイトの順で比べる（ロケールの照合順に頼らず、どの機械でも同じ順にする）
fn running_order(lines: &str) -> Vec<Job<'_>> {
    let mut jobs: Vec<Job> = lines.split('\n').map(Job::parse).collect();
    jobs.sort_by_cached_key(|job| (job.sort_key(), job.line));
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
    if unclassed
        .iter()
        .any(|j| j.state == "PENDING" && j.partition == "main")
    {
        notes.push(NOTE_UNCLASSED_BEHIND);
    } else if !unclassed.is_empty() {
        notes.push(NOTE_UNCLASSED);
    }
    if jobs.iter().any(Job::unmovable) {
        notes.push(NOTE_UNMOVABLE);
    }
    if jobs.iter().any(|j| j.state == "SUSPENDED") {
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
        if job.state != "PENDING" || job.partition != "main" || job.unmovable() {
            continue;
        }
        let Some((class, rank)) = job.class else {
            unclassed.push(job.id);
            continue;
        };
        let last_unclassed = unclassed.last();
        if last_unclassed.is_some() {
            unclassed_ahead = true;
        }
        // 先に並ぶ、より下の区分のジョブのうち最も近いもの（無ければ、先に並ぶ区分の無いジョブ）
        if let Some((id, lower, _)) = pending.iter().rev().find(|(_, _, r)| *r > rank) {
            behind.push(format!("{}（{class}）が {id}（{lower}）の後ろ", job.id));
        } else if let Some(id) = last_unclassed {
            behind.push(format!("{}（{class}）が区分の無い {id} の後ろ", job.id));
        }
        pending.push((job.id, class, rank));
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
    use super::{Mode, number, parse_args, render, same_job};

    const C: &str = "feat/x /home/u/github/xlc-feat-x claude=abc";

    fn list(lines: &[String]) -> String {
        render(&lines.join("\n"), &Mode::List)
    }

    fn order(lines: &[String], first: &str) -> String {
        render(
            &lines.join("\n"),
            &Mode::Order {
                first: first.to_string(),
            },
        )
    }

    /// `<番号>|<パーティション>|<名前>|<状態>|<優先度>|10:00|N/A|<区分> | #1 | p | <連絡先>`
    fn job(id: &str, part: &str, name: &str, state: &str, prio: &str, class: &str) -> String {
        format!("{id}|{part}|{name}|{state}|{prio}|10:00|N/A|{class} | #1 | p | {C}")
    }

    /// 区分の無いジョブ（コメントが連絡先だけ）
    fn unclassed(id: &str, part: &str, state: &str, prio: &str) -> String {
        format!("{id}|{part}|old|{state}|{prio}|10:00|N/A|{C}")
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
        let out = list(&[format!(
            "601|main|xlc-small-core|PENDING|10030|2:30:00|2026-10-30T03:00:00|decision | #0200 | 平衡キセノンで振動が消えるかで、修正方法を決める | {C}"
        )]);
        assert_eq!(
            out,
            format!(
                "601 PENDING main 10030 decision 2:30:00 2026-10-30T03:00:00 xlc-small-core #0200\n  目的: 平衡キセノンで振動が消えるかで、修正方法を決める\n  連絡先: {C}\n"
            )
        );
    }

    #[test]
    fn everything_after_the_third_separator_is_the_contact_even_with_a_bar() {
        let out = list(&[
            "601|main|x|PENDING|10030|30:00|N/A|trial | - 環境の確認 | 確認だけ | feat/a|b /p"
                .to_string(),
        ]);
        assert!(
            out.contains("601 PENDING main 10030 trial 30:00 N/A x - 環境の確認\n"),
            "{out}"
        );
        assert!(out.contains("  連絡先: feat/a|b /p\n"), "{out}");
    }

    #[test]
    fn a_bar_inside_the_reference_keeps_the_rest_of_the_comment_as_the_contact() {
        // 最初の | の前後が空白でなければ区切りとみなさない（slurm-run.sh は参照と目的の | を拒む）
        let out = list(&["601|main|x|RUNNING|10030|30:00|N/A|trial | #1|x | p | c".to_string()]);
        assert!(out.contains(" x #1|x\n"), "{out}");
        assert!(out.contains("  連絡先: #1|x | p | c\n"), "{out}");
    }

    #[test]
    fn a_job_without_a_class_shows_dash_and_its_comment_as_the_contact() {
        let out = list(&[format!(
            "590|main|old-job|RUNNING|10000|20:00|2026-10-29T08:43:25|{C}"
        )]);
        assert!(
            out.contains("590 RUNNING main 10000 - 20:00 2026-10-29T08:43:25 old-job -\n"),
            "{out}"
        );
        assert!(out.contains("  目的: （無い）\n"), "{out}");
        assert!(out.contains(&format!("  連絡先: {C}\n")), "{out}");
    }

    #[test]
    fn a_comment_starting_with_an_unknown_word_has_no_class() {
        let out = list(&["5|main|x|RUNNING|10|1:00|N/A|other | #1 | p | c".to_string()]);
        assert!(out.contains("5 RUNNING main 10 - 1:00 N/A x -\n"), "{out}");
        assert!(out.contains("  連絡先: other | #1 | p | c\n"), "{out}");
    }

    #[test]
    fn timed_first_then_running_suspended_pending_by_priority_then_number() {
        let out = list(&[
            job("12", "main", "m-pend-low", "PENDING", "10001", "trial"),
            job("11", "main", "m-pend-high", "PENDING", "10005", "trial"),
            job("13", "main", "m-pend-tie", "PENDING", "10005", "trial"),
            job("10", "main", "m-susp", "SUSPENDED", "10000", "trial"),
            job("14", "timed", "t-pend", "PENDING", "10000", "timed"),
            job("9", "main", "m-run", "RUNNING", "10000", "trial"),
            job("15", "timed", "t-run", "RUNNING", "10000", "timed"),
        ]);
        assert_eq!(ids(&out), ["15", "14", "9", "10", "11", "13", "12"]);
    }

    #[test]
    fn jobs_that_started_like_configuring_come_before_pending_ones() {
        let out = list(&[
            job("80", "main", "pend", "PENDING", "10050", "trial"),
            job("81", "main", "conf", "CONFIGURING", "10000", "trial"),
            job("82", "main", "comp", "COMPLETING", "10000", "trial"),
        ]);
        assert_eq!(ids(&out), ["81", "82", "80"]);
    }

    #[test]
    fn an_array_job_sorts_by_the_number_before_the_underscore() {
        let out = list(&[
            job("13", "main", "b", "PENDING", "10030", "trial"),
            job("12_[1-3]", "main", "arr", "PENDING", "10030", "trial"),
        ]);
        assert_eq!(ids(&out), ["12_[1-3]", "13"]);
    }

    #[test]
    fn a_higher_class_behind_a_lower_one_gets_an_order_line() {
        let out = list(&[
            job("20", "main", "trial-job", "PENDING", "10010", "trial"),
            job("21", "main", "record-job", "PENDING", "10005", "record"),
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
            job("30", "main", "a", "PENDING", "10040", "trial"),
            job("31", "main", "b", "PENDING", "10030", "decision"),
            job("32", "main", "c", "PENDING", "10020", "record"),
            job("33", "main", "d", "PENDING", "10010", "timed"),
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
            job("70", "main", "r1", "PENDING", "10030", "record"),
            job("71", "main", "t", "PENDING", "10020", "trial"),
            job("72", "main", "d", "PENDING", "10010", "decision"),
        ]);
        assert!(
            out.contains("ORDER: main: 72（decision）が 71（trial）の後ろで待っている。scontrol top 70,72,71\n"),
            "{out}"
        );
    }

    #[test]
    fn with_the_same_priority_the_number_decides_who_is_behind() {
        let out = list(&[
            job("41", "main", "record-job", "PENDING", "10005", "record"),
            job("40", "main", "trial-job", "PENDING", "10005", "trial"),
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
            job("50", "main", "record-job", "PENDING", "10010", "record"),
            job("51", "main", "trial-job", "PENDING", "10005", "trial"),
        ]);
        assert!(!out.contains("ORDER:"), "{out}");
    }

    #[test]
    fn running_jobs_other_partitions_and_unclassed_jobs_behind_are_not_compared() {
        let out = list(&[
            job("60", "main", "trial-run", "RUNNING", "10000", "trial"),
            job("61", "main", "record-pend", "PENDING", "10000", "record"),
            job("62", "timed", "trial-timed", "PENDING", "10050", "trial"),
            unclassed("63", "main", "PENDING", "9990"),
        ]);
        assert!(!out.contains("ORDER:"), "{out}");
    }

    #[test]
    fn the_order_line_names_the_unclassed_pending_jobs_that_fall_behind() {
        let out = list(&[
            unclassed("90", "main", "PENDING", "10060"),
            job("91", "main", "t", "PENDING", "10050", "trial"),
            job("92", "main", "r", "PENDING", "10040", "record"),
        ]);
        assert!(
            out.contains("ORDER: main: 91（trial）が区分の無い 90 の後ろ、92（record）が 91（trial）の後ろで待っている。scontrol top 92,91（区分の無い 90 は後ろに回る）\n"),
            "{out}"
        );
    }

    #[test]
    fn an_empty_queue_says_so_in_the_list_and_prints_nothing_for_order() {
        assert_eq!(render("", &Mode::List), "待ち行列にジョブが無い\n");
        assert_eq!(render("\n\n", &Mode::List), "待ち行列にジョブが無い\n");
        assert_eq!(order(&[], ""), "");
    }

    #[test]
    fn timed_jobs_are_not_reordered_by_class() {
        let lines = [
            job("20", "timed", "quick", "PENDING", "10010", "decision"),
            job("21", "timed", "bench", "PENDING", "10005", "timed"),
        ];
        assert!(!list(&lines).contains("ORDER:"));
        assert_eq!(order(&lines, ""), "");
    }

    #[test]
    fn priorities_with_a_fraction_or_an_exponent_are_read_as_their_whole_number() {
        // 1.5 は 1 より大きいので動かせ、1e3 は 1000 で 1.5 より先に並ぶ
        let out = list(&[
            job("20", "main", "t", "PENDING", "1.5", "trial"),
            job("21", "main", "r", "PENDING", "1e3", "record"),
        ]);
        assert_eq!(ids(&out), ["21", "20"]);
        assert!(!out.contains("動かせない"), "{out}");
        let behind = [
            job("20", "main", "t", "PENDING", "1e3", "trial"),
            job("21", "main", "r", "PENDING", "1.5", "record"),
        ];
        assert_eq!(order(&behind, ""), "21,20\n");
    }

    #[test]
    fn held_and_array_parent_jobs_are_unmovable_and_left_out_of_the_order() {
        let out = list(&[
            job("10", "main", "t", "PENDING", "10050", "trial"),
            job("11", "main", "r", "PENDING", "10040", "record"),
            job("12_[1-3]", "main", "arr", "PENDING", "10030", "record"),
            job("13", "main", "held", "PENDING", "0", "record"),
            job("14", "main", "low", "PENDING", "1", "timed"),
        ]);
        assert!(out.contains("12_[1-3] PENDING（動かせない） main"), "{out}");
        assert!(out.contains("13 PENDING（動かせない） main"), "{out}");
        assert!(out.contains("14 PENDING（動かせない） main"), "{out}");
        assert!(out.contains("scontrol top 11,10\n"), "{out}");
    }

    #[test]
    fn only_unmovable_jobs_out_of_class_order_get_no_order_line() {
        let out = list(&[
            job("10", "main", "t", "PENDING", "10050", "trial"),
            job("13", "main", "held", "PENDING", "0", "record"),
        ]);
        assert!(!out.contains("ORDER:"), "{out}");
    }

    #[test]
    fn order_prints_the_list_only_when_main_needs_reordering() {
        let needs = [
            job("20", "main", "t", "PENDING", "10010", "trial"),
            job("21", "main", "r", "PENDING", "10005", "record"),
        ];
        assert_eq!(order(&needs, ""), "21,20\n");
        let sorted = [
            job("21", "main", "r", "PENDING", "10010", "record"),
            job("20", "main", "t", "PENDING", "10005", "trial"),
        ];
        assert_eq!(order(&sorted, ""), "");
    }

    #[test]
    fn order_first_puts_the_job_at_the_head_of_its_class_without_passing_higher_ones() {
        let lines = [
            job("21", "main", "r", "PENDING", "10030", "record"),
            job("20", "main", "t1", "PENDING", "10020", "trial"),
            job("22", "main", "t2", "PENDING", "10010", "trial"),
            job("42", "main", "new", "PENDING", "10000", "trial"),
        ];
        assert_eq!(order(&lines, "42"), "21,42,20,22\n");
        let already = [
            job("21", "main", "r", "PENDING", "10030", "record"),
            job("42", "main", "new", "PENDING", "10000", "trial"),
        ];
        assert_eq!(order(&already, "42"), "");
    }

    #[test]
    fn order_first_names_the_same_job_however_the_number_is_written() {
        let lines = [
            job("20", "main", "t1", "PENDING", "10020", "trial"),
            job("42", "main", "new", "PENDING", "10000", "trial"),
        ];
        assert_eq!(order(&lines, "042"), "42,20\n");
    }

    #[test]
    fn an_unclassed_pending_job_ahead_of_a_classed_one_needs_the_order() {
        let ahead = [
            unclassed("90", "main", "PENDING", "10060"),
            job("42", "main", "new", "PENDING", "10000", "record"),
        ];
        assert_eq!(order(&ahead, "42"), "42\n");
        assert!(
            list(&ahead).contains("ORDER: main: 42（record）が区分の無い 90 の後ろで待っている。scontrol top 42（区分の無い 90 は後ろに回る）\n"),
            "{}",
            list(&ahead)
        );
        let behind = [
            job("42", "main", "new", "PENDING", "10060", "record"),
            unclassed("90", "main", "PENDING", "10000"),
        ];
        assert_eq!(order(&behind, ""), "");
    }

    #[test]
    fn notes_follow_the_jobs_only_for_the_marks_that_appear() {
        let out = list(&[
            "10|main|s|SUSPENDED|10050|10:00|N/A|trial | #1 | p | feat/x /p commit=abc1234-dirty"
                .to_string(),
            unclassed("11", "main", "PENDING", "10040"),
            job("13", "main", "held", "PENDING", "0", "record"),
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
        let clean = list(&[
            "601|main|x|RUNNING|10030|30:00|N/A|trial | #1 | p | feat/x /p commit=abc1234"
                .to_string(),
        ]);
        assert!(!clean.contains("NOTE:"), "{clean}");
    }

    #[test]
    fn dirty_is_a_whole_commit_word_ending_in_dirty() {
        let dirty = |contact: &str| {
            list(&[format!(
                "1|main|x|RUNNING|10|1:00|N/A|trial | #1 | p | {contact}"
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
                unclassed("11", "main", "PENDING", "10040"),
                "10|main|s|SUSPENDED|10050|10:00|N/A|trial | #1 | p | feat/x /p commit=abc1234-dirty".to_string(),
            ],
            "",
        );
        assert_eq!(out, "");
    }

    #[test]
    fn the_order_line_is_followed_by_how_to_run_it_at_once() {
        let out = list(&[
            job("20", "main", "trial-job", "PENDING", "10010", "trial"),
            job("21", "main", "record-job", "PENDING", "10005", "record"),
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
        let out = list(&[unclassed("590", "main", "RUNNING", "10000")]);
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
    fn numbers_are_the_leading_decimal_number_of_the_field() {
        assert_eq!(number("10030"), (10030.0, true));
        assert_eq!(number("12_[1-3]"), (12.0, false));
        assert_eq!(number(" -5x"), (-5.0, false));
        assert_eq!(number("1.5"), (1.5, true));
        assert_eq!(number("1e3 "), (1000.0, true));
        assert_eq!(number("2e"), (2.0, false));
        assert_eq!(number(".5"), (0.5, true));
        assert_eq!(number(""), (0.0, false));
        assert_eq!(number("N/A"), (0.0, false));
        assert!(same_job("42", "42.0"));
        assert!(same_job("42", "042"));
        assert!(!same_job("12_[1-3]", "12"));
        assert!(!same_job("42", ""));
    }
}
