//! Slurm のコマンド（sbatch、squeue、sacct、scontrol）を呼ぶ所と、その出力を解く所（ADR-0101）。
//!
//! 判定（slurm_run の待ち、印、終了コード、slurm_queue の走る順と並べ直す列）は、ここが返す、Slurm の名前や出力の形を
//! 含まない型（[`JobRecord`]、[`JobState`]、[`RunningJob`]、[`Submission`]、[`QueuedJob`]）を受ける。会社の環境の
//! スケジューラ（Sun Grid Engine）の実装を後で足すため、Slurm に依る所をこのモジュールに集める（#353）。差し替えの口
//! （trait）は、2 つ目の実装を足すときに 2 つを比べて切り出す。
//!
//! 状態は `--json=v0.0.45` の形で読む。版を固定するのは、Slurm の更新で形が変わったときに、読み違えずに起動の失敗で
//! 気付くため。r995 の Slurm は 26.05.6 で、選べる形の版は v0.0.42〜v0.0.45（2026-11-19 に確かめた）。

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
    /// ログを書くディレクトリ（ジョブの走るホストの書き方）。ファイル名は [`log_file`]
    pub log_dir: &'a str,
    /// 見積もり（[`valid_time_limit`] の形）
    pub estimate: &'a str,
    /// 待ち行列の一覧に出すコメント
    pub comment: &'a str,
    pub command: &'a str,
}

/// 終わりまで待つ形で投げる子のプロセス（標準出力はパイプ。最初の行を [`submitted_job`] で読む）
pub fn submit(job: &Submission) -> std::io::Result<Child> {
    let mut cmd = Command::new("sbatch");
    cmd.args(submit_args(job))
        .stdin(Stdio::null())
        .stdout(Stdio::piped());
    crate::interrupt::ignore_terminal_interrupt(&mut cmd);
    cmd.spawn()
}

/// ジョブのログのファイル名（[`Submission::log_dir`] の下）。投げるときに渡す名前の形と同じ
pub fn log_file(name: &str, job: &str) -> String {
    format!("{name}-{job}.out")
}

/// sbatch の引数。排他（既定）か CPU の数、パーティション、終わるまで待つ、環境を引き継ぐ、名前、ログ、見積もり、
/// コメント、コマンド
fn submit_args(job: &Submission) -> Vec<String> {
    let mut args = vec![match job.cpus {
        Some(cpus) => format!("--cpus-per-task={cpus}"),
        None => "--exclusive".to_string(),
    }];
    // 既定のパーティションも明示する。コマンド行の --partition は SBATCH_PARTITION より強く、環境から割り込まれない
    // パーティションに入った -c のジョブが、時間を記録するジョブとして数えられることを防ぐ
    let partition = if job.uninterrupted {
        UNINTERRUPTED
    } else {
        DEFAULT
    };
    args.push(format!("--partition={partition}"));
    args.extend(
        [
            "--wait",
            "--export=ALL",
            "-J",
            job.name,
            "-o",
            // %x はジョブの名前、%j は番号に置き換わる
            &format!("{}/{}", job.log_dir, log_file("%x", "%j")),
            "--time",
            job.estimate,
            "--comment",
            job.comment,
            "--wrap",
            job.command,
        ]
        .map(str::to_string),
    );
    args
}

static SUBMITTED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^Submitted batch job ([0-9]+)").expect("正規表現は正しい"));

/// 投げた子の最初の行から、受けたジョブの番号
pub fn submitted_job(line: &str) -> Option<String> {
    SUBMITTED.captures(line).map(|c| c[1].to_string())
}

static TIME_LIMIT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^([0-9]+|[0-9]+:[0-9]{2}:[0-9]{2}|[0-9]+-[0-9]+(:[0-9]{2}){0,2})$")
        .expect("正規表現は正しい")
});

/// 見積もりの形（sbatch --time のうち、分、時:分:秒、日-時[:分[:秒]]）か。分:秒（1:30）は sbatch が受けるが、時:分の
/// つもりで書くと 60 分の 1 の見積もりになるので受けない
pub fn valid_time_limit(estimate: &str) -> bool {
    TIME_LIMIT.is_match(estimate)
}

