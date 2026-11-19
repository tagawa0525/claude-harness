//! slurm-run.sh が `--class` で受け、ジョブのコメントの先頭に書く区分（ADR-0101）。並べ直す順に並べる（先のものが
//! 先に走る）。slurm-queue の並べ直しと一覧はこの一覧を読む。slurm-run.sh の --class の検査（case の並び）は、
//! slurm-run を claude-hooks に移すまで別に持つので、区分を変えるときは両方を合わせる（ADR-0101 の帰結の順の 2 で
//! こちらに寄せる）

/// 区分。timed（時間を記録する測定）、record（記録に載せる本番）、decision（決定に要る確認）、trial（検討）の順に優先する
pub const CLASSES: [&str; 4] = ["timed", "record", "decision", "trial"];

/// 区分の順（小さいほど先）。区分でなければ None
pub fn rank(class: &str) -> Option<usize> {
    CLASSES.iter().position(|c| *c == class)
}

#[cfg(test)]
mod tests {
    use super::rank;

    #[test]
    fn the_classes_rank_timed_record_decision_trial() {
        let ranks: Vec<_> = ["timed", "record", "decision", "trial"]
            .iter()
            .map(|c| rank(c).unwrap())
            .collect();
        assert!(ranks.windows(2).all(|w| w[0] < w[1]), "{ranks:?}");
    }

    #[test]
    fn other_words_are_not_classes() {
        assert_eq!(rank("-"), None);
        assert_eq!(rank(""), None);
        assert_eq!(rank("Timed"), None);
    }
}
