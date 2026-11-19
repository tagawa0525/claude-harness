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
    days.checked_mul(24)?
        .checked_add(hours)?
        .checked_mul(60)?
        .checked_add(minutes)?
        .checked_add(seconds.div_ceil(60))
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

/// 待ち行列のジョブを、timeout までに読む（hook が使う。slurmctld が応答しないと、hook の時間切れで判定を飛ばされる）。
/// 時間内に終わらなければ squeue を止めて Unreadable
pub fn queued_jobs_within(timeout: std::time::Duration) -> Result<Vec<QueuedJob>, QueueError> {
    let mut child = Command::new("squeue")
        .arg(JSON)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        // 失敗の理由（slurmctld に届かない、など）を見せるため、queued_jobs と同じく流す
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|error| QueueError::Spawn {
            code: if error.kind() == std::io::ErrorKind::NotFound {
                127
            } else {
                126
            },
            error,
        })?;
    // 読まない間に squeue がパイプを満たして終われないので、別のスレッドで読む
    let mut pipe = child.stdout.take().expect("stdout はパイプ");
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut pipe, &mut bytes).map(|_| bytes)
    });
    let deadline = std::time::Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(20))
            }
            waited => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(match waited {
                    Err(e) => format!("squeue の終わりを受けられない: {e}"),
                    _ => format!("squeue が {} 秒で終わらない", timeout.as_secs()),
                });
            }
        }
    };
    let bytes = reader.join().ok().and_then(Result::ok).unwrap_or_default();
    let status = status.map_err(QueueError::Unreadable)?;
    if !status.success() {
        return Err(QueueError::Failed(crate::child::exit_code(status)));
    }
    queue_jobs(&String::from_utf8_lossy(&bytes)).map_err(QueueError::Unreadable)
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
        t => clock_text(t),
    }
}