/// 見積もり（[`valid_time_limit`] の形）の分。秒は分に切り上げる（見込みを短くしない）。形が違えば None
pub fn estimate_minutes(estimate: &str) -> Option<u64> {
    if !valid_time_limit(estimate) {
        return None;
    }
    let (days, clock) = match estimate.split_once('-') {
        Some((days, clock)) => (days.parse::<u64>().ok()?, Some(clock)),
        None => (0, None),
    };
    let parts: Vec<u64> = clock
        .unwrap_or(estimate)
        .split(':')
        .map(|p| p.parse().ok())
        .collect::<Option<_>>()?;
    // 日の無い形は、分か時:分:秒。日のある形は、時、時:分、時:分:秒
    let (hours, minutes, seconds) = match (clock.is_some(), parts.as_slice()) {
        (false, [m]) => (0, *m, 0),
        (false, [h, m, s]) | (true, [h, m, s]) => (*h, *m, *s),
        (true, [h]) => (*h, 0, 0),
        (true, [h, m]) => (*h, *m, 0),
        _ => return None,
    };
    let minutes = ((days * 24 + hours) * 60 + minutes).checked_add(seconds.div_ceil(60))?;
    Some(minutes)
}

/// 待ち行列の先頭に、list（`,` でつないだ番号）をその順に出す
pub fn move_to_front(list: &str) -> bool {
    Command::new("scontrol")
        .args(["top", list])
        .status()
        .is_ok_and(|s| s.success())
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
    let out = Command::new("squeue")
        .args([
            JSON,
            &format!("--partition={DEFAULT}"),
            "--states=RUNNING,SUSPENDED,CONFIGURING",
        ])
        .output()
        .map_err(|e| format!("squeue を起動できない: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr)
            .lines()
            .next()
            .unwrap_or("")
            .to_string());
    }
    running_jobs(&String::from_utf8_lossy(&out.stdout))
}

fn running_jobs(json: &str) -> Result<Vec<RunningJob>, String> {
    Ok(json_jobs("squeue", json)?
        .iter()
        .map(|job| RunningJob {
            id: id_text(&job["job_id"]),
            dir: job["current_working_directory"]
                .as_str()
                .unwrap_or("")
                .to_string(),
        })
        .collect())
}

/// 待ち行列のパーティションの種類
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Partition {
    /// 割り込まれない（時間を記録するジョブの）パーティション
    Uninterrupted,
    /// 既定のパーティション
    Default,
    /// ほか（複数のパーティションに投げて、まだ決まっていないものを含む）
    Other,
}

/// 待ち行列のジョブの段階
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Phase {
    /// 走り始めた（走っている、走る準備をしている、終わりかけている、など）
    Started,
    /// 止められた
    Suspended,
    /// 走るのを待っている
    Pending,
}

/// 待ち行列のジョブ。文字列の欄は、一覧に出す形
#[derive(Clone, Debug, PartialEq)]
pub struct QueuedJob {
    /// 番号（配列のまとめて待っているものは `12_[1-3]`、配列の 1 つは `12_1`、異種のジョブの部分は `5+1`）
    pub id: String,
    /// 番号の先頭の数（配列と異種のジョブは、まとめた番号）
    pub group: u64,
    /// 配列のまとめて待っているもの（1 つのジョブとして動かせない）
    pub array: bool,
    pub partition: String,
    pub partition_kind: Partition,
    pub name: String,
    pub state: String,
    pub phase: Phase,
    /// 大きいほど先に走る
    pub priority: u64,
    /// 見積もり（`10:00`、`2:55:00`、`1-00:00:00`、`UNLIMITED` など）
    pub limit: String,
    /// 開始の見込みか走り始めた時刻（`2026-11-19T17:45:36`。無ければ `N/A`）
    pub start: String,
    pub comment: String,
    /// 開始の見込みか走り始めた時刻の UNIX の秒（見込みが無ければ None）
    pub start_at: Option<i64>,
    /// 見積もりの分（無制限か、パーティションの上限に任せたものは None）
    pub limit_minutes: Option<u64>,
}

/// 待ち行列を読めなかった理由
#[derive(Debug)]
pub enum QueueError {
    /// コマンドを起動できない。code は sh と同じく、無いコマンドは 127、起動できないコマンドは 126
    Spawn { code: i32, error: std::io::Error },
    /// コマンドが失敗した（その終了コード）
    Failed(i32),
    /// 出力を解けない
    Unreadable(String),
}

/// 待ち行列のジョブ（squeue の既定の絞り込み）。squeue の標準エラーはそのまま流す
pub fn queued_jobs() -> Result<Vec<QueuedJob>, QueueError> {
    let out = Command::new("squeue")
        .arg(JSON)
        .stderr(Stdio::inherit())
        .output()
        .map_err(|error| QueueError::Spawn {
            code: if error.kind() == std::io::ErrorKind::NotFound {
                127
            } else {
                126
            },
            error,
        })?;
    if !out.status.success() {
        return Err(QueueError::Failed(crate::child::exit_code(out.status)));
    }
    queue_jobs(&String::from_utf8_lossy(&out.stdout)).map_err(QueueError::Unreadable)
}

