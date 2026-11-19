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

#[derive(Debug, PartialEq)]
#[cfg_attr(not(test), allow(dead_code))]
enum Mode {
    List,
    /// first は、自分の区分の先頭に置くジョブの番号（無ければ空）
    Order {
        first: String,
    },
}

pub fn main(args: &[String]) -> i32 {
    let _ = parse_args(args);
    1
}

fn parse_args(_args: &[String]) -> Option<Mode> {
    None
}

#[cfg_attr(not(test), allow(dead_code))]
fn render(_squeue_out: &str, _mode: &Mode) -> String {
    String::new()
}

#[cfg_attr(not(test), allow(dead_code))]
fn leading_integer(_text: &str) -> i64 {
    0
}

#[cfg_attr(not(test), allow(dead_code))]
fn same_job(_id: &str, _first: &str) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::{Mode, leading_integer, parse_args, render, same_job};

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
    #[ignore = "RED: slurm-queue は未実装"]
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
    #[ignore = "RED: slurm-queue は未実装"]
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
    #[ignore = "RED: slurm-queue は未実装"]
    fn a_bar_inside_the_reference_keeps_it_in_the_contact_like_the_sh_version() {
        // sh の版の awk は、最初の | の前後が空白でなければ区切りを外さない（slurm-run.sh は参照と目的の | を拒む）
        let out = list(&["601|main|x|RUNNING|10030|30:00|N/A|trial | #1|x | p | c".to_string()]);
        assert!(out.contains(" x #1|x\n"), "{out}");
        assert!(out.contains("  連絡先: #1|x | p | c\n"), "{out}");
    }

    #[test]
    #[ignore = "RED: slurm-queue は未実装"]
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
    #[ignore = "RED: slurm-queue は未実装"]
    fn a_comment_starting_with_an_unknown_word_has_no_class() {
        let out = list(&["5|main|x|RUNNING|10|1:00|N/A|other | #1 | p | c".to_string()]);
        assert!(out.contains("5 RUNNING main 10 - 1:00 N/A x -\n"), "{out}");
        assert!(out.contains("  連絡先: other | #1 | p | c\n"), "{out}");
    }

    #[test]
    #[ignore = "RED: slurm-queue は未実装"]
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
    #[ignore = "RED: slurm-queue は未実装"]
    fn jobs_that_started_like_configuring_come_before_pending_ones() {
        let out = list(&[
            job("80", "main", "pend", "PENDING", "10050", "trial"),
            job("81", "main", "conf", "CONFIGURING", "10000", "trial"),
            job("82", "main", "comp", "COMPLETING", "10000", "trial"),
        ]);
        assert_eq!(ids(&out), ["81", "82", "80"]);
    }

    #[test]
    #[ignore = "RED: slurm-queue は未実装"]
    fn an_array_job_sorts_by_the_number_before_the_underscore() {
        let out = list(&[
            job("13", "main", "b", "PENDING", "10030", "trial"),
            job("12_[1-3]", "main", "arr", "PENDING", "10030", "trial"),
        ]);
        assert_eq!(ids(&out), ["12_[1-3]", "13"]);
    }

    #[test]
    #[ignore = "RED: slurm-queue は未実装"]
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
    #[ignore = "RED: slurm-queue は未実装"]
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
    #[ignore = "RED: slurm-queue は未実装"]
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
    #[ignore = "RED: slurm-queue は未実装"]
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
    #[ignore = "RED: slurm-queue は未実装"]
    fn jobs_already_in_class_order_get_no_order_line() {
        let out = list(&[
            job("50", "main", "record-job", "PENDING", "10010", "record"),
            job("51", "main", "trial-job", "PENDING", "10005", "trial"),
        ]);
        assert!(!out.contains("ORDER:"), "{out}");
    }

    #[test]
    #[ignore = "RED: slurm-queue は未実装"]
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
    #[ignore = "RED: slurm-queue は未実装"]
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
    #[ignore = "RED: slurm-queue は未実装"]
    fn an_empty_queue_says_so_in_the_list_and_prints_nothing_for_order() {
        assert_eq!(render("", &Mode::List), "待ち行列にジョブが無い\n");
        assert_eq!(render("\n\n", &Mode::List), "待ち行列にジョブが無い\n");
        assert_eq!(order(&[], ""), "");
    }

    #[test]
    #[ignore = "RED: slurm-queue は未実装"]
    fn timed_jobs_are_not_reordered_by_class() {
        let lines = [
            job("20", "timed", "quick", "PENDING", "10010", "decision"),
            job("21", "timed", "bench", "PENDING", "10005", "timed"),
        ];
        assert!(!list(&lines).contains("ORDER:"));
        assert_eq!(order(&lines, ""), "");
    }

    #[test]
    #[ignore = "RED: slurm-queue は未実装"]
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
    #[ignore = "RED: slurm-queue は未実装"]
    fn only_unmovable_jobs_out_of_class_order_get_no_order_line() {
        let out = list(&[
            job("10", "main", "t", "PENDING", "10050", "trial"),
            job("13", "main", "held", "PENDING", "0", "record"),
        ]);
        assert!(!out.contains("ORDER:"), "{out}");
    }

    #[test]
    #[ignore = "RED: slurm-queue は未実装"]
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
    #[ignore = "RED: slurm-queue は未実装"]
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
    #[ignore = "RED: slurm-queue は未実装"]
    fn order_first_compares_numbers_as_awk_does() {
        let lines = [
            job("20", "main", "t1", "PENDING", "10020", "trial"),
            job("42", "main", "new", "PENDING", "10000", "trial"),
        ];
        assert_eq!(order(&lines, "042"), "42,20\n");
    }

    #[test]
    #[ignore = "RED: slurm-queue は未実装"]
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
    #[ignore = "RED: slurm-queue は未実装"]
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
    #[ignore = "RED: slurm-queue は未実装"]
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
    #[ignore = "RED: slurm-queue は未実装"]
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
    #[ignore = "RED: slurm-queue は未実装"]
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
    #[ignore = "RED: slurm-queue は未実装"]
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
    #[ignore = "RED: slurm-queue は未実装"]
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
    #[ignore = "RED: slurm-queue は未実装"]
    fn numbers_are_read_like_awk() {
        assert_eq!(leading_integer("10030"), 10030);
        assert_eq!(leading_integer("12_[1-3]"), 12);
        assert_eq!(leading_integer(" -5x"), -5);
        assert_eq!(leading_integer(""), 0);
        assert_eq!(leading_integer("N/A"), 0);
        assert!(same_job("42", "042"));
        assert!(!same_job("12_[1-3]", "12"));
        assert!(!same_job("42", ""));
    }
}
