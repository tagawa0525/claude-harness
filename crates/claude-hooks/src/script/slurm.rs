//! Slurm のコマンド（sbatch、squeue、sacct、scontrol）を呼ぶ所と、その出力を解く所（ADR-0101）。
//!
//! 判定（slurm_run の待ち、印、終了コード）は、ここが返す、Slurm の名前や出力の形を含まない型（[`JobRecord`]、
//! [`JobState`]、[`RunningJob`]、[`Submission`]）を受ける。会社の環境のスケジューラ（Sun Grid Engine）の実装を後で足す
//! ため、Slurm に依る所をこのモジュールに集める（#353）。差し替えの口（trait）は、2 つ目の実装を足すときに 2 つを比べて
//! 切り出す。
//!
//! 状態は `--json=v0.0.45` の形で読む。版を固定するのは、Slurm の更新で形が変わったときに、読み違えずに起動の失敗で
//! 気付くため。r995 の Slurm は 26.05.6 で、選べる形の版は v0.0.42〜v0.0.45（2026-11-19 に確かめた）。
//! 待ち行列を区分の順に並べる列は、まだ slurm_queue が `squeue --format` の文字の列で読む（ADR-0101 の帰結の順の 3 で
//! JSON に替える）。

use regex::Regex;
use serde_json::Value;
use std::process::{Child, Command, Stdio};
use std::sync::LazyLock;

const JSON: &str = "--json=v0.0.45";

/// 割り込まれない（ほかのジョブを止めて走り、自分は止められない）パーティション。時間を記録するジョブを投げる
const UNINTERRUPTED: &str = "timed";

/// 既定のパーティション。割り込まれうる
const DEFAULT: &str = "main";

/// 投げるジョブ
pub struct Submission<'a> {
    /// 排他にせず取る CPU の数（None なら排他）
    pub cpus: Option<&'a str>,
    /// 割り込まれないパーティションに投げる
    pub uninterrupted: bool,
    pub name: &'a str,
    /// ログを書くディレクトリ（ジョブの走るホストの書き方）。ファイル名は `<name>-<番号>.out`
    pub log_dir: &'a str,
    /// 見積もり（[`valid_time_limit`] の形）
    pub estimate: &'a str,
    /// 待ち行列の一覧に出すコメント
    pub comment: &'a str,
    pub command: &'a str,
}

/// 終わりまで待つ形で投げる子のプロセス（標準出力はパイプ。最初の行を [`submitted_job`] で読む）
pub fn submit(job: &Submission) -> std::io::Result<Child> {
    todo!("RED")
}

/// sbatch の引数。排他（既定）か CPU の数、パーティション、終わるまで待つ、環境を引き継ぐ、名前、ログ、見積もり、
/// コメント、コマンド
fn submit_args(job: &Submission) -> Vec<String> {
    todo!("RED")
}

static SUBMITTED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^Submitted batch job ([0-9]+)").expect("正規表現は正しい"));

/// 投げた子の最初の行から、受けたジョブの番号
pub fn submitted_job(line: &str) -> Option<String> {
    todo!("RED")
}

static TIME_LIMIT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^([0-9]+|[0-9]+:[0-9]{2}:[0-9]{2}|[0-9]+-[0-9]+(:[0-9]{2}){0,2})$")
        .expect("正規表現は正しい")
});

/// 見積もりの形（sbatch --time のうち、分、時:分:秒、日-時[:分[:秒]]）か。分:秒（1:30）は sbatch が受けるが、時:分の
/// つもりで書くと 60 分の 1 の見積もりになるので受けない
pub fn valid_time_limit(estimate: &str) -> bool {
    todo!("RED")
}

/// 待ち行列の先頭に、list（`,` でつないだ番号）をその順に出す
pub fn move_to_front(list: &str) -> bool {
    todo!("RED")
}

/// 走っているジョブ（止められたものと走り始めたものを含む）
#[derive(Debug, PartialEq)]
pub struct RunningJob {
    pub id: String,
    /// 投げたときの作業ディレクトリ（ジョブの走るホストの書き方）
    pub dir: String,
}

/// 既定のパーティションで走っているジョブ。読めなければ理由（コマンドの標準エラーの最初の行か、解けない理由）
pub fn running_default_jobs() -> Result<Vec<RunningJob>, String> {
    todo!("RED")
}

fn running_jobs(json: &str) -> Result<Vec<RunningJob>, String> {
    todo!("RED")
}

/// ジョブの状態
#[derive(Clone, Debug, PartialEq)]
pub struct JobState {
    /// 出力に書く名前（Slurm の State の最初の語）
    pub name: String,
    /// 終わった（もう変わらない）
    pub ended: bool,
    /// 成功して終わった
    pub completed: bool,
}