fn queue_jobs(json: &str) -> Result<Vec<QueuedJob>, String> {
    json_jobs("squeue", json)?.iter().map(queued_job).collect()
}

/// Slurm の C の値で「無い」と「無制限」を表す数（slurm/slurm.h の NO_VAL、INFINITE、NO_VAL64、INFINITE64）
const NO_VAL: u64 = 0xffff_fffe;
const INFINITE: u64 = 0xffff_ffff;
const NO_VAL64: u64 = 0xffff_ffff_ffff_fffe;
const INFINITE64: u64 = u64::MAX;

/// squeue の 1 本のジョブ。一覧の欄は、squeue の --format の文字の出力（26.05.6 の src/squeue/print.c の
/// `_print_job_*`。2026-11-19 に slurm-26-05-6-1 のタグで読んだ）と同じ形にする。そのため JSON の
/// `{set, infinite, number}` を C の値（NO_VAL、INFINITE を含む）に戻してから、print.c と同じ分け方で書く。
///
/// 文字の出力が表示の前に行う、待っている配列のタスクをまとめた記録に戻す処理（`_combine_pending_array_tasks`）と、
/// 番号の欄の 32 バイトでの切り詰めは行わない。slurm-run は配列のジョブを投げない（#361）
fn queued_job(job: &Value) -> Result<QueuedJob, String> {
    let job_id = job["job_id"]
        .as_u64()
        .ok_or("squeue の出力のジョブに数の job_id が無い")?;
    let missing = |key: &str| format!("squeue の出力のジョブ {job_id} の {key} を読めない");
    let number = |key: &str| c_number(&job[key], NO_VAL, INFINITE).ok_or_else(|| missing(key));
    // %s の文字列。C の NULL は JSON で空の文字列になり、squeue は glibc の printf の (null) を書く
    let printed = |key: &str| {
        job[key]
            .as_str()
            .map(|s| if s.is_empty() { "(null)" } else { s }.to_string())
            .ok_or_else(|| missing(key))
    };
    let tasks = job["array_task_string"]
        .as_str()
        .ok_or_else(|| missing("array_task_string"))?;
    let array_job = number("array_job_id")?;
    let array_task = number("array_task_id")?;
    let het_job = number("het_job_id")?;
    let (id, group) = if !tasks.is_empty() {
        (format!("{array_job}_[{tasks}]"), array_job)
    } else if array_task != NO_VAL {
        (format!("{array_job}_{array_task}"), array_job)
    } else if het_job != 0 {
        (format!("{het_job}+{}", number("het_job_offset")?), het_job)
    } else {
        (job_id.to_string(), job_id)
    };
    if job["job_state"][0].as_str().is_none() {
        return Err(missing("job_state"));
    }
    let state = state_word(&job["job_state"]).to_string();
    let phase = match state.as_str() {
        "PENDING" => Phase::Pending,
        "SUSPENDED" => Phase::Suspended,
        _ => Phase::Started,
    };
    let partition = printed("partition")?;
    let partition_kind = match partition.as_str() {
        UNINTERRUPTED => Partition::Uninterrupted,
        DEFAULT => Partition::Default,
        _ => Partition::Other,
    };
    let start =
        c_number(&job["start_time"], NO_VAL64, INFINITE64).ok_or_else(|| missing("start_time"))?;
    let time_limit = number("time_limit")?;
    Ok(QueuedJob {
        id,
        group,
        array: !tasks.is_empty(),
        partition,
        partition_kind,
        name: printed("name")?,
        state,
        phase,
        priority: number("priority")?,
        limit: limit_text(time_limit),
        // C の time_t に戻す。NO_VAL64 と INFINITE64 は -2 と -1 になり、slurm_make_time_str が比べるのは 32 ビットの
        // NO_VAL と INFINITE だけなので、squeue と同じく 1970 年の時刻で書く
        start: start_text(start as i64).ok_or_else(|| missing("start_time"))?,
        comment: printed("comment")?,
        start_at: start_time(start),
        limit_minutes: (time_limit != INFINITE && time_limit != NO_VAL).then_some(time_limit),
    })
}

/// 開始の時刻の C の値（[`start_text`] が時刻として書くもの）のうち、見込みとして使える UNIX の秒。0（N/A）、
/// INFINITE（Unknown）、NO_VAL（None）と、32 ビットを超える値（NO_VAL64 と INFINITE64 の 1970 年の時刻）は見込みが無い
fn start_time(start: u64) -> Option<i64> {
    (start != 0 && start < NO_VAL).then_some(start as i64)
}