/// 時刻（UNIX の秒）を JST の %FT%T で。範囲の外なら None
pub fn clock_text(time: i64) -> Option<String> {
    let (y, mo, d, h, mi, s, _) = super::time::jst_fields(super::time::Time {
        micros: time.checked_mul(1_000_000)?,
    });
    Some(format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}"))
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
    controller_job(job).ok().flatten().map(|found| found.state)
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

/// 制御のデーモンのジョブを読めなかった理由
#[derive(Debug)]
pub enum ControllerError {
    /// scontrol を起動できない（読み直しても変わらない）
    Spawn(String),
    /// scontrol が失敗した、出力を読めない（一時の不調でありうる）
    Failed(String),
}

/// 制御のデーモンが覚えているジョブ。覚えていなければ（知らない番号と、終わってから MinJobAge を過ぎたものは、scontrol が
/// 空の jobs を 0 で返す）None。読めなければ理由（一時の失敗を、覚えていないと読み違えない）
pub fn controller_job(job: &str) -> Result<Option<ControllerJob>, ControllerError> {
    let out = Command::new("scontrol")
        .args(["show", "job", job, JSON])
        .stderr(Stdio::null())
        .output()
        .map_err(|e| ControllerError::Spawn(format!("scontrol を起動できない: {e}")))?;
    if !out.status.success() {
        return Err(ControllerError::Failed(format!(
            "scontrol が終了コード {} で失敗した",
            crate::child::exit_code(out.status)
        )));
    }
    scontrol_job(&String::from_utf8_lossy(&out.stdout), job).map_err(ControllerError::Failed)
}

fn scontrol_job(json: &str, job: &str) -> Result<Option<ControllerJob>, String> {
    let jobs = json_jobs("scontrol", json)?;
    let Some(found) = jobs.iter().find(|j| id_text(&j["job_id"]) == job) else {
        return Ok(None);
    };
    if found["job_state"][0].as_str().is_none() {
        return Err(format!("scontrol の出力のジョブ {job} に状態が無い"));
    }
    let text = |key: &str| found[key].as_str().unwrap_or("").to_string();
    Ok(Some(ControllerJob {
        name: text("name"),
        comment: text("comment"),
        log: text("stdout_expanded"),
        state: JobState::from_slurm(state_word(&found["job_state"])),
        exit: exit_status(&found["exit_code"]),
    }))
}

#[cfg(test)]
fn scontrol_state(json: &str, job: &str) -> Option<JobState> {
    scontrol_job(json, job)
        .ok()
        .flatten()
        .map(|found| found.state)
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

/// 終わったジョブの記録のファイル（jobcomp/filetxt）の置き場。r995 の JobCompLoc
pub const COMPLETIONS: &str = "/var/log/slurm/job_completions";

/// 投げたときの資源の取り方（コマンド行の sbatch の直後の語。slurm-run は排他のときだけそこに --exclusive を置き、-c の
/// ときは --cpus-per-task=N を置く。会計の記録の CPU の数は、-c 32 でも排他でも 32 なので見分けられない）
#[derive(Clone, Debug, PartialEq)]
pub enum Allocation {
    Exclusive,
    /// 排他にせず取った CPU の数（`--cpus-per-task=` の後ろのまま）
    Cpus(String),
    /// どちらでもない（slurm-run を通らずに投げた）。sbatch の直後の語（コマンド行に sbatch が無ければ `-`）
    Unknown(String),
}

/// 投げたときのコマンド行（sacct の SubmitLine）から読んだもの
#[derive(Debug, PartialEq)]
pub struct Submitted {
    pub allocation: Allocation,
    /// コメントの連絡先（slurm-run の「<区分> | <参照> | <目的> | <連絡先>」の連絡先。区分の無い古いコメントは全体）。
    /// コメントが無ければ空
    pub contact: String,
    /// --wrap のコマンド（無ければ None）
    pub command: Option<String>,
}

/// コマンド行から、資源の取り方、連絡先、コマンドを読む。参照と目的は ` | ` を含まないが「 --wrap 」を含みうるので、
/// 区分のあるコメントは 3 つの ` | ` の後から次の「 --wrap 」までを連絡先とする。自分の --comment の無い行では、
/// --wrap のコマンドの中の「 --comment 」を読まない
pub fn submitted(line: &str) -> Submitted {
    const COMMENT: &str = " --comment ";
    const WRAP: &str = " --wrap ";
    let (contact, command) = match line.split_once(COMMENT) {
        Some((before, mut comment)) if !before.contains(WRAP) => {
            let labelled = super::slurm_class::CLASSES
                .iter()
                .any(|class| comment.starts_with(&format!("{class} | ")));
            if labelled {
                for _ in 0..3 {
                    if let Some((_, rest)) = comment.split_once(" | ") {
                        comment = rest;
                    }
                }
            }
            match comment.split_once(WRAP) {
                Some((contact, command)) => (contact, Some(command)),
                None => (comment, None),
            }
        }
        _ => ("", line.split_once(WRAP).map(|(_, command)| command)),
    };
    Submitted {
        allocation: allocation(line),
        contact: contact.to_string(),
        command: command.map(str::to_string),
    }
}

fn allocation(line: &str) -> Allocation {
    let word = match line.split_once("sbatch ") {
        Some((_, rest)) => rest.split(char::is_whitespace).next().unwrap_or(""),
        None => "-",
    };
    if word == "--exclusive" {
        Allocation::Exclusive
    } else if let Some(cpus) = word.strip_prefix("--cpus-per-task=") {
        Allocation::Cpus(cpus.to_string())
    } else {
        Allocation::Unknown(word.to_string())
    }
}

/// 終わったジョブ（会計の記録では、待っているものと走っているものを含む）の、見積もりと実績を比べるための記録
#[derive(Debug, PartialEq)]
pub struct PastJob {
    pub id: String,
    pub name: String,
    /// 最後まで走って成功した（会計の記録は State が COMPLETED、ファイルはそれに加えてノードを 1 つ以上取った）
    pub completed: bool,
    /// 見積もりの分。無い、0、無制限、パーティションの上限に任せたものは None
    pub limit_minutes: Option<u64>,
    /// 走り始めた時刻（JST）を、時間帯を数えずに数えた秒（[`wall_clock`]）。記録に無いか読めなければ None
    pub start: Option<i64>,
    /// 終わった時刻（start と同じ形）
    pub end: Option<i64>,
    /// 割り込まれないパーティションで走ったか（パーティションが無ければ None）
    pub uninterrupted: Option<bool>,
    /// 投げたときの資源の取り方（終わったジョブの記録のファイルは持たないので None）
    pub allocation: Option<Allocation>,
}

/// 会計の記録の 1 行。並びは sacct の文字の出力と同じ（ジョブの行の後に、そのステップの行）
#[derive(Debug, PartialEq)]
pub enum AccountingRow {
    Job(PastJob),
    /// ジョブのステップ（.batch など）
    Step {
        job: String,
        max_rss: MaxRss,
    },
}

/// ステップのメモリの最大（MaxRSS）
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MaxRss {
    /// 記録に無い（sacct の文字の出力は空の欄）
    Missing,
    /// MiB。sacct の `--units=M` の文字の出力と同じく、整数でなければ小数 2 桁に丸めた値（数バイトは 0.0）
    Mib(f64),
}

/// 会計の記録を読めなかった理由
#[derive(Debug, PartialEq)]
pub enum AccountingError {
    /// sacct を起動できないか、失敗した（理由は sacct の標準エラーに出る）
    Failed,
    /// 出力を解けない
    Unreadable(String),
}

/// since（`YYYY-MM-DD`）以降の会計の記録の、ジョブとステップの行。sacct の標準エラーはそのまま流す
pub fn accounting_since(since: &str) -> Result<Vec<AccountingRow>, AccountingError> {
    let out = Command::new("sacct")
        .args([JSON, "-S", since])
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| {
            eprintln!("sacct を起動できない: {e}");
            AccountingError::Failed
        })?;
    if !out.status.success() {
        return Err(AccountingError::Failed);
    }
    accounting_rows(&String::from_utf8_lossy(&out.stdout)).map_err(AccountingError::Unreadable)
}

fn accounting_rows(json: &str) -> Result<Vec<AccountingRow>, String> {
    let mut rows = Vec::new();
    for job in json_jobs("sacct", json)? {
        let id = job["job_id"]
            .as_u64()
            .ok_or("sacct の出力のジョブに数の job_id が無い")?
            .to_string();
        let time = &job["time"];
        let limit = &time["limit"];
        let partition = job["partition"].as_str().unwrap_or("");
        rows.push(AccountingRow::Job(PastJob {
            id: id.clone(),
            name: job["name"].as_str().unwrap_or("").to_string(),
            completed: state_word(&job["state"]["current"]) == "COMPLETED",
            limit_minutes: (limit["set"].as_bool() == Some(true)
                && limit["infinite"].as_bool() != Some(true))
            .then(|| limit["number"].as_u64())
            .flatten()
            .filter(|m| *m > 0),
            start: moment(&time["start"]).at.map(|t| t + super::time::JST),
            end: moment(&time["end"]).at.map(|t| t + super::time::JST),
            uninterrupted: (!partition.is_empty()).then(|| partition == UNINTERRUPTED),
            allocation: Some(allocation(job["submit_line"].as_str().unwrap_or(""))),
        }));
        for step in job["steps"].as_array().into_iter().flatten() {
            rows.push(AccountingRow::Step {
                job: id.clone(),
                max_rss: max_rss(step),
            });
        }
    }
    Ok(rows)
}

/// ステップのメモリの最大の MiB。sacct の MaxRSS の文字の出力（26.05.6 の src/sacct/print.c の PRINT_MAXRSS と、
/// src/common/slurm_protocol_api.c の convert_num_unit2。2026-11-20 に slurm-26-05-6-1 のタグで読んだ）と同じく、
/// 割り切れれば整数、割り切れなければ小数 2 桁に丸める
fn max_rss(step: &Value) -> MaxRss {
    let bytes = step["tres"]["requested"]["max"]
        .as_array()
        .and_then(|all| all.iter().find(|tres| tres["type"].as_str() == Some("mem")))
        .and_then(|mem| mem["count"].as_u64());
    match bytes {
        // 記録が無い値（NO_VAL64、INFINITE64）は空の欄
        None => MaxRss::Missing,
        Some(b) if b >= NO_VAL64 => MaxRss::Missing,
        Some(b) => {
            let mib = b as f64 / 1024.0 / 1024.0;
            if mib.fract() == 0.0 {
                MaxRss::Mib(mib)
            } else {
                format!("{mib:.2}")
                    .parse()
                    .map_or(MaxRss::Missing, MaxRss::Mib)
            }
        }
    }
}

/// 終わったジョブの記録のファイルの 1 行
#[derive(Debug, PartialEq)]
pub enum CompletionLine {
    /// 空の行
    Blank,
    /// JobId の無い行（理由）
    Unreadable(&'static str),
    /// JobId のある行。ほかの欄が無いか壊れていれば、その理由
    Job {
        id: String,
        record: Result<PastJob, &'static str>,
    },
}

/// 終わったジョブの記録のファイルの 1 行を読む。行は `キー=値` の空白区切りで、値に `=` や `,` を含む欄（Tres）が
/// あるので最初の `=` で分ける。`=` を含まない語は直前の値の続き（空白のあるジョブ名）として読む
pub fn completion_line(line: &str) -> CompletionLine {
    let mut fields: std::collections::HashMap<&str, String> = std::collections::HashMap::new();
    let mut key: Option<&str> = None;
    let mut words = line.split([' ', '\t']).filter(|w| !w.is_empty()).peekable();
    if words.peek().is_none() {
        return CompletionLine::Blank;
    }
    for word in words {
        if let Some((k, v)) = word.split_once('=') {
            fields.insert(k, v.to_string());
            key = (!k.is_empty()).then_some(k);
        } else if let Some(value) = key.and_then(|k| fields.get_mut(k)) {
            value.push(' ');
            value.push_str(word);
        }
    }
    let Some(id) = fields.get("JobId") else {
        return CompletionLine::Unreadable("JobId がありません");
    };
    let text = |k: &str| fields.get(k).map(String::as_str);
    let record = (|| {
        let state = text("JobState").ok_or("JobState がありません")?;
        let limit = text("TimeLimit").ok_or("TimeLimit がありません")?;
        let nodes = text("NodeCnt").unwrap_or("");
        if nodes.is_empty() || !nodes.bytes().all(|b| b.is_ascii_digit()) {
            return Err("NodeCnt が数値ではありません");
        }
        let partition = text("Partition").unwrap_or("");
        Ok(PastJob {
            id: id.clone(),
            name: text("Name").unwrap_or("").to_string(),
            completed: state == "COMPLETED" && nodes.bytes().any(|b| b != b'0'),
            limit_minutes: (!limit.is_empty() && limit.bytes().all(|b| b.is_ascii_digit()))
                .then(|| limit.parse().ok())
                .flatten()
                .filter(|m| *m > 0),
            start: wall_clock(text("StartTime").unwrap_or("")),
            end: wall_clock(text("EndTime").unwrap_or("")),
            uninterrupted: (!partition.is_empty()).then(|| partition == UNINTERRUPTED),
            allocation: None,
        })
    })();
    CompletionLine::Job {
        id: id.clone(),
        record,
    }
}

/// `YYYY-MM-DDTHH:MM:SS` の時刻を、時間帯を数えずに 1970-01-01T00:00:00 から数えた秒に。月は 1〜12、日は
/// 1〜31、時は 23、分と秒は 59 まで（月の日数は見ない）。形が違えば None
pub fn wall_clock(text: &str) -> Option<i64> {
    let b = text.as_bytes();
    let shape = b.len() == 19
        && b.iter().enumerate().all(|(i, c)| match i {
            4 | 7 => *c == b'-',
            10 => *c == b'T',
            13 | 16 => *c == b':',
            _ => c.is_ascii_digit(),
        });
    if !shape {
        return None;
    }
    let num = |from: usize, len: usize| text[from..from + len].parse::<i64>().ok();
    let (y, mo, d, h, mi, s) = (
        num(0, 4)?,
        num(5, 2)?,
        num(8, 2)?,
        num(11, 2)?,
        num(14, 2)?,
        num(17, 2)?,
    );
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || s > 59 {
        return None;
    }
    Some(super::time::days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 + s)
}

/// 会計の記録の時刻
#[derive(Clone, Debug, PartialEq)]
pub struct Moment {
    /// 出力に書く形（sacct の文字の出力と同じ。JST の `%FT%T`、無い時刻は `Unknown` か `None`）
    pub text: String,
    /// UNIX の秒（無い時刻は None）
    pub at: Option<i64>,
}

/// JSON の時刻（UNIX の秒）を、sacct の文字の出力（src/common/parse_time.c の `_make_time_str_internal`）の形と
/// 合わせて。0 と INFINITE は Unknown、NO_VAL は None
fn moment(value: &Value) -> Moment {
    match value.as_i64() {
        None | Some(0) => Moment {
            text: "Unknown".to_string(),
            at: None,
        },
        Some(t) if t == INFINITE as i64 => Moment {
            text: "Unknown".to_string(),
            at: None,
        },
        Some(t) if t == NO_VAL as i64 => Moment {
            text: "None".to_string(),
            at: None,
        },
        Some(t) => Moment {
            text: clock_text(t).unwrap_or_default(),
            at: Some(t),
        },
    }
}

/// 経過の秒を、sacct の Elapsed の文字の出力（src/common/parse_time.c の `secs2time_str`）の形で
fn elapsed_text(secs: u64) -> String {
    if secs == INFINITE {
        return "UNLIMITED".to_string();
    }
    let (days, hours, minutes, seconds) =
        (secs / 86_400, secs / 3600 % 24, secs / 60 % 60, secs % 60);
    if days > 0 {
        format!("{days}-{hours:02}:{minutes:02}:{seconds:02}")
    } else {
        format!("{hours:02}:{minutes:02}:{seconds:02}")
    }
}

/// 会計の記録の 1 本のジョブ。文字列の欄は、出力に書く形（sacct の文字の出力と同じ）
#[derive(Debug, PartialEq)]
pub struct AccountedJob {
    pub id: String,
    pub name: String,
    /// sacct の State の文字の出力のまま（取り消したジョブは `CANCELLED by <uid>`）
    pub state: String,
    pub elapsed: String,
    pub submit: Moment,
    pub start: Moment,
    pub end: Moment,
    pub workdir: String,
    pub submitted: Submitted,
}

/// 会計の記録の、番号 job のジョブ（ステップを除く）。記録が無ければ None。sacct の標準エラーはそのまま流す。
///
/// 状態だけは文字の出力（`-o State`）で読む。取り消したジョブの文字の出力は `CANCELLED by <取り消した uid>` で、JSON は
/// 取り消した利用者を名前（`kill_request_user`。26.05.6 の data_parser の v0.0.45 の parsers.c の DUMP_FUNC(USER_ID)）で
/// しか持たず、uid の数を持たない
pub fn accounted_job(job: &str) -> Result<Option<AccountedJob>, AccountingError> {
    let run = |args: &[&str]| -> Result<String, AccountingError> {
        let out = Command::new("sacct")
            .args(args)
            .stderr(Stdio::inherit())
            .output()
            .map_err(|e| {
                eprintln!("sacct を起動できない: {e}");
                AccountingError::Failed
            })?;
        if !out.status.success() {
            return Err(AccountingError::Failed);
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    };
    let json = run(&[JSON, "-X", "-j", job])?;
    let Some(mut found) = accounted_job_json(&json, "").map_err(AccountingError::Unreadable)?
    else {
        return Ok(None);
    };
    // 記録のあるジョブだけ、状態を読み足す
    let state = run(&["-j", job, "-X", "-P", "-n", "-o", "State"])?;
    found.state = state.lines().next().unwrap_or("").to_string();
    Ok(Some(found))
}

fn accounted_job_json(json: &str, state: &str) -> Result<Option<AccountedJob>, String> {
    let jobs = json_jobs("sacct", json)?;
    let found = match jobs.as_slice() {
        [] => return Ok(None),
        [found] => found,
        _ => return Err(format!("記録が {} 件ある", jobs.len())),
    };
    let time = &found["time"];
    let text = |key: &str| found[key].as_str().unwrap_or("").to_string();
    Ok(Some(AccountedJob {
        id: id_text(&found["job_id"]),
        name: text("name"),
        state: state.to_string(),
        elapsed: time["elapsed"]
            .as_u64()
            .map(elapsed_text)
            .unwrap_or_default(),
        submit: moment(&time["submission"]),
        start: moment(&time["start"]),
        end: moment(&time["end"]),
        workdir: text("working_directory"),
        submitted: submitted(found["submit_line"].as_str().unwrap_or("")),
    }))
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
            // 数として大きすぎるものは読めない（桁あふれで小さな見積もりにしない）
            ("20000000000000000-00", None),
            ("99999999999999999999", None),
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
            Ok(Some(super::ControllerJob {
                name: "openmc-0293".to_string(),
                comment: "decision | #0293 | p | c".to_string(),
                log: "/home/u/github/slurm-logs/openmc-0293-962.out".to_string(),
                state: JobState::from_slurm("RUNNING"),
                exit: Some(0),
            }))
        );
        assert_eq!(super::scontrol_job(&json, "7"), Ok(None));
        assert!(super::scontrol_job("not json", "962").is_err());
        assert!(super::scontrol_job(r#"{"jobs": [{"job_id": 962}]}"#, "962").is_err());
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

    /// slurm-run が投げた形のコマンド行（資源の引数、コメント、コマンド）
    fn submit_line(resources: &str, comment: &str, command: &str) -> String {
        format!(
            "/nix/store/x-slurm/bin/sbatch {resources} --partition=main --wait --export=ALL -J j -o /logs/%x-%j.out --time 10 --comment {comment} --wrap {command}"
        )
    }

    #[test]
    fn the_submit_line_gives_the_allocation_the_contact_after_the_labels_and_the_command() {
        assert_eq!(
            super::submitted(&submit_line(
                "--exclusive",
                "decision | #1 | 結果で方式を決める | feat/x /w commit=abc claude=s",
                "lattice input.toml > out.txt"
            )),
            super::Submitted {
                allocation: super::Allocation::Exclusive,
                contact: "feat/x /w commit=abc claude=s".to_string(),
                command: Some("lattice input.toml > out.txt".to_string()),
            }
        );
        let shared = super::submitted(&submit_line("--cpus-per-task=8", "trial | - | p | c", "x"));
        assert_eq!(shared.allocation, super::Allocation::Cpus("8".to_string()));
    }

    #[test]
    fn a_purpose_with_wrap_does_not_cut_the_contact_and_an_old_comment_is_all_contact() {
        // 参照と目的は | を含まないが「 --wrap 」を含みうるので、区分のあるコメントは 3 つの | の後から読む
        let purpose = super::submitted(&submit_line(
            "--exclusive",
            "decision | #1 | --wrap の扱いで方式を決める | feat/x /w",
            "lattice input.toml",
        ));
        assert_eq!(
            (purpose.contact.as_str(), purpose.command.as_deref()),
            ("feat/x /w", Some("lattice input.toml"))
        );
        // 区分の無い古いコメントは、全体が連絡先
        let old = super::submitted(
            "/x/sbatch --cpus-per-task=8 --wait -J j --time 10 --comment feat/x /w claude=s shared-cpus=8 --wrap lattice input.toml",
        );
        assert_eq!(
            (old.contact.as_str(), old.command.as_deref()),
            (
                "feat/x /w claude=s shared-cpus=8",
                Some("lattice input.toml")
            )
        );
    }

    #[test]
    fn a_comment_inside_the_wrapped_command_is_not_the_contact() {
        let line =
            super::submitted("/x/sbatch --exclusive --wrap tool --comment feat/y /x commit=abc");
        assert_eq!(
            line,
            super::Submitted {
                allocation: super::Allocation::Exclusive,
                contact: String::new(),
                command: Some("tool --comment feat/y /x commit=abc".to_string()),
            }
        );
    }

    #[test]
    fn a_submit_line_not_from_slurm_run_has_an_unknown_allocation_and_may_lack_the_command() {
        assert_eq!(
            super::submitted("/usr/bin/sbatch --wrap run").allocation,
            super::Allocation::Unknown("--wrap".to_string())
        );
        let script = super::submitted("salloc job.sh");
        assert_eq!(
            script.allocation,
            super::Allocation::Unknown("-".to_string())
        );
        assert_eq!(script.command, None);
        assert_eq!(script.contact, "");
        // 改行でも語は切れる
        assert_eq!(
            super::submitted("/x/sbatch --exclusive\n--wrap x").allocation,
            super::Allocation::Exclusive
        );
    }

    /// 2026-11-20 に r995 の `sacct --json=v0.0.45 -S 2026-10-12` が出したジョブの、読む欄（changes で置き換える）と、
    /// ステップのメモリの最大（バイト。None なら mem の欄が無い）
    fn sacct_job(changes: serde_json::Value, steps: &[(&str, Option<u64>)]) -> serde_json::Value {
        let steps: Vec<_> = steps
            .iter()
            .map(|(id, bytes)| {
                let max: Vec<_> = bytes
                    .iter()
                    .map(|b| json!({"type": "mem", "name": "", "id": 2, "count": b, "task": 0, "node": "r995"}))
                    .chain([json!({"type": "cpu", "name": "", "id": 1, "count": 0, "task": 0, "node": "r995"})])
                    .collect();
                json!({"step": {"id": id, "name": "batch"}, "tres": {"requested": {"max": max}}})
            })
            .collect();
        let mut job = json!({
            "job_id": 128,
            "name": "xlc-2G",
            "partition": "timed",
            "state": {"current": ["COMPLETED"], "reason": "None"},
            "time": {"start": 1795077405, "end": 1795077870, "limit": set(60)},
            "submit_line": "/nix/store/x/bin/sbatch --exclusive --partition=timed --wrap run",
            "steps": steps,
        });
        for (key, value) in changes.as_object().unwrap() {
            job[key] = value.clone();
        }
        job
    }

    fn rows(jobs: &[serde_json::Value]) -> Vec<super::AccountingRow> {
        super::accounting_rows(&json!({"jobs": jobs}).to_string()).unwrap()
    }

    #[test]
    fn sacct_json_gives_each_job_row_followed_by_its_step_rows_like_the_text_output() {
        use super::{AccountingRow, Allocation, PastJob};
        assert_eq!(
            rows(&[sacct_job(
                json!({}),
                &[("128.batch", Some(1_861_193_728)), ("128.extern", None)]
            )]),
            vec![
                AccountingRow::Job(PastJob {
                    id: "128".to_string(),
                    name: "xlc-2G".to_string(),
                    completed: true,
                    limit_minutes: Some(60),
                    // 2026-11-19T17:36:45 と 2026-11-19T17:44:30（JST）
                    start: super::wall_clock("2026-11-19T17:36:45"),
                    end: super::wall_clock("2026-11-19T17:44:30"),
                    uninterrupted: Some(true),
                    allocation: Some(Allocation::Exclusive),
                }),
                AccountingRow::Step {
                    job: "128".to_string(),
                    max_rss: super::MaxRss::Mib(1774.97),
                },
                AccountingRow::Step {
                    job: "128".to_string(),
                    max_rss: super::MaxRss::Missing,
                },
            ]
        );
    }

    #[test]
    fn the_max_rss_is_mib_rounded_to_2_decimals_unless_whole_like_sacct_units_m() {
        use super::MaxRss;
        // sacct --units=M の文字の出力（1861193728 バイトは 1774.97M、1 GiB は 1024M、4096 バイトは 0.00M、0 バイトは単位の無い 0）
        for (bytes, mib) in [
            (Some(1_861_193_728), MaxRss::Mib(1774.97)),
            (Some(8_990_720), MaxRss::Mib(8.57)),
            (Some(1 << 30), MaxRss::Mib(1024.0)),
            (Some(4096), MaxRss::Mib(0.0)),
            (Some(0), MaxRss::Mib(0.0)),
            (Some(0xffff_ffff_ffff_fffe), MaxRss::Missing),
            (Some(u64::MAX), MaxRss::Missing),
            (None, MaxRss::Missing),
        ] {
            let got = rows(&[sacct_job(json!({}), &[("1.batch", bytes)])]);
            assert_eq!(
                got[1],
                super::AccountingRow::Step {
                    job: "128".to_string(),
                    max_rss: mib
                },
                "{bytes:?}"
            );
        }
    }

    #[test]
    fn sacct_json_reads_the_limit_times_partition_state_and_allocation_of_the_job() {
        let job = |changes: serde_json::Value| match rows(&[sacct_job(changes, &[])]).remove(0) {
            super::AccountingRow::Job(job) => job,
            row => panic!("{row:?}"),
        };
        for (limit, minutes) in [
            (set(45), Some(45)),
            (set(0), None),
            (infinite(), None),
            (unset(), None),
        ] {
            assert_eq!(
                job(json!({"time": {"start": 1, "end": 2, "limit": limit}})).limit_minutes,
                minutes,
                "{limit}"
            );
        }
        // 文字の出力が Unknown（0 と INFINITE）か None（NO_VAL）と書く時刻は、時刻として読めない
        for start in [0u64, 0xffff_ffff, 0xffff_fffe] {
            assert_eq!(
                job(json!({"time": {"start": start, "end": 2, "limit": set(1)}})).start,
                None,
                "{start}"
            );
        }
        assert_eq!(job(json!({"partition": ""})).uninterrupted, None);
        assert_eq!(job(json!({"partition": "main"})).uninterrupted, Some(false));
        // 終わりかけ（COMPLETING の旗）は、文字の出力でも COMPLETED でない
        assert!(!job(json!({"state": {"current": ["COMPLETED", "COMPLETING"]}})).completed);
        assert!(!job(json!({"state": {"current": ["CANCELLED"]}})).completed);
        assert_eq!(
            job(json!({"submit_line": "/x/sbatch --cpus-per-task=4 --wrap run"})).allocation,
            Some(super::Allocation::Cpus("4".to_string()))
        );
    }

    #[test]
    fn sacct_json_without_jobs_or_a_job_number_is_unreadable() {
        assert!(super::accounting_rows("not json").is_err());
        assert!(super::accounting_rows("{}").is_err());
        assert!(super::accounting_rows(&json!({"jobs": [{"name": "x"}]}).to_string()).is_err());
        assert_eq!(super::accounting_rows(r#"{"jobs": []}"#), Ok(vec![]));
    }

    /// jobcomp/filetxt の 1 行（r995 の /var/log/slurm/job_completions の形）
    fn jobcomp(
        id: &str,
        name: &str,
        state: &str,
        limit: &str,
        start: &str,
        end: &str,
        nodes: &str,
    ) -> String {
        format!(
            "JobId={id} UserId=tagawa(1000) GroupId=users(100) Name={name} JobState={state} Partition=main TimeLimit={limit} StartTime={start} EndTime={end} NodeList=r995 NodeCnt={nodes} ProcCnt=32 WorkDir=/w ReservationName= Tres=cpu=1,mem=86000M,node=1,billing=1 Account= QOS= WcKey= Cluster=unknown SubmitTime=2026-10-11T12:00:00 EligibleTime=2026-10-11T12:00:00 DerivedExitCode=0:0 ExitCode=0:0 "
        )
    }

    fn completion(line: &str) -> super::PastJob {
        match super::completion_line(line) {
            super::CompletionLine::Job { record, .. } => record.unwrap(),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_jobcomp_line_gives_the_job_with_its_times_limit_and_partition() {
        assert_eq!(
            completion(&jobcomp(
                "73",
                "openmc-2A",
                "COMPLETED",
                "45",
                "2026-10-11T13:45:32",
                "2026-10-11T14:24:05",
                "1"
            )),
            super::PastJob {
                id: "73".to_string(),
                name: "openmc-2A".to_string(),
                completed: true,
                limit_minutes: Some(45),
                start: super::wall_clock("2026-10-11T13:45:32"),
                end: super::wall_clock("2026-10-11T14:24:05"),
                uninterrupted: Some(false),
                allocation: None,
            }
        );
        let timed =
            completion("JobId=1 Name=a JobState=COMPLETED Partition=timed TimeLimit=10 NodeCnt=1");
        assert_eq!(timed.uninterrupted, Some(true));
        assert_eq!((timed.start, timed.end), (None, None));
    }

    #[test]
    fn a_jobcomp_name_with_spaces_continues_the_value_and_values_split_at_the_first_equals() {
        let job = completion(&jobcomp(
            "8",
            "my job",
            "COMPLETED",
            "10",
            "2026-10-11T10:00:00",
            "2026-10-11T10:05:00",
            "1",
        ));
        assert_eq!(job.name, "my job");
        assert_eq!(job.limit_minutes, Some(10));
    }

    #[test]
    fn only_completed_jobcomp_lines_with_a_node_have_run_to_the_end() {
        let at = |state: &str, nodes: &str| {
            completion(&jobcomp(
                "1",
                "a",
                state,
                "10",
                "2026-10-11T10:00:00",
                "2026-10-11T10:05:00",
                nodes,
            ))
            .completed
        };
        assert!(at("COMPLETED", "1"));
        assert!(at("COMPLETED", "02"));
        assert!(!at("COMPLETED", "0"));
        assert!(!at("FAILED", "1"));
        assert!(!at("TIMEOUT", "1"));
        assert!(!at("CANCELLED", "0"));
    }

    #[test]
    fn a_jobcomp_limit_that_is_not_a_positive_number_is_no_estimate() {
        for limit in ["UNLIMITED", "0", "Partition_Limit", ""] {
            assert_eq!(
                completion(&format!(
                    "JobId=1 JobState=COMPLETED TimeLimit={limit} NodeCnt=1"
                ))
                .limit_minutes,
                None,
                "{limit}"
            );
        }
    }

    #[test]
    fn a_jobcomp_line_without_an_id_state_limit_or_node_count_is_unreadable() {
        use super::CompletionLine;
        assert_eq!(super::completion_line(""), CompletionLine::Blank);
        assert_eq!(super::completion_line(" \t "), CompletionLine::Blank);
        assert_eq!(
            super::completion_line("Name=x JobState=COMPLETED"),
            CompletionLine::Unreadable("JobId がありません")
        );
        for (line, why) in [
            ("JobId=2 TimeLimit=10 NodeCnt=1", "JobState がありません"),
            (
                "JobId=2 JobState=COMPLETED NodeCnt=1",
                "TimeLimit がありません",
            ),
            (
                "JobId=2 JobState=COMPLETED TimeLimit=10",
                "NodeCnt が数値ではありません",
            ),
            (
                "JobId=2 JobState=COMPLETED TimeLimit=10 NodeCnt=x",
                "NodeCnt が数値ではありません",
            ),
        ] {
            assert_eq!(
                super::completion_line(line),
                CompletionLine::Job {
                    id: "2".to_string(),
                    record: Err(why)
                },
                "{line}"
            );
        }
    }

    #[test]
    fn the_wall_clock_counts_seconds_across_days_and_months_and_rejects_impossible_fields() {
        let at = super::wall_clock;
        assert_eq!(at("1970-01-01T09:00:00"), Some(32_400));
        assert_eq!(
            at("2026-10-01T00:30:00").unwrap() - at("2026-09-30T23:30:00").unwrap(),
            3600
        );
        // 月の日数は見ない（31 日の無い月の 31 日は、次の月の 1 日と同じ）
        assert_eq!(at("2026-09-31T00:00:00"), at("2026-10-01T00:00:00"));
        for bad in [
            "2026-13-01T10:00:00",
            "2026-10-11T25:00:00",
            "2026-10-11T10:60:00",
            "2026-10-00T10:00:00",
            "2026-10-11 10:00:00",
            "2026-10-11T10:00",
            "Unknown",
            "",
        ] {
            assert_eq!(at(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_sacct_job_gives_the_fields_as_the_text_output_writes_them() {
        let json = json!({"jobs": [{
            "job_id": 963,
            "name": "probe",
            "time": {"elapsed": 93_784, "submission": 1795084999, "start": 4294967294u64, "end": 0},
            "working_directory": "/home/u/github/xlc",
            "submit_line": "/x/sbatch --cpus-per-task=1 --comment trial | - | p | feat/x /w --wrap sleep 1",
        }]})
        .to_string();
        let job = super::accounted_job_json(&json, "CANCELLED by 1000")
            .unwrap()
            .unwrap();
        assert_eq!(
            (job.id.as_str(), job.name.as_str(), job.state.as_str()),
            ("963", "probe", "CANCELLED by 1000")
        );
        assert_eq!(job.elapsed, "1-02:03:04");
        assert_eq!(
            job.submit,
            super::Moment {
                text: "2026-11-19T19:43:19".to_string(),
                at: Some(1795084999)
            }
        );
        // 走り始めなかったジョブの Start は None、終わっていないジョブの End は Unknown
        assert_eq!(
            (
                job.start.text.as_str(),
                job.start.at,
                job.end.text.as_str(),
                job.end.at
            ),
            ("None", None, "Unknown", None)
        );
        assert_eq!(job.workdir, "/home/u/github/xlc");
        assert_eq!(job.submitted.contact, "feat/x /w");
    }

    #[test]
    fn the_elapsed_time_is_hh_mm_ss_with_days_in_front() {
        for (secs, text) in [
            (0, "00:00:00"),
            (600, "00:10:00"),
            (22_302, "06:11:42"),
            (86_400, "1-00:00:00"),
            (0xffff_ffff, "UNLIMITED"),
        ] {
            assert_eq!(super::elapsed_text(secs), text, "{secs}");
        }
    }

    #[test]
    fn a_sacct_job_that_is_missing_or_ambiguous_is_none_or_an_error() {
        assert_eq!(super::accounted_job_json(r#"{"jobs": []}"#, ""), Ok(None));
        let two = json!({"jobs": [{"job_id": 5}, {"job_id": 6}]}).to_string();
        assert_eq!(
            super::accounted_job_json(&two, ""),
            Err("記録が 2 件ある".to_string())
        );
        assert!(super::accounted_job_json("not json", "").is_err());
    }
}