impl JobState {
    /// Slurm の State（`CANCELLED by 1000` のように後ろが付くものは最初の語で見る）から
    pub fn from_slurm(state: &str) -> JobState {
        todo!("RED")
    }
}

/// 終わった State だけを列挙する。それ以外（PENDING、CONFIGURING、RUNNING、SUSPENDED、COMPLETING、REQUEUED など今後増える
/// 非終端の State を含む）は、終わりが届くまで待つ
fn is_final(state: &str) -> bool {
    matches!(
        state,
        "COMPLETED"
            | "FAILED"
            | "CANCELLED"
            | "TIMEOUT"
            | "NODE_FAIL"
            | "OUT_OF_MEMORY"
            | "PREEMPTED"
            | "BOOT_FAIL"
            | "DEADLINE"
            | "REVOKED"
            | "SPECIAL_EXIT"
            | "STOPPED"
    )
}

/// 見積もり
#[derive(Debug, PartialEq)]
pub enum Limit {
    Minutes(i64),
    Unlimited,
    /// パーティションの上限に任せた
    Unset,
}

/// 終わったジョブの記録
#[derive(Debug, PartialEq)]
pub struct JobRecord {
    /// 走った秒数（止められていた間を除く）
    pub elapsed: Option<i64>,
    pub limit: Option<Limit>,
    /// 割り込まれないパーティションで走ったか（記録に無ければ None）
    pub uninterrupted: Option<bool>,
    /// 止められていた秒数（記録に無ければ None）
    pub suspended: Option<i64>,
    pub state: JobState,
}

/// 会計の記録（sacct）。読めない、記録がまだ届いていないなら None。標準エラーは捨てる（読めないことは呼ぶ側が警告する）
pub fn accounting_record(job: &str) -> Option<JobRecord> {
    todo!("RED")
}

fn sacct_record(json: &str, job: &str) -> Option<JobRecord> {
    todo!("RED")
}

/// 制御のデーモン（scontrol）が覚えている状態。終わった直後（MinJobAge の 300 秒の間）だけ読める
pub fn controller_state(job: &str) -> Option<JobState> {
    todo!("RED")
}

fn scontrol_state(json: &str, job: &str) -> Option<JobState> {
    todo!("RED")
}

/// ジョブの番号（数か文字列）を文字列に
fn id_text(id: &Value) -> String {
    match id {
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        _ => String::new(),
    }
}