/// `{set, infinite, number}` を C の値に（data_parser の UINT32_NO_VAL と UINT64_NO_VAL の DUMP の逆）
fn c_number(value: &Value, no_val: u64, infinite: u64) -> Option<u64> {
    if value["infinite"].as_bool()? {
        Some(infinite)
    } else if value["set"].as_bool()? {
        value["number"].as_u64()
    } else {
        Some(no_val)
    }
}

/// 見積もり（分）。print.c の `_print_job_time_limit` と `_print_secs`
fn limit_text(minutes: u64) -> String {
    match minutes {
        INFINITE => "UNLIMITED".to_string(),
        NO_VAL => "NOT_SET".to_string(),
        // C は uint32_t のまま 60 を掛ける
        _ => secs_text((minutes as u32).wrapping_mul(60)),
    }
}

fn secs_text(secs: u32) -> String {
    const YEAR_SECONDS: u32 = 365 * 86_400;
    let (days, hours, minutes, seconds) =
        (secs / 86_400, secs / 3600 % 24, secs / 60 % 60, secs % 60);
    if secs > YEAR_SECONDS {
        "INVALID".to_string()
    } else if days > 0 {
        format!("{days}-{hours:02}:{minutes:02}:{seconds:02}")
    } else if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

/// 時刻（UNIX の秒）。print.c の `_print_time` と src/common/parse_time.c の `slurm_make_time_str`（0 は N/A、
/// INFINITE は Unknown、NO_VAL は None、ほかは %FT%T）。範囲の外なら None
///
/// slurm_make_time_str は機械の時間帯（localtime_r）と SLURM_TIME_FORMAT で書くが、ここは JST の %FT%T に
/// 固定する。r995 の時間帯は Asia/Tokyo で、SLURM_TIME_FORMAT は設定していない（2026-11-19 に確かめた）。機械の
/// 時間帯を読むには chrono を足すか localtime_r を unsafe で呼ぶことになり、どちらも一覧の 1 欄のためには重い
fn start_text(time: i64) -> Option<String> {
    match time {
        0 => Some("N/A".to_string()),
        t if t == INFINITE as i64 => Some("Unknown".to_string()),
        t if t == NO_VAL as i64 => Some("None".to_string()),
        t => {
            let (y, mo, d, h, mi, s, _) = super::time::jst_fields(super::time::Time {
                micros: t.checked_mul(1_000_000)?,
            });
            Some(format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}"))
        }
    }
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
    /// 終わりを確かめられなかった状態
    pub fn unknown() -> JobState {
        JobState {
            name: "unknown".to_string(),
            ended: false,
            completed: false,
        }
    }

    /// Slurm の State（`CANCELLED by 1000` のように後ろが付くものは最初の語で見る）から
    pub fn from_slurm(state: &str) -> JobState {
        let name = state.split(' ').next().unwrap_or("");
        JobState {
            name: name.to_string(),
            ended: is_final(name),
            completed: name == "COMPLETED",
        }
    }
}

/// 文字の出力（sacct の State、scontrol の JobState）が、土台の状態より先に出す旗。並びは Slurm の `job_state_string`
/// （26.05.6 の `src/common/slurm_protocol_defs.c`。2026-11-19 に slurm-26-05-6-1 のタグで読んだ）の、旗を見る順
const STATE_FLAGS: &[&str] = &[
    "COMPLETING",
    "STAGE_OUT",
    "CONFIGURING",
    "EXPEDITING",
    "RESIZING",
    "REQUEUED",
    "REQUEUE_FED",
    "REQUEUE_HOLD",
    "SPECIAL_EXIT",
    "STOPPED",
    "REVOKED",
    "RESV_DEL_HOLD",
    "SIGNALING",
];

/// JSON の状態の配列（土台の状態が先で、旗が後ろ）から、文字の出力と同じ 1 語を作る。旗があればその語、無ければ土台の状態。
/// 先頭だけを読むと、終わりかけのジョブ（`["COMPLETED", "COMPLETING"]`）が終わったことになる
fn state_word(states: &Value) -> &str {
    let has = |flag: &str| {
        states
            .as_array()
            .is_some_and(|all| all.iter().any(|s| s.as_str() == Some(flag)))
    };
    STATE_FLAGS
        .iter()
        .copied()
        .find(|flag| has(flag))
        .or(states[0].as_str())
        .unwrap_or("")
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
    /// ジョブの名前（記録に無ければ空）
    pub name: String,
    /// ログのパス（ジョブの走るホストの書き方。記録に無ければ空）
    pub log: String,
    /// ジョブの終了コード（[`exit_status`]。記録に無ければ None）
    pub exit: Option<i32>,
}

/// 会計の記録（sacct）。読めない、記録がまだ届いていないなら None。標準エラーは捨てる（読めないことは呼ぶ側が警告する）
pub fn accounting_record(job: &str) -> Option<JobRecord> {
    sacct_record(&quiet_output(&["sacct", JSON, "-X", "-j", job])?, job)
}

fn sacct_record(json: &str, job: &str) -> Option<JobRecord> {
    let jobs = json_jobs("sacct", json).ok()?;
    let found = jobs.iter().find(|j| id_text(&j["job_id"]) == job)?;
    let time = &found["time"];
    let limit = &time["limit"];
    Some(JobRecord {
        elapsed: time["elapsed"].as_i64(),
        limit: if limit["infinite"].as_bool() == Some(true) {
            Some(Limit::Unlimited)
        } else if limit["set"].as_bool() == Some(false) {
            Some(Limit::Unset)
        } else {
            limit["number"].as_i64().map(Limit::Minutes)
        },
        uninterrupted: found["partition"]
            .as_str()
            .filter(|p| !p.is_empty())
            .map(|p| p == UNINTERRUPTED),
        suspended: time["suspended"].as_i64(),
        state: JobState::from_slurm(state_word(&found["state"]["current"])),
        name: found["name"].as_str().unwrap_or("").to_string(),
        log: found["stdout_expanded"].as_str().unwrap_or("").to_string(),
        exit: exit_status(&found["exit_code"]),
    })
}

/// ジョブの終了コードを、sbatch --wait が返す形で（26.05.6 の src/sbatch/sbatch.c の `_job_wait`。2026-11-20 に
/// slurm-26-05-6-1 のタグで読んだ）。普通に終わればその終了コード、シグナルで終われば 1。記録に無ければ None
fn exit_status(exit_code: &Value) -> Option<i32> {
    if exit_code["signal"]["id"]["set"].as_bool() == Some(true)
        && exit_code["signal"]["id"]["number"].as_u64() != Some(0)
    {
        return Some(1);
    }
    let code = &exit_code["return_code"];
    if code["set"].as_bool() != Some(true) {
        return None;
    }
    code["number"].as_i64().and_then(|n| i32::try_from(n).ok())
}

/// 制御のデーモン（scontrol）が覚えている状態。終わった直後（MinJobAge の 300 秒の間）だけ読める
pub fn controller_state(job: &str) -> Option<JobState> {
    controller_job(job).map(|found| found.state)
}

/// 制御のデーモンが覚えているジョブ（待っているもの、走っているもの、終わった直後のもの）
#[derive(Debug, PartialEq)]
pub struct ControllerJob {
    pub name: String,
    /// 投げたときのコメント（会計の記録には残らない。r995 の AccountingStoreFlags は job_comment を含まない）
    pub comment: String,
    /// ログのパス（ジョブの走るホストの書き方）
    pub log: String,
    pub state: JobState,
    /// ジョブの終了コード（[`exit_status`]。終わる前は意味を持たない）
    pub exit: Option<i32>,
}

/// 制御のデーモンが覚えているジョブ。覚えていない、読めないなら None
pub fn controller_job(job: &str) -> Option<ControllerJob> {
    scontrol_job(&quiet_output(&["scontrol", "show", "job", job, JSON])?, job)
}

fn scontrol_job(json: &str, job: &str) -> Option<ControllerJob> {
    let jobs = json_jobs("scontrol", json).ok()?;
    let found = jobs.iter().find(|j| id_text(&j["job_id"]) == job)?;
    found["job_state"][0].as_str()?;
    let text = |key: &str| found[key].as_str().unwrap_or("").to_string();
    Some(ControllerJob {
        name: text("name"),
        comment: text("comment"),
        log: text("stdout_expanded"),
        state: JobState::from_slurm(state_word(&found["job_state"])),
        exit: exit_status(&found["exit_code"]),
    })
}

#[cfg(test)]
fn scontrol_state(json: &str, job: &str) -> Option<JobState> {
    scontrol_job(json, job).map(|found| found.state)
}

/// コマンドの JSON の出力の jobs の配列。読めなければ理由
fn json_jobs(command: &str, json: &str) -> Result<Vec<Value>, String> {
    let mut value: Value = serde_json::from_str(json)
        .map_err(|e| format!("{command} の出力を JSON として読めない: {e}"))?;
    match value.get_mut("jobs").map(Value::take) {
        Some(Value::Array(jobs)) => Ok(jobs),
        _ => Err(format!("{command} の出力に jobs が無い")),
    }
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
        JobRecord, JobState, Limit, Partition, Phase, QueuedJob, RunningJob, Submission, log_file,
        queue_jobs, running_jobs, sacct_record, scontrol_state, submit_args, submitted_job,
        valid_time_limit,
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
    fn the_estimate_is_minutes_or_h_mm_ss_or_days_hours() {
        for bad in ["25m", "1h", "1.5", "abc", "1:30", "30:00", ""] {
            assert!(!valid_time_limit(bad), "{bad}");
        }
        for good in ["90", "1:30:00", "1-00", "1-12:00", "2-12:00:00"] {
            assert!(valid_time_limit(good), "{good}");
        }
    }

    #[test]
    fn the_state_word_is_the_flag_before_the_base_state_like_the_text_output() {
        // 文字の出力（sacct の State、scontrol の JobState）は、旗（COMPLETING など）があればそれを、無ければ土台の状態を
        // 出す。JSON の配列は土台が先で旗が後ろなので、先頭だけを読むと、終わりかけのジョブが終わったことになる
        for (states, word) in [
            (json!(["COMPLETED"]), "COMPLETED"),
            (json!(["COMPLETED", "COMPLETING"]), "COMPLETING"),
            (json!(["RUNNING", "STOPPED"]), "STOPPED"),
            (json!(["CANCELLED", "REVOKED"]), "REVOKED"),
            (json!(["PENDING", "REQUEUED", "SPECIAL_EXIT"]), "REQUEUED"),
            (json!(["RUNNING", "FUTURE_FLAG"]), "RUNNING"),
            (json!([]), ""),
            (json!(null), ""),
        ] {
            assert_eq!(super::state_word(&states), word, "{states}");
        }
        let finishing = json!({"jobs": [{"job_id": 42, "job_state": ["COMPLETED", "COMPLETING"]}]});
        assert!(!scontrol_state(&finishing.to_string(), "42").unwrap().ended);
        let stopped = json!({"jobs": [{"job_id": 42, "job_state": ["RUNNING", "STOPPED"]}]});
        assert!(scontrol_state(&stopped.to_string(), "42").unwrap().ended);
    }

    #[test]
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
    fn sacct_json_gives_the_times_the_partition_kind_and_the_first_state() {
        let json = json!({"jobs": [{
            "job_id": 42,
            "name": "openmc-endfb70-1B-1D-2B",
            "partition": "main",
            "state": {"current": ["CANCELLED"], "reason": "None"},
            "stdout_expanded": "/home/u/github/slurm-logs/openmc-endfb70-1B-1D-2B-42.out",
            "exit_code": exit_code(0, None),
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
                name: "openmc-endfb70-1B-1D-2B".to_string(),
                log: "/home/u/github/slurm-logs/openmc-endfb70-1B-1D-2B-42.out".to_string(),
                exit: Some(0),
            })
        );
        assert_eq!(sacct_record(&json, "43"), None);
        // 記録が slurmdbd に届く前は jobs が空
        assert_eq!(sacct_record(r#"{"jobs": []}"#, "42"), None);
        assert_eq!(sacct_record("Invalid", "42"), None);
    }

    #[test]
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

    /// `{set, infinite, number}` の形の数
    fn set(number: u64) -> serde_json::Value {
        json!({"set": true, "infinite": false, "number": number})
    }

    fn infinite() -> serde_json::Value {
        json!({"set": false, "infinite": true, "number": 0})
    }

    fn unset() -> serde_json::Value {
        json!({"set": false, "infinite": false, "number": 0})
    }

    /// 2026-11-19 に r995 の `squeue --json=v0.0.45` が出したジョブ（961）の、読む欄。changes の欄で置き換える
    fn squeue_job(changes: serde_json::Value) -> serde_json::Value {
        let mut job = json!({
            "job_id": 961,
            "name": "openmc-endfb70-1B-1D-2B",
            "job_state": ["RUNNING"],
            "partition": "main",
            "priority": set(10000),
            "time_limit": set(175),
            "start_time": set(1795077936),
            "comment": "decision | #0131 | p | c",
            "array_job_id": set(0),
            "array_task_id": unset(),
            "array_task_string": "",
            "het_job_id": set(0),
            "het_job_offset": unset(),
        });
        for (key, value) in changes.as_object().unwrap() {
            job[key] = value.clone();
        }
        job
    }

    /// 1 本のジョブの待ち行列を読む
    fn queued(changes: serde_json::Value) -> Result<QueuedJob, String> {
        let json = json!({"jobs": [squeue_job(changes)]}).to_string();
        queue_jobs(&json).map(|mut jobs| jobs.remove(0))
    }

    #[test]
    fn squeue_json_gives_the_queued_job_with_the_fields_as_the_listing_shows_them() {
        assert_eq!(
            queued(json!({})),
            Ok(QueuedJob {
                id: "961".to_string(),
                group: 961,
                array: false,
                partition: "main".to_string(),
                partition_kind: Partition::Default,
                name: "openmc-endfb70-1B-1D-2B".to_string(),
                state: "RUNNING".to_string(),
                phase: Phase::Started,
                priority: 10000,
                limit: "2:55:00".to_string(),
                start: "2026-11-19T17:45:36".to_string(),
                comment: "decision | #0131 | p | c".to_string(),
                start_at: Some(1795077936),
                limit_minutes: Some(175),
            })
        );
        assert_eq!(queue_jobs(r#"{"jobs": []}"#), Ok(vec![]));
    }

    #[test]
    fn the_start_and_the_limit_are_numbers_only_when_they_are_estimates() {
        // 一覧が時刻として書かない値（N/A、Unknown、None）と、1970 年の時刻になる無い値と無制限は、見込みが無い
        for (start, at) in [
            (set(1795077936), Some(1795077936)),
            (set(0), None),
            (set(0xffff_ffff), None),
            (set(0xffff_fffe), None),
            (unset(), None),
            (infinite(), None),
        ] {
            assert_eq!(
                queued(json!({"start_time": start})).unwrap().start_at,
                at,
                "{start}"
            );
        }
        for (limit, minutes) in [
            (set(30), Some(30)),
            (set(0), Some(0)),
            (infinite(), None),
            (unset(), None),
        ] {
            assert_eq!(
                queued(json!({"time_limit": limit})).unwrap().limit_minutes,
                minutes,
                "{limit}"
            );
        }
    }

    #[test]
    fn the_estimate_in_minutes_rounds_seconds_up() {
        for (estimate, minutes) in [
            ("90", Some(90)),
            ("0", Some(0)),
            ("1:30:00", Some(90)),
            ("0:00:01", Some(1)),
            ("1-00", Some(1440)),
            ("1-12:30", Some(2190)),
            ("2-01:00:30", Some(2941)),
            ("25m", None),
            ("1:30", None),
            ("", None),
        ] {
            assert_eq!(super::estimate_minutes(estimate), minutes, "{estimate}");
        }
    }

    /// 会計の記録と制御のデーモンの exit_code（return_code と、シグナルの番号。シグナルで終わらなければ None）
    fn exit_code(code: u64, signal: Option<u64>) -> serde_json::Value {
        json!({
            "status": [if signal.is_some() { "SIGNALED" } else if code == 0 { "SUCCESS" } else { "ERROR" }],
            "return_code": set(code),
            "signal": {"id": signal.map_or_else(unset, set), "name": ""},
        })
    }

    #[test]
    fn the_exit_status_is_the_return_code_or_1_for_a_signal_like_sbatch_wait() {
        assert_eq!(super::exit_status(&exit_code(0, None)), Some(0));
        assert_eq!(super::exit_status(&exit_code(137, None)), Some(137));
        assert_eq!(super::exit_status(&exit_code(0, Some(9))), Some(1));
        assert_eq!(super::exit_status(&json!({"return_code": unset()})), None);
        assert_eq!(super::exit_status(&json!(null)), None);
    }

    #[test]
    fn scontrol_json_gives_the_name_comment_log_state_and_exit_of_the_job() {
        let json = json!({"jobs": [{
            "job_id": 962,
            "name": "openmc-0293",
            "comment": "decision | #0293 | p | c",
            "stdout_expanded": "/home/u/github/slurm-logs/openmc-0293-962.out",
            "job_state": ["RUNNING"],
            "exit_code": exit_code(0, None),
        }]})
        .to_string();
        assert_eq!(
            super::scontrol_job(&json, "962"),
            Some(super::ControllerJob {
                name: "openmc-0293".to_string(),
                comment: "decision | #0293 | p | c".to_string(),
                log: "/home/u/github/slurm-logs/openmc-0293-962.out".to_string(),
                state: JobState::from_slurm("RUNNING"),
                exit: Some(0),
            })
        );
        assert_eq!(super::scontrol_job(&json, "7"), None);
    }

    #[test]
    fn the_limit_is_written_like_squeue_in_minutes_hours_and_days() {
        for (limit, text) in [
            (set(1), "1:00"),
            (set(10), "10:00"),
            (set(59), "59:00"),
            (set(60), "1:00:00"),
            (set(175), "2:55:00"),
            (set(1440), "1-00:00:00"),
            (set(2 * 1440 + 61), "2-01:01:00"),
            (set(0), "0:00"),
            // 1 年を超える見積もりは、squeue と同じく INVALID
            (set(365 * 1440), "365-00:00:00"),
            (set(365 * 1440 + 1), "INVALID"),
            (infinite(), "UNLIMITED"),
            (unset(), "NOT_SET"),
        ] {
            assert_eq!(
                queued(json!({"time_limit": limit})).unwrap().limit,
                text,
                "{limit}"
            );
        }
    }

    #[test]
    fn the_start_is_the_local_time_or_na_when_there_is_no_estimate() {
        for (start, text) in [
            (set(1795077936), "2026-11-19T17:45:36"),
            (set(1767193199), "2025-12-31T23:59:59"),
            (set(1767193200), "2026-01-01T00:00:00"),
            (set(0), "N/A"),
            (set(0xffff_ffff), "Unknown"),
            (set(0xffff_fffe), "None"),
            (unset(), "1970-01-01T08:59:58"),
            (infinite(), "1970-01-01T08:59:59"),
        ] {
            assert_eq!(
                queued(json!({"start_time": start})).unwrap().start,
                text,
                "{start}"
            );
        }
    }

    #[test]
    fn array_and_heterogeneous_jobs_get_the_id_and_the_group_number_squeue_shows() {
        let parent =
            queued(json!({"job_id": 14, "array_job_id": set(12), "array_task_string": "1-3"}))
                .unwrap();
        assert_eq!(
            (parent.id.as_str(), parent.group, parent.array),
            ("12_[1-3]", 12, true)
        );
        let task = queued(json!({"job_id": 13, "array_job_id": set(12), "array_task_id": set(1)}))
            .unwrap();
        assert_eq!(
            (task.id.as_str(), task.group, task.array),
            ("12_1", 12, false)
        );
        let het =
            queued(json!({"job_id": 6, "het_job_id": set(5), "het_job_offset": set(1)})).unwrap();
        assert_eq!((het.id.as_str(), het.group, het.array), ("5+1", 5, false));
    }

    #[test]
    fn the_state_word_decides_the_phase_and_the_partition_name_its_kind() {
        for (states, word, phase) in [
            (json!(["PENDING"]), "PENDING", Phase::Pending),
            (json!(["SUSPENDED"]), "SUSPENDED", Phase::Suspended),
            (json!(["RUNNING"]), "RUNNING", Phase::Started),
            (
                json!(["RUNNING", "CONFIGURING"]),
                "CONFIGURING",
                Phase::Started,
            ),
            (
                json!(["COMPLETED", "COMPLETING"]),
                "COMPLETING",
                Phase::Started,
            ),
            // 一覧の語が旗なら、待っているジョブとして扱わない
            (json!(["PENDING", "REQUEUED"]), "REQUEUED", Phase::Started),
        ] {
            let job = queued(json!({"job_state": states})).unwrap();
            assert_eq!((job.state.as_str(), job.phase), (word, phase), "{states}");
        }
        for (name, kind) in [
            ("timed", Partition::Uninterrupted),
            ("main", Partition::Default),
            ("main,timed", Partition::Other),
            ("debug", Partition::Other),
        ] {
            assert_eq!(
                queued(json!({"partition": name})).unwrap().partition_kind,
                kind,
                "{name}"
            );
        }
    }

    #[test]
    fn the_priority_is_the_number_and_an_empty_comment_is_shown_as_null() {
        assert_eq!(queued(json!({"priority": set(0)})).unwrap().priority, 0);
        // squeue は C の %u と %s で書くので、無い値は NO_VAL と INFINITE の数、無い文字列は (null)
        assert_eq!(
            queued(json!({"priority": unset()})).unwrap().priority,
            0xffff_fffe
        );
        assert_eq!(
            queued(json!({"priority": infinite()})).unwrap().priority,
            0xffff_ffff
        );
        assert_eq!(queued(json!({"comment": ""})).unwrap().comment, "(null)");
    }

    #[test]
    fn a_job_without_a_state_is_an_error_like_scontrol() {
        // squeue の文字の出力は、どのジョブにも土台の状態の語を書く
        assert!(queued(json!({"job_state": []})).is_err());
        assert!(queued(json!({"job_state": [1]})).is_err());
    }

    #[test]
    fn an_unexpected_shape_is_an_error() {
        for bad in ["not json", "{}", "[]", r#""jobs""#, r#"{"jobs": {}}"#] {
            assert!(queue_jobs(bad).is_err(), "{bad}");
        }
        for changes in [
            json!({"job_id": "961"}),
            json!({"priority": 10000}),
            json!({"time_limit": null}),
            json!({"start_time": {"set": true}}),
            json!({"job_state": "RUNNING"}),
            json!({"comment": null}),
        ] {
            assert!(queued(changes.clone()).is_err(), "{changes}");
        }
    }

    #[test]
    fn the_log_file_is_the_name_and_the_number_and_sbatch_gets_the_same_form() {
        assert_eq!(log_file("xlc-2G", "42"), "xlc-2G-42.out");
        assert!(
            submit_args(&job(None, false))
                .contains(&"/home/u/github/slurm-logs/%x-%j.out".to_string())
        );
    }
}