/// コマンドを起動し、成功すれば標準出力
fn quiet_output(argv: &[&str]) -> Option<String> {
    let out = Command::new(argv[0])
        .args(&argv[1..])
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::{
        JobRecord, JobState, Limit, RunningJob, Submission, running_jobs, sacct_record,
        scontrol_state, submit_args, submitted_job, valid_time_limit,
    };
    use serde_json::json;

    fn job<'a>(cpus: Option<&'a str>, uninterrupted: bool) -> Submission<'a> {
        Submission {
            cpus,
            uninterrupted,
            name: "xlc-2G",
            log_dir: "/home/u/github/slurm-logs",
            estimate: "30",
            comment: "trial | #1 | p | c",
            command: "cargo run --release",
        }
    }

    #[test]
    #[ignore = "RED: 実装の前"]
    fn a_job_waits_exclusively_on_main_with_the_environment_name_log_estimate_comment_and_command()
    {
        assert_eq!(
            submit_args(&job(None, false)),
            [
                "--exclusive",
                "--partition=main",
                "--wait",
                "--export=ALL",
                "-J",
                "xlc-2G",
                "-o",
                "/home/u/github/slurm-logs/%x-%j.out",
                "--time",
                "30",
                "--comment",
                "trial | #1 | p | c",
                "--wrap",
                "cargo run --release",
            ]
        );
    }

    #[test]
    #[ignore = "RED: 実装の前"]
    fn a_shared_job_takes_cpus_and_an_uninterrupted_job_goes_to_timed() {
        assert_eq!(
            submit_args(&job(Some("8"), false))[..2],
            ["--cpus-per-task=8", "--partition=main"]
        );
        assert_eq!(
            submit_args(&job(None, true))[..2],
            ["--exclusive", "--partition=timed"]
        );
    }

    #[test]
    #[ignore = "RED: 実装の前"]
    fn the_job_number_is_read_from_the_first_line() {
        assert_eq!(
            submitted_job("Submitted batch job 42\n").as_deref(),
            Some("42")
        );
        assert_eq!(
            submitted_job("Submitted batch job 42 on cluster c\n").as_deref(),
            Some("42")
        );
        assert_eq!(submitted_job("sbatch: error: x\n"), None);
        assert_eq!(submitted_job(""), None);
    }

    #[test]
    #[ignore = "RED: 実装の前"]
    fn the_estimate_is_minutes_or_h_mm_ss_or_days_hours() {
        for bad in ["25m", "1h", "1.5", "abc", "1:30", "30:00", ""] {
            assert!(!valid_time_limit(bad), "{bad}");
        }
        for good in ["90", "1:30:00", "1-00", "1-12:00", "2-12:00:00"] {
            assert!(valid_time_limit(good), "{good}");
        }
    }

    #[test]
    #[ignore = "RED: 実装の前"]
    fn only_final_states_end_by_their_first_word_and_only_completed_succeeds() {
        for state in [
            "COMPLETED",
            "FAILED",
            "CANCELLED by 1000",
            "TIMEOUT",
            "NODE_FAIL",
            "OUT_OF_MEMORY",
            "PREEMPTED",
            "BOOT_FAIL",
            "DEADLINE",
            "REVOKED",
            "SPECIAL_EXIT",
            "STOPPED",
        ] {
            assert!(JobState::from_slurm(state).ended, "{state}");
        }
        for state in [
            "PENDING",
            "CONFIGURING",
            "RUNNING",
            "SUSPENDED",
            "COMPLETING",
            "REQUEUED",
            "",
        ] {
            assert!(!JobState::from_slurm(state).ended, "{state}");
        }
        let cancelled = JobState::from_slurm("CANCELLED by 1000");
        assert_eq!(cancelled.name, "CANCELLED");
        assert!(!cancelled.completed);
        assert!(JobState::from_slurm("COMPLETED").completed);
    }

    #[test]
    #[ignore = "RED: 実装の前"]
    fn sacct_json_gives_the_times_the_partition_kind_and_the_first_state() {
        let json = json!({"jobs": [{
            "job_id": 42,
            "partition": "main",
            "state": {"current": ["CANCELLED"], "reason": "None"},
            "time": {"elapsed": 465, "suspended": 0, "limit": {"set": true, "infinite": false, "number": 175}},
        }]})
        .to_string();
        assert_eq!(
            sacct_record(&json, "42"),
            Some(JobRecord {
                elapsed: Some(465),
                limit: Some(Limit::Minutes(175)),
                uninterrupted: Some(false),
                suspended: Some(0),
                state: JobState::from_slurm("CANCELLED"),
            })
        );
        assert_eq!(sacct_record(&json, "43"), None);
        // 記録が slurmdbd に届く前は jobs が空
        assert_eq!(sacct_record(r#"{"jobs": []}"#, "42"), None);
        assert_eq!(sacct_record("Invalid", "42"), None);
    }

    #[test]
    #[ignore = "RED: 実装の前"]
    fn sacct_json_reads_timed_unlimited_unset_and_missing_fields() {
        let record = |job: serde_json::Value| {
            sacct_record(&json!({"jobs": [job]}).to_string(), "42").unwrap()
        };
        let timed = record(
            json!({"job_id": 42, "partition": "timed", "state": {"current": ["COMPLETED"]},
            "time": {"limit": {"set": true, "infinite": true, "number": 0}}}),
        );
        assert_eq!(timed.uninterrupted, Some(true));
        assert_eq!(timed.limit, Some(Limit::Unlimited));
        let unset = record(
            json!({"job_id": 42, "partition": "", "state": {"current": ["COMPLETED"]},
            "time": {"limit": {"set": false, "infinite": false, "number": 0}}}),
        );
        assert_eq!(unset.limit, Some(Limit::Unset));
        assert_eq!(unset.uninterrupted, None);
        let bare = record(json!({"job_id": 42, "state": {"current": ["COMPLETED"]}, "time": {}}));
        assert_eq!(
            (bare.elapsed, bare.limit, bare.suspended),
            (None, None, None)
        );
    }

    #[test]
    #[ignore = "RED: 実装の前"]
    fn scontrol_json_gives_the_first_state_of_the_job() {
        let json = json!({"jobs": [{"job_id": 42, "job_state": ["CANCELLED"]}]}).to_string();
        assert_eq!(
            scontrol_state(&json, "42"),
            Some(JobState::from_slurm("CANCELLED"))
        );
        assert_eq!(scontrol_state(&json, "7"), None);
        // 覚えている時間（MinJobAge）を過ぎたジョブは、jobs が空で終了コード 0
        assert_eq!(scontrol_state(r#"{"jobs": []}"#, "42"), None);
    }

    #[test]
    #[ignore = "RED: 実装の前"]
    fn squeue_json_gives_the_running_jobs_and_their_working_directories() {
        let json =
            json!({"jobs": [{"job_id": 961, "current_working_directory": "/home/u/github/xlc"}]})
                .to_string();
        assert_eq!(
            running_jobs(&json),
            Ok(vec![RunningJob {
                id: "961".to_string(),
                dir: "/home/u/github/xlc".to_string()
            }])
        );
        assert!(running_jobs("not json").is_err());
        assert!(running_jobs("{}").is_err());
    }
}
