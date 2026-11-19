//! `slurm-history.sh [--by-name] [--sacct] [FILE]` と `slurm-history.sh --job [--repo <本体のリポジトリ>] <番号>...`:
//! 終わったジョブの記録と会計の記録から見積もりと実績を比べ、`--job` はジョブごとに排他か、コマンド、走った版を出す
//! （ADR-0101。`~/.claude/scripts/slurm-history.sh` が呼ぶ）。
//!
//! 引数、出力の行、表の定義、版の決め方は `claude/scripts/slurm-history.sh` の先頭に書く。ここには、その形を作る判定を
//! 置く。記録は [`slurm`] が読み、判定（[`table`] と [`decide`]）は Slurm の出力の形を含まない型を受ける。
//!
//! 出力の行とエラーの文に書く Slurm の語（`StartTime`、`sacct`、`timed`）は、出力の行を変えないために、ここの文字列に
//! 残す（ADR-0101 の決定と理由）。

use super::slurm::{
    self, AccountingError, AccountingRow, Allocation, CompletionLine, MaxRss, PastJob,
};
use std::collections::{HashMap, HashSet};
use std::process::{Command, Stdio};

/// 会計の記録を始めた日（r995 に slurmdbd を置いた日。それより前のジョブは sacct に無い）。sacct の既定の範囲は今日だけ
/// なので、-S で渡す
const SACCT_SINCE: &str = "2026-10-12";

/// 割り込み（パーティションの間の SUSPEND）を r995 に反映した時刻（2026-10-23 21:40:32 の nixos-rebuild switch）より
/// 少し前。これより前に終わったジョブは、どのパーティションでも止められていない
const PREEMPT_SINCE: &str = "2026-10-23T21:40:00";

/// rebase で外れたコミットの reflog の項が黙って消えるまでの日数（gc.reflogExpireUnreachable の既定）
const REFLOG_DAYS: i64 = 30;

const NEXT: &str = "NEXT: UNTIMED と SHARED のジョブ（main か -c で走り、時間が当てにならない）の実績は sacct -S <日付> --name=<名前> -o JobID,JobName,Partition,Elapsed,TimeLimit,State,MaxRSS,Suspended で読む（Elapsed は止められていた時間を除く）";

const HEAD_NOTE: &str = "worktree の HEAD の動き（ブランチの切り替え）は確かめていない（worktree の reflog が無いか、版を取った時点まで届かない）";

const REPO_UNKNOWN: &str = "本体のリポジトリが分からない（WorkDir が消えた。--repo で渡す）";

pub fn main(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("--job") => job_main(&args[1..]),
        _ => table_main(args),
    }
}

fn table_main(args: &[String]) -> i32 {
    let mut by_name = false;
    let mut sacct = false;
    let mut file = "";
    for arg in args {
        match arg.as_str() {
            "--by-name" => by_name = true,
            "--sacct" => sacct = true,
            option if option.starts_with('-') => {
                eprintln!("ERROR: 不明なオプション: {option}");
                return 1;
            }
            path => {
                if !file.is_empty() {
                    eprintln!("ERROR: ファイルは 1 つだけ渡せます: {file} と {path}");
                    return 1;
                }
                file = path;
            }
        }
    }
    // ファイルだけを渡したときは、終わったジョブの記録だけを読み、sacct を呼ばない
    let sacct = sacct || file.is_empty();
    let file = if file.is_empty() {
        slurm::COMPLETIONS
    } else {
        file
    };
    let Ok(bytes) = std::fs::read(file) else {
        eprintln!("ERROR: ファイルを読めません: {file}");
        return 1;
    };
    let accounting = if sacct {
        match slurm::accounting_since(SACCT_SINCE) {
            Ok(rows) => rows,
            Err(AccountingError::Failed) => {
                eprintln!("ERROR: sacct を読めませんでした");
                return 1;
            }
            Err(AccountingError::Unreadable(reason)) => {
                eprintln!("ERROR: sacct を読めませんでした（{reason}）");
                return 1;
            }
        }
    } else {
        Vec::new()
    };
    let completions: Vec<CompletionLine> = String::from_utf8_lossy(&bytes)
        .split('\n')
        .map(slurm::completion_line)
        .collect();
    match table(&accounting, &completions, by_name) {
        Ok(out) => {
            print!("{out}");
            0
        }
        Err(error) => {
            eprintln!("{error}");
            1
        }
    }
}

/// 表に入れなかったものの数
#[derive(Default)]
struct Counts {
    /// 最後まで走っていない
    skipped: usize,
    /// 割り込みを入れた後に、割り込まれうるパーティションで終わった
    untimed: usize,
    /// 排他でない
    shared: usize,
}

/// 表に入れる 1 本
struct Entry<'a> {
    id: &'a str,
    name: &'a str,
    /// 見積もりの秒（無ければ None）
    estimate: Option<i64>,
    elapsed: i64,
}

/// 会計の記録の行と、終わったジョブの記録のファイルの行から、標準出力に書く全文を作る。読めない行があれば、その行を
/// 示すエラーの 1 行。
///
/// 会計の記録を先に、行の順に読み、最初の読めない行で止まる。表の行は、ファイルのジョブを先に、会計の記録のジョブを
/// 後に並べる（会計の記録のジョブは、ステップの行のメモリを読み終えてから加える）。両方にあるジョブは会計の記録の
/// ほうを使う（メモリと排他かを持つ）
fn table(
    accounting: &[AccountingRow],
    completions: &[CompletionLine],
    by_name: bool,
) -> Result<String, String> {
    let preempt_since = slurm::wall_clock(PREEMPT_SINCE).expect("PREEMPT_SINCE は時刻の形");
    let mut counts = Counts::default();
    let mut accounted: HashSet<&str> = HashSet::new();
    let mut memory: HashMap<&str, f64> = HashMap::new();
    let mut later: Vec<Entry> = Vec::new();
    for (i, row) in accounting.iter().enumerate() {
        let bad = |why: &str| format!("ERROR: sacct の {} 行目を読めません（{why}）", i + 1);
        match row {
            AccountingRow::Step { job, max_rss } => {
                // 0 バイトは読めない値として止める（直すのは #376）
                let m = match max_rss {
                    MaxRss::Zero => return Err(bad("MaxRSS を読めません")),
                    // 無いものは -1 として比べる（無い値のステップだけなら -）
                    MaxRss::Missing => -1.0,
                    MaxRss::Mib(m) => *m,
                };
                let held = memory.entry(job.as_str()).or_insert(m);
                if m > *held {
                    *held = m;
                }
            }
            AccountingRow::Job(job) => {
                accounted.insert(&job.id);
                if shared(job.allocation.as_ref()) {
                    counts.shared += 1;
                    continue;
                }
                if let Some(entry) = take(job, preempt_since, &mut counts).map_err(bad)? {
                    later.push(entry);
                }
            }
        }
    }
    let mut out = Table::new(by_name);
    for (i, line) in completions.iter().enumerate() {
        let bad = |why: &str| format!("ERROR: {} 行目を読めません（{why}）", i + 1);
        match line {
            CompletionLine::Blank => {}
            CompletionLine::Unreadable(why) => return Err(bad(why)),
            CompletionLine::Job { id, .. } if accounted.contains(id.as_str()) => {}
            CompletionLine::Job { record, .. } => {
                let job = record.as_ref().map_err(|why| bad(why))?;
                if let Some(entry) = take(job, preempt_since, &mut counts).map_err(bad)? {
                    out.add(&entry, -1.0);
                }
            }
        }
    }
    for entry in &later {
        out.add(entry, memory.get(entry.id).copied().unwrap_or(-1.0));
    }
    Ok(out.render(&counts))
}

/// 排他でないか（CPU の数を取って投げた）。排他か読めない（slurm-run を通らない）ジョブは、排他として表に入れる
fn shared(allocation: Option<&Allocation>) -> bool {
    matches!(allocation, Some(Allocation::Cpus(_)))
}

/// 表に入れるかを決める。最後まで走っていないものと、割り込みを入れた後に割り込まれないパーティションの外で終わった
/// ものは数えて外す（main のジョブは、止められなかったときも割り込まれうる条件で走った時間なので入れない）。時刻を読むのは
/// 入れうるものだけ（開始前に取り消したジョブは開始の時刻が無い）
fn take<'a>(
    job: &'a PastJob,
    preempt_since: i64,
    counts: &mut Counts,
) -> Result<Option<Entry<'a>>, &'static str> {
    if !job.completed {
        counts.skipped += 1;
        return Ok(None);
    }
    let start = job.start.ok_or("StartTime がありません")?;
    let end = job.end.ok_or("EndTime がありません")?;
    if end < start {
        return Err("EndTime が StartTime より前です");
    }
    if end >= preempt_since {
        match job.uninterrupted {
            None => return Err("Partition がありません"),
            Some(false) => {
                counts.untimed += 1;
                return Ok(None);
            }
            Some(true) => {}
        }
    }
    Ok(Some(Entry {
        id: &job.id,
        name: &job.name,
        estimate: job.limit_minutes.map(|m| m as i64 * 60),
        elapsed: end - start,
    }))
}

/// ジョブ名ごとの集計
#[derive(Default)]
struct Group {
    runs: usize,
    /// 見積もりのあるジョブの、見積もりと実績の合計と数
    estimate_sum: i64,
    elapsed_sum: i64,
    timed_runs: usize,
    max_ratio: f64,
    /// メモリの最大の MiB。無い値は負なので、無い値だけのジョブ名は負のまま（表は -）
    max_rss: f64,
}

/// 表の行（ジョブごと）か、ジョブ名ごとの集計
enum Table {
    Jobs(Vec<String>),
    Names(Vec<(String, Group)>),
}

impl Table {
    fn new(by_name: bool) -> Table {
        if by_name {
            Table::Names(Vec::new())
        } else {
            Table::Jobs(Vec::new())
        }
    }

    /// 1 本を加える（rss はメモリの最大の MiB、無ければ負）
    fn add(&mut self, entry: &Entry, rss: f64) {
        let ratio = entry.estimate.map(|e| entry.elapsed as f64 / e as f64);
        match self {
            Table::Jobs(rows) => rows.push(format!(
                "{:>7}  {:<24} {:>10} {:>10} {:>7} {:>8}",
                entry.id,
                entry.name,
                entry.estimate.map_or("-".to_string(), |e| hms(e as f64)),
                hms(entry.elapsed as f64),
                ratio.map_or("-".to_string(), |r| format!("{r:.2}")),
                rss_text(rss),
            )),
            Table::Names(groups) => {
                let at = match groups.iter().position(|(n, _)| n == entry.name) {
                    Some(at) => at,
                    None => {
                        groups.push((entry.name.to_string(), Group::default()));
                        groups.len() - 1
                    }
                };
                let group = &mut groups[at].1;
                group.runs += 1;
                if group.runs == 1 || rss > group.max_rss {
                    group.max_rss = rss;
                }
                if let (Some(estimate), Some(ratio)) = (entry.estimate, ratio) {
                    group.estimate_sum += estimate;
                    group.elapsed_sum += entry.elapsed;
                    group.timed_runs += 1;
                    if group.timed_runs == 1 || ratio > group.max_ratio {
                        group.max_ratio = ratio;
                    }
                }
            }
        }
    }

    fn render(&self, counts: &Counts) -> String {
        let mut out = String::new();
        match self {
            Table::Jobs(rows) => {
                out.push_str(&format!(
                    "{:>7}  {:<24} {:>10} {:>10} {:>7} {:>8}\n",
                    "JOBID", "NAME", "ESTIMATE", "ELAPSED", "RATIO", "MAX_RSS"
                ));
                for row in rows {
                    out.push_str(row);
                    out.push('\n');
                }
            }
            Table::Names(groups) => {
                out.push_str(&format!(
                    "{:<24} {:>5} {:>10} {:>10} {:>7} {:>9} {:>8}\n",
                    "NAME", "RUNS", "ESTIMATE", "ELAPSED", "RATIO", "MAX_RATIO", "MAX_RSS"
                ));
                for (name, g) in groups {
                    let (estimate, elapsed, ratio, max_ratio) = if g.timed_runs > 0 {
                        let n = g.timed_runs as f64;
                        (
                            hms(g.estimate_sum as f64 / n),
                            hms(g.elapsed_sum as f64 / n),
                            format!("{:.2}", g.elapsed_sum as f64 / g.estimate_sum as f64),
                            format!("{:.2}", g.max_ratio),
                        )
                    } else {
                        let dash = || "-".to_string();
                        (dash(), dash(), dash(), dash())
                    };
                    out.push_str(&format!(
                        "{:<24} {:>5} {:>10} {:>10} {:>7} {:>9} {:>8}\n",
                        name,
                        g.runs,
                        estimate,
                        elapsed,
                        ratio,
                        max_ratio,
                        rss_text(g.max_rss)
                    ));
                }
            }
        }
        out.push_str(&format!("SKIPPED: {}\n", counts.skipped));
        out.push_str(&format!("UNTIMED: {}\n", counts.untimed));
        out.push_str(&format!("SHARED: {}\n", counts.shared));
        if counts.untimed + counts.shared > 0 {
            out.push_str(NEXT);
            out.push('\n');
        }
        out
    }
}

/// 秒を `時:分:秒`（時は 24 を超えても積み上げる）。端数の秒は四捨五入する
fn hms(secs: f64) -> String {
    let s = (secs + 0.5) as i64;
    format!("{}:{:02}:{:02}", s / 3600, s % 3600 / 60, s % 60)
}

/// メモリの MiB を、1 GiB 以上は G で小数 1 桁、未満は M で整数に。負は無い値
fn rss_text(mib: f64) -> String {
    if mib < 0.0 {
        "-".to_string()
    } else if mib >= 1024.0 {
        format!("{:.1}G", mib / 1024.0)
    } else {
        format!("{mib:.0}M")
    }
}

fn job_main(args: &[String]) -> i32 {
    let mut repo: Option<&str> = None;
    let mut ids: Vec<&str> = Vec::new();
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--repo" => match rest.next() {
                Some(path) => repo = Some(path),
                None => {
                    eprintln!("ERROR: --repo に本体のリポジトリのパスを渡す");
                    return 1;
                }
            },
            option if option.starts_with('-') => {
                eprintln!("ERROR: 不明なオプション: {option}");
                return 1;
            }
            id => ids.push(id),
        }
    }
    if ids.is_empty() {
        eprintln!("ERROR: --job にジョブ番号を 1 つ以上渡す");
        return 1;
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let mut rc = 0;
    let mut first = true;
    for id in ids {
        if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
            eprintln!("ERROR: ジョブ番号の形でない: {id}");
            rc = 1;
            continue;
        }
        // 前のジョブを出した後は、読めなかったジョブの前にも空行を置く
        if !first {
            println!();
        }
        match one_job(id, repo, now) {
            Ok(text) => {
                print!("{text}");
                first = false;
            }
            Err(error) => {
                eprintln!("{error}");
                rc = 1;
            }
        }
    }
    rc
}

/// 1 本のジョブの行（`JOB:` から `COMMAND` まで）。読めなければエラーの 1 行
fn one_job(id: &str, repo_opt: Option<&str>, now: i64) -> Result<String, String> {
    let job = match slurm::accounted_job(id) {
        Ok(Some(job)) => job,
        Ok(None) => {
            return Err(format!(
                "ERROR: sacct にジョブ {id} が無い（番号の誤りか、sacct の記録より前のジョブ）"
            ));
        }
        Err(AccountingError::Failed) => {
            return Err(format!("ERROR: sacct がジョブ {id} を読めなかった"));
        }
        Err(AccountingError::Unreadable(reason)) => {
            return Err(format!(
                "ERROR: sacct がジョブ {id} を読めなかった（{reason}）"
            ));
        }
    };
    if job.id != id {
        return Err(format!(
            "ERROR: sacct がジョブ {id} の代わりに {} を返した",
            job.id
        ));
    }
    let contact = contact(&job.submitted.contact);
    let place = locate(repo_opt, &job.workdir, contact.branch);
    let branch_log = match (&place.git_dir, contact.branch) {
        (Some(git_dir), Some(branch))
            if git_ok(&[
                &format!("--git-dir={git_dir}"),
                "reflog",
                "exists",
                &format!("refs/heads/{branch}"),
            ]) =>
        {
            reflog(
                &[&format!("--git-dir={git_dir}")],
                &format!("refs/heads/{branch}"),
            )
        }
        _ => Vec::new(),
    };
    let head_log = place
        .worktree
        .as_deref()
        .map_or_else(Vec::new, |wt| reflog(&["-C", wt], "HEAD"));
    let commit_hash = match (&place.git_dir, contact.commit) {
        (Some(git_dir), Some(commit)) => git_text(&[
            &format!("--git-dir={git_dir}"),
            "rev-parse",
            "-q",
            "--verify",
            &format!(
                "{}^{{commit}}",
                commit.strip_suffix("-dirty").unwrap_or(commit)
            ),
        ]),
        _ => None,
    };
    let version = decide(&Evidence {
        branch: contact.branch,
        commit: contact.commit,
        repo_known: place.git_dir.is_some(),
        commit_hash: commit_hash.as_deref(),
        branch_log: &branch_log,
        head_log: &head_log,
        submitted: job.submit.at,
        started: job.start.at,
        ended: job.end.at,
        now,
    });

    // rev-parse --show-toplevel は --git-dir を渡しても cwd の作業ツリーを返すので、共通の git dir の名前から読む
    let repo = place
        .git_dir
        .as_deref()
        .map_or("-", |d| d.strip_suffix("/.git").unwrap_or(d));
    let exclusive = match &job.submitted.allocation {
        Allocation::Exclusive => "yes".to_string(),
        Allocation::Cpus(cpus) => format!("no (--cpus-per-task={cpus})"),
        Allocation::Unknown(word) => format!("unknown ({word})"),
    };
    let mut out = String::new();
    let mut line = |text: String| {
        out.push_str(&text);
        out.push('\n');
    };
    line(format!("JOB: {} {}", job.id, job.name));
    line(format!("STATE: {}", job.state));
    line(format!("ELAPSED: {}", job.elapsed));
    line(format!(
        "TIMES: submit={} start={} end={}",
        job.submit.text, job.start.text, job.end.text
    ));
    line(format!("EXCLUSIVE: {exclusive}"));
    line(format!("WORKDIR: {}", job.workdir));
    line(format!("BRANCH: {}", contact.branch.unwrap_or("-")));
    line(format!("COMMIT: {}", contact.commit.unwrap_or("-")));
    line(format!("REPO: {repo}"));
    match version {
        Version::Unknown { reasons } => {
            line("VERSION: -".to_string());
            for reason in reasons {
                line(format!("REASON: {reason}"));
            }
        }
        Version::Ran {
            hash,
            from_reflog,
            notes,
        } => {
            match from_reflog {
                Some(at) => line(format!(
                    "VERSION: {hash}（reflog の {}@{{{}}}）",
                    contact.branch.unwrap_or(""),
                    clock(at)
                )),
                None => line(format!("VERSION: {hash}")),
            }
            for note in notes {
                line(format!("NOTE: {note}"));
            }
        }
    }
    let command = job.submitted.command.as_deref().unwrap_or("-");
    for (i, text) in command.split('\n').enumerate() {
        if i == 0 {
            line(format!("COMMAND: {text}"));
        } else {
            line(format!("COMMAND+ {text}"));
        }
    }
    Ok(out)
}

/// 連絡先（`<ブランチ> <$PWD> <キー=値>…`）のブランチと commit=（-dirty を含む）
#[derive(Debug, PartialEq)]
struct Contact<'a> {
    branch: Option<&'a str>,
    commit: Option<&'a str>,
}

fn contact(text: &str) -> Contact<'_> {
    let words: Vec<&str> = text
        .split('\n')
        .next()
        .unwrap_or("")
        .split([' ', '\t'])
        .filter(|w| !w.is_empty())
        .collect();
    Contact {
        branch: words
            .first()
            .copied()
            .filter(|w| !w.starts_with('/') && !w.contains('=')),
        // 最後の commit= を読む。値が空なら commit= が無いものとする
        commit: words
            .iter()
            .rev()
            .find_map(|w| w.strip_prefix("commit="))
            .filter(|c| !c.is_empty()),
    }
}

/// 本体のリポジトリの共通の git dir と、残っている worktree の最上位
struct Place {
    git_dir: Option<String>,
    worktree: Option<String>,
}

/// 本体のリポジトリを、--repo、残っている WorkDir の git の共通のディレクトリ、worktree-add.sh の名前
/// （<本体>-<ブランチの / を - に>）の順に探す。worktree は後片付けで消えるが、reflog は本体の共通の git dir に残る
fn locate(repo_opt: Option<&str>, workdir: &str, branch: Option<&str>) -> Place {
    let common = |dir: &str| {
        git_text(&[
            "-C",
            dir,
            "rev-parse",
            "--path-format=absolute",
            "--git-common-dir",
        ])
    };
    let is_repo = |dir: &str| git_ok(&["-C", dir, "rev-parse", "--git-dir"]);
    let workdir_exists = std::path::Path::new(workdir).is_dir();
    let git_dir = if let Some(repo) = repo_opt {
        common(repo)
    } else if workdir_exists && is_repo(workdir) {
        common(workdir)
    } else if let Some(branch) = branch {
        let suffix = format!("-{}", branch.replace('/', "-"));
        let mut found = None;
        let mut p = workdir;
        while p.len() >= 2 && p.starts_with('/') {
            let cut = p.rfind('/').unwrap_or(0);
            let (parent, base) = (&p[..cut], &p[cut + 1..]);
            if base.len() > suffix.len() && base.ends_with(&suffix) {
                let candidate = format!("{parent}/{}", &base[..base.len() - suffix.len()]);
                if is_repo(&candidate) {
                    found = common(&candidate);
                    break;
                }
            }
            p = parent;
        }
        found
    } else {
        None
    };
    // 同じパスに後から別のリポジトリが置かれていれば、その reflog はこのジョブのものでない
    let worktree = match &git_dir {
        Some(g) if workdir_exists && common(workdir).as_ref() == Some(g) => {
            git_text(&["-C", workdir, "rev-parse", "--show-toplevel"]).filter(|t| !t.is_empty())
        }
        _ => None,
    };
    Place { git_dir, worktree }
}

/// git を起動し、成功すれば標準出力（末尾の改行を除く）。標準エラーは捨てる
fn git_text(args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status.success().then(|| {
        String::from_utf8_lossy(&out.stdout)
            .trim_end_matches('\n')
            .to_string()
    })
}

fn git_ok(args: &[&str]) -> bool {
    git_text(args).is_some()
}

/// reflog の 1 項
#[derive(Clone, Debug, PartialEq)]
struct ReflogEntry {
    /// UNIX の秒
    at: i64,
    hash: String,
    subject: String,
}

/// reflog を新しい順に読む（読めなければ、読めたところまで）。版は項を時刻で比べて選ぶ。`git log --before=<時刻>` は
/// コミットの日付で選ぶので rebase や amend の後は走った版でないコミットを返し、`rev-parse '<ブランチ>@{<時刻>}'` は
/// reflog がその時刻まで届かなくても警告だけで別のコミットを返す
fn reflog(git_args: &[&str], reference: &str) -> Vec<ReflogEntry> {
    let mut args = git_args.to_vec();
    args.extend([
        "log",
        "-g",
        "--date=unix",
        "--format=%gd%x09%H%x09%gs",
        reference,
    ]);
    let Ok(out) = Command::new("git")
        .args(&args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
    else {
        return Vec::new();
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(reflog_entry)
        .collect()
}

/// `<ref>@{<UNIX の秒>}\t<hash>\t<件名>` の 1 行
fn reflog_entry(line: &str) -> Option<ReflogEntry> {
    let (selector, rest) = line.split_once('\t')?;
    let at = selector
        .strip_suffix('}')?
        .rsplit_once("@{")?
        .1
        .parse()
        .ok()?;
    let (hash, subject) = rest.split_once('\t').unwrap_or((rest, ""));
    Some(ReflogEntry {
        at,
        hash: hash.to_string(),
        subject: subject.trim_matches('\t').to_string(),
    })
}

/// 走った版を決める材料
struct Evidence<'a> {
    branch: Option<&'a str>,
    /// 連絡先の commit=（-dirty を含む）
    commit: Option<&'a str>,
    /// 本体のリポジトリが見つかった
    repo_known: bool,
    /// commit= のコミットの完全な hash（本体に無いか、本体が分からなければ None）
    commit_hash: Option<&'a str>,
    /// ブランチの reflog（新しい順。無ければ空）
    branch_log: &'a [ReflogEntry],
    /// 残っている worktree の HEAD の reflog（新しい順。無ければ空）
    head_log: &'a [ReflogEntry],
    /// 投げた、走り始めた、終わった時刻（UNIX の秒。無ければ None）
    submitted: Option<i64>,
    started: Option<i64>,
    ended: Option<i64>,
    now: i64,
}

/// 走った版
#[derive(Debug, PartialEq)]
enum Version {
    /// 決めた。from_reflog は、commit= が無く、ブランチの reflog の項から決めたときのその時刻
    Ran {
        hash: String,
        from_reflog: Option<i64>,
        /// 確かめていないこと
        notes: Vec<String>,
    },
    /// 決められない理由
    Unknown { reasons: Vec<String> },
}

/// 走った版を決める。commit= があればそのコミット（本体に無ければ、-dirty なら決めない）で、版を取った時点は投げた
/// 時刻。無ければ、ブランチの reflog の、走り始めた時刻以前の最後の項のコミットで、版を取った時点は走り始めた時刻。
/// 版を取った時点から終わり（走っている間は今）までに、ブランチか worktree の HEAD の reflog に、版と違うコミットへ
/// 移る項があれば決めない
fn decide(e: &Evidence) -> Version {
    let cutoff = e.now - REFLOG_DAYS * 86_400;
    let reaches = |log: &[ReflogEntry], t: i64| log.last().is_some_and(|oldest| oldest.at <= t);
    let mut reasons: Vec<String> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    let mut version: Option<&str> = None;
    let mut version_at: Option<i64> = None;
    let mut since: Option<i64> = None;
    if let Some(commit) = e.commit {
        // 投げた時刻の無い記録は、どの項より後とする（会計の記録の文字の出力の `Unknown` を時刻の文字列と比べた順）
        let at = e.submitted.unwrap_or(i64::MAX);
        since = Some(at);
        if commit.ends_with("-dirty") {
            reasons.push("コミットしていない変更で走った（commit= の -dirty）".to_string());
        }
        if !e.repo_known {
            reasons.push(REPO_UNKNOWN.to_string());
        } else if let Some(hash) = e.commit_hash {
            version = Some(hash);
        } else {
            reasons.push(format!(
                "commit= のコミット {} が本体のリポジトリに無い（rebase で外れ、gc で消えた）",
                commit.strip_suffix("-dirty").unwrap_or(commit)
            ));
        }
        if e.branch_log.is_empty() && e.head_log.is_empty() {
            notes.push(
                "投げた後の変化は確かめていない（ブランチと worktree の reflog が無い）"
                    .to_string(),
            );
        } else if at < cutoff {
            notes.push(format!("投げた後の変化は確かめていない（版を取った時点が {REFLOG_DAYS} 日より前で、rebase で外れたコミットの reflog の項は黙って消えうる）"));
        } else if !reaches(e.branch_log, at) && !reaches(e.head_log, at) {
            notes.push(
                "投げた後の変化は確かめていない（reflog が版を取った時点まで届かない）".to_string(),
            );
        } else {
            notes.push("投げた後のコミットしない編集は確かめていない（reflog はコミットしない編集を記録せず、-dirty は投げた時点しか示さない）".to_string());
            if !reaches(e.head_log, at) {
                notes.push(HEAD_NOTE.to_string());
            }
        }
    } else if e.branch.is_none() {
        reasons.push(
            "commit= もブランチも無い（HEAD を切り離して投げたか、git の外から投げた）".to_string(),
        );
    } else if !e.repo_known {
        reasons.push(REPO_UNKNOWN.to_string());
    } else if e.branch_log.is_empty() {
        reasons.push(format!(
            "ブランチ {} か、その reflog が無い（消えた）",
            e.branch.unwrap_or("")
        ));
    } else if let Some(start) = e.started {
        since = Some(start);
        if start < cutoff {
            reasons.push(format!("Start が {REFLOG_DAYS} 日より前（rebase で外れたコミットの reflog の項は黙って消えうる）"));
        }
        let oldest = e.branch_log.last().map_or(i64::MIN, |o| o.at);
        if oldest > start {
            reasons.push(format!(
                "reflog が Start まで届かない（最も古い項は {}）",
                clock(oldest)
            ));
        } else {
            if let Some(entry) = e.branch_log.iter().find(|x| x.at <= start) {
                version = Some(&entry.hash);
                version_at = Some(entry.at);
            }
            // ブランチの版でも、投げてから Start までに worktree が別のコミットへ移っていれば、ジョブはそちらを読む
            if let Some(entry) = e.head_log.iter().find(|x| x.at <= start)
                && Some(entry.hash.as_str()) != version
            {
                reasons.push(format!(
                    "Start の時点の worktree の HEAD がブランチの版と違う（{} {}）",
                    clock(entry.at),
                    entry.subject
                ));
            }
        }
        notes.push(
            "コミットしていない変更の有無は確かめていない（nix を通らないジョブには dirty の印が出ない）"
                .to_string(),
        );
        if !reaches(e.head_log, start) {
            notes.push(HEAD_NOTE.to_string());
        }
    } else {
        reasons.push("Start が無い（走り始めていない）".to_string());
    }

    if let (Some(hash), Some(since)) = (version, since) {
        let until = e.ended.unwrap_or(e.now);
        let mut moves: Vec<String> = e
            .branch_log
            .iter()
            .chain(e.head_log)
            .filter(|x| since <= x.at && x.at <= until && x.hash != hash)
            .map(|x| format!("{} {}", clock(x.at), x.subject))
            .collect();
        moves.sort();
        moves.dedup();
        for m in moves {
            reasons.push(format!(
                "版を取った時点（{}）から End までに reflog の項がある: {m}",
                clock(since)
            ));
        }
    }
    match version {
        Some(hash) if reasons.is_empty() => Version::Ran {
            hash: hash.to_string(),
            from_reflog: if e.commit.is_some() { None } else { version_at },
            notes,
        },
        _ => Version::Unknown { reasons },
    }
}

/// UNIX の秒を、会計の記録と同じ JST の `%FT%T` で
fn clock(at: i64) -> String {
    slurm::clock_text(at).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{Contact, Evidence, ReflogEntry, Version, contact, decide, reflog_entry, table};
    use crate::script::slurm::{
        AccountingRow, Allocation, CompletionLine, MaxRss, PastJob, wall_clock,
    };

    /// 最後まで走ったジョブ（見積もりの分、開始と終わりの時刻、パーティション）
    fn done(
        id: &str,
        name: &str,
        limit: Option<u64>,
        start: &str,
        end: &str,
        partition: &str,
    ) -> PastJob {
        PastJob {
            id: id.to_string(),
            name: name.to_string(),
            completed: true,
            limit_minutes: limit,
            start: wall_clock(start),
            end: wall_clock(end),
            uninterrupted: (!partition.is_empty()).then(|| partition == "timed"),
            allocation: None,
        }
    }

    /// 最後まで走っていないジョブ（時刻は読まない）
    fn gone(id: &str, name: &str) -> PastJob {
        PastJob {
            completed: false,
            start: None,
            end: None,
            ..done(id, name, Some(10), "", "", "")
        }
    }

    /// 終わったジョブの記録のファイルの行
    fn lines(jobs: Vec<PastJob>) -> Vec<CompletionLine> {
        jobs.into_iter()
            .map(|job| CompletionLine::Job {
                id: job.id.clone(),
                record: Ok(job),
            })
            .collect()
    }

    /// 会計の記録のジョブの行（資源の取り方と、ステップのメモリの最大の MiB）
    fn accounted(job: PastJob, allocation: Allocation, steps: &[MaxRss]) -> Vec<AccountingRow> {
        let id = job.id.clone();
        let mut rows = vec![AccountingRow::Job(PastJob {
            allocation: Some(allocation),
            ..job
        })];
        rows.extend(steps.iter().map(|m| AccountingRow::Step {
            job: id.clone(),
            max_rss: *m,
        }));
        rows
    }

    fn jobs(completions: Vec<PastJob>) -> String {
        table(&[], &lines(completions), false).unwrap()
    }

    fn names(completions: Vec<PastJob>) -> String {
        table(&[], &lines(completions), true).unwrap()
    }

    /// 表の、番号か名前で始まる行
    fn row<'a>(out: &'a str, key: &str) -> &'a str {
        out.lines()
            .find(|l| l.split_whitespace().next() == Some(key))
            .unwrap_or_else(|| panic!("{key} の行が無い: {out}"))
    }

    #[test]
    fn each_job_gets_its_estimate_elapsed_ratio_and_memory_in_columns() {
        let out = jobs(vec![
            done(
                "73",
                "openmc-2A",
                Some(45),
                "2026-10-11T13:45:32",
                "2026-10-11T14:24:05",
                "main",
            ),
            done(
                "74",
                "openmc-2A",
                Some(45),
                "2026-10-11T14:24:05",
                "2026-10-11T15:14:32",
                "main",
            ),
        ]);
        assert_eq!(
            out,
            "  JOBID  NAME                       ESTIMATE    ELAPSED   RATIO  MAX_RSS\n     \
             73  openmc-2A                   0:45:00    0:38:33    0.86        -\n     \
             74  openmc-2A                   0:45:00    0:50:27    1.12        -\n\
             SKIPPED: 0\nUNTIMED: 0\nSHARED: 0\n"
        );
    }

    #[test]
    fn jobs_that_did_not_run_to_the_end_are_counted_as_skipped_without_reading_their_times() {
        let out = jobs(vec![
            done(
                "1",
                "ok",
                Some(10),
                "2026-10-11T10:00:00",
                "2026-10-11T10:05:00",
                "main",
            ),
            gone("2", "bad"),
            gone("3", "slow"),
        ]);
        assert!(!out.contains("bad") && !out.contains("slow"), "{out}");
        assert!(out.contains("SKIPPED: 2"), "{out}");
    }

    #[test]
    fn the_elapsed_time_crosses_days_and_stacks_hours_past_24() {
        let night = jobs(vec![done(
            "5",
            "night",
            Some(120),
            "2026-09-30T23:30:00",
            "2026-10-01T00:30:00",
            "main",
        )]);
        assert!(row(&night, "5").contains(" 1:00:00    0.50"), "{night}");
        let long = jobs(vec![done(
            "6",
            "long",
            Some(3000),
            "2026-10-10T00:00:00",
            "2026-10-11T02:00:00",
            "main",
        )]);
        assert!(row(&long, "6").contains("26:00:00"), "{long}");
    }

    #[test]
    fn a_job_without_an_estimate_has_dashes_for_the_estimate_and_ratio() {
        let out = jobs(vec![done(
            "7",
            "nolimit",
            None,
            "2026-10-11T10:00:00",
            "2026-10-11T10:05:00",
            "main",
        )]);
        assert_eq!(
            row(&out, "7"),
            "      7  nolimit                           -    0:05:00       -        -"
        );
    }

    #[test]
    fn by_name_gives_runs_averages_the_total_ratio_and_the_largest_ratio() {
        let out = names(vec![
            done(
                "73",
                "openmc-2A",
                Some(45),
                "2026-10-11T13:45:32",
                "2026-10-11T14:24:05",
                "main",
            ),
            done(
                "74",
                "openmc-2A",
                Some(45),
                "2026-10-11T14:24:05",
                "2026-10-11T15:14:32",
                "main",
            ),
            done(
                "80",
                "other",
                Some(10),
                "2026-10-11T16:00:00",
                "2026-10-11T16:05:00",
                "main",
            ),
            gone("81", "gone"),
        ]);
        assert_eq!(
            out.lines().next().unwrap(),
            "NAME                      RUNS   ESTIMATE    ELAPSED   RATIO MAX_RATIO  MAX_RSS"
        );
        assert_eq!(
            row(&out, "openmc-2A"),
            "openmc-2A                    2    0:45:00    0:44:30    0.99      1.12        -"
        );
        assert!(out.contains("other") && !out.contains("gone"), "{out}");
        assert!(out.contains("SKIPPED: 1"), "{out}");
    }

    #[test]
    fn by_name_averages_only_jobs_with_an_estimate_but_counts_every_run() {
        let out = names(vec![
            done(
                "1",
                "a",
                Some(10),
                "2026-10-11T10:00:00",
                "2026-10-11T10:05:00",
                "main",
            ),
            done(
                "2",
                "a",
                None,
                "2026-10-11T10:00:00",
                "2026-10-11T12:00:00",
                "main",
            ),
        ]);
        assert_eq!(
            row(&out, "a"),
            "a                            2    0:10:00    0:05:00    0.50      0.50        -"
        );
        let untimed = names(vec![done(
            "1",
            "b",
            None,
            "2026-10-11T10:00:00",
            "2026-10-11T10:05:00",
            "main",
        )]);
        assert_eq!(
            row(&untimed, "b"),
            "b                            1          -          -       -         -        -"
        );
    }

    #[test]
    fn by_name_takes_the_largest_ratio_even_after_a_first_ratio_of_0() {
        let out = names(vec![
            done(
                "1",
                "a",
                Some(10),
                "2026-10-11T10:00:00",
                "2026-10-11T10:00:00",
                "main",
            ),
            done(
                "2",
                "a",
                Some(10),
                "2026-10-11T10:00:00",
                "2026-10-11T10:05:00",
                "main",
            ),
        ]);
        assert!(row(&out, "a").contains("0.25      0.50"), "{out}");
    }

    #[test]
    fn a_finished_line_whose_end_is_before_its_start_or_lacks_a_time_stops_with_its_line_number() {
        let error = |job: PastJob| {
            table(
                &[],
                &lines(vec![
                    done(
                        "1",
                        "ok",
                        Some(10),
                        "2026-10-11T10:00:00",
                        "2026-10-11T10:05:00",
                        "main",
                    ),
                    job,
                ]),
                false,
            )
            .unwrap_err()
        };
        assert_eq!(
            error(done(
                "3",
                "neg",
                Some(10),
                "2026-10-11T10:05:00",
                "2026-10-11T10:00:00",
                "main"
            )),
            "ERROR: 2 行目を読めません（EndTime が StartTime より前です）"
        );
        assert_eq!(
            error(done(
                "3",
                "nostart",
                Some(10),
                "Unknown",
                "2026-10-11T10:00:00",
                "main"
            )),
            "ERROR: 2 行目を読めません（StartTime がありません）"
        );
        assert_eq!(
            error(done(
                "3",
                "noend",
                Some(10),
                "2026-10-11T10:00:00",
                "",
                "main"
            )),
            "ERROR: 2 行目を読めません（EndTime がありません）"
        );
    }

    #[test]
    fn an_unreadable_line_stops_with_its_line_number_and_reason() {
        let completions = vec![
            CompletionLine::Blank,
            CompletionLine::Job {
                id: "2".to_string(),
                record: Err("TimeLimit がありません"),
            },
        ];
        assert_eq!(
            table(&[], &completions, false).unwrap_err(),
            "ERROR: 2 行目を読めません（TimeLimit がありません）"
        );
        assert_eq!(
            table(
                &[],
                &[CompletionLine::Unreadable("JobId がありません")],
                false
            )
            .unwrap_err(),
            "ERROR: 1 行目を読めません（JobId がありません）"
        );
    }

    // 割り込まれうるのは、割り込みを r995 に反映した時刻（2026-10-23T21:40）より後に終わったジョブ
    #[test]
    fn after_preemption_only_uninterrupted_jobs_enter_the_table_and_the_rest_are_untimed() {
        let out = jobs(vec![
            done(
                "73",
                "bench",
                Some(45),
                "2026-11-01T13:45:32",
                "2026-11-01T14:24:05",
                "timed",
            ),
            done(
                "74",
                "table",
                Some(45),
                "2026-11-01T14:24:05",
                "2026-11-01T15:14:32",
                "main",
            ),
            done(
                "75",
                "quick",
                Some(10),
                "2026-11-01T15:20:00",
                "2026-11-01T15:25:00",
                "urgent",
            ),
        ]);
        assert!(row(&out, "73").contains("0.86"), "{out}");
        assert!(!out.contains("table") && !out.contains("quick"), "{out}");
        assert!(out.contains("UNTIMED: 2"), "{out}");
    }

    #[test]
    fn before_preemption_jobs_of_any_partition_enter_the_table() {
        let out = jobs(vec![
            done(
                "60",
                "before",
                Some(45),
                "2026-10-11T13:45:32",
                "2026-10-11T14:24:05",
                "main",
            ),
            done(
                "500",
                "sameday",
                Some(600),
                "2026-10-23T10:00:00",
                "2026-10-23T21:39:59",
                "main",
            ),
            done(
                "73",
                "bench",
                Some(45),
                "2026-11-01T13:45:32",
                "2026-11-01T14:24:05",
                "timed",
            ),
        ]);
        assert!(
            out.contains("before") && out.contains("sameday") && out.contains("bench"),
            "{out}"
        );
        assert!(out.contains("UNTIMED: 0"), "{out}");
    }

    #[test]
    fn a_default_job_that_started_before_preemption_and_ended_after_is_untimed() {
        let out = jobs(vec![
            done(
                "501",
                "long",
                Some(600),
                "2026-10-23T20:00:00",
                "2026-10-23T22:00:00",
                "main",
            ),
            done(
                "502",
                "edge",
                Some(600),
                "2026-10-23T20:00:00",
                "2026-10-23T21:40:00",
                "main",
            ),
        ]);
        assert!(!out.contains("long") && !out.contains("edge"), "{out}");
        assert!(out.contains("UNTIMED: 2"), "{out}");
    }

    #[test]
    fn by_name_does_not_mix_untimed_jobs_into_the_totals() {
        let out = names(vec![
            done(
                "73",
                "openmc-2A",
                Some(45),
                "2026-11-01T13:45:32",
                "2026-11-01T14:24:05",
                "timed",
            ),
            done(
                "74",
                "openmc-2A",
                Some(45),
                "2026-11-01T14:24:05",
                "2026-11-02T14:24:05",
                "main",
            ),
        ]);
        assert!(
            row(&out, "openmc-2A").contains("    1    0:45:00    0:38:33"),
            "{out}"
        );
        assert!(out.contains("UNTIMED: 1"), "{out}");
    }

    #[test]
    fn jobs_that_did_not_finish_are_skipped_rather_than_untimed_after_preemption() {
        let out = jobs(vec![
            done(
                "73",
                "bench",
                Some(45),
                "2026-11-01T13:45:32",
                "2026-11-01T14:24:05",
                "timed",
            ),
            gone("77", "gone"),
        ]);
        assert!(out.contains("SKIPPED: 1\nUNTIMED: 0"), "{out}");
    }

    #[test]
    fn a_job_that_ended_after_preemption_without_a_partition_stops_with_its_line_number() {
        assert_eq!(
            table(
                &[],
                &lines(vec![done(
                    "80",
                    "nopart",
                    Some(10),
                    "2026-11-01T10:00:00",
                    "2026-11-01T10:05:00",
                    ""
                )]),
                false
            )
            .unwrap_err(),
            "ERROR: 1 行目を読めません（Partition がありません）"
        );
        // 割り込みの前に終わったジョブは、パーティションを見ない
        assert!(
            table(
                &[],
                &lines(vec![done(
                    "80",
                    "nopart",
                    Some(10),
                    "2026-10-11T10:00:00",
                    "2026-10-11T10:05:00",
                    ""
                )]),
                false
            )
            .is_ok()
        );
    }

    #[test]
    fn next_shows_how_to_read_untimed_and_shared_jobs_only_when_there_are_some() {
        let next = "NEXT: UNTIMED と SHARED のジョブ（main か -c で走り、時間が当てにならない）の実績は sacct -S <日付> --name=<名前> -o JobID,JobName,Partition,Elapsed,TimeLimit,State,MaxRSS,Suspended で読む（Elapsed は止められていた時間を除く）";
        let bench = || {
            done(
                "73",
                "bench",
                Some(45),
                "2026-11-01T13:45:32",
                "2026-11-01T14:24:05",
                "timed",
            )
        };
        let untimed = names(vec![
            bench(),
            done(
                "74",
                "table",
                Some(45),
                "2026-11-01T14:24:05",
                "2026-11-01T15:14:32",
                "main",
            ),
        ]);
        assert_eq!(untimed.lines().last(), Some(next));
        let shared = table(
            &accounted(bench(), Allocation::Cpus("8".to_string()), &[]),
            &[],
            false,
        )
        .unwrap();
        assert_eq!(shared.lines().last(), Some(next));
        assert!(!names(vec![bench()]).contains("NEXT:"));
    }

    fn sacct_job(id: &str, name: &str, start: &str, end: &str) -> PastJob {
        done(id, name, Some(60), start, end, "timed")
    }

    #[test]
    fn accounted_jobs_get_the_largest_step_memory_after_the_jobcomp_rows() {
        let accounting = accounted(
            sacct_job(
                "201",
                "lattice",
                "2026-11-01T10:00:00",
                "2026-11-01T10:30:00",
            ),
            Allocation::Exclusive,
            &[MaxRss::Missing, MaxRss::Mib(512.0), MaxRss::Mib(2048.0)],
        );
        let completions = lines(vec![done(
            "100",
            "old",
            Some(60),
            "2026-10-05T10:00:00",
            "2026-10-05T10:10:00",
            "timed",
        )]);
        let out = table(&accounting, &completions, false).unwrap();
        let rows: Vec<&str> = out.lines().skip(1).take(2).collect();
        assert_eq!(
            rows,
            [
                "    100  old                         1:00:00    0:10:00    0.17        -",
                "    201  lattice                     1:00:00    0:30:00    0.50     2.0G",
            ]
        );
    }

    #[test]
    fn a_job_in_both_records_is_read_once_from_the_accounting() {
        let accounting = accounted(
            sacct_job(
                "201",
                "lattice",
                "2026-11-01T10:00:00",
                "2026-11-01T10:30:00",
            ),
            Allocation::Exclusive,
            &[MaxRss::Mib(2048.0)],
        );
        let completions = lines(vec![
            done(
                "100",
                "lattice",
                Some(60),
                "2026-10-05T10:00:00",
                "2026-10-05T10:10:00",
                "timed",
            ),
            // 会計の記録にもあるジョブの行は、ほかの欄が壊れていても読まない
            PastJob {
                start: None,
                ..sacct_job("201", "lattice", "", "")
            },
        ]);
        let out = table(&accounting, &completions, true).unwrap();
        assert!(
            row(&out, "lattice").starts_with("lattice                      2 "),
            "{out}"
        );
        assert!(row(&out, "lattice").ends_with("2.0G"), "{out}");
    }

    #[test]
    fn by_name_memory_is_the_largest_of_the_jobs_with_a_value_and_a_dash_without_any() {
        let job = |id: &str, name: &str| {
            sacct_job(id, name, "2026-11-01T10:00:00", "2026-11-01T10:30:00")
        };
        let mut accounting = accounted(
            job("201", "none"),
            Allocation::Exclusive,
            &[MaxRss::Missing],
        );
        accounting.extend(accounted(
            job("202", "mixed"),
            Allocation::Exclusive,
            &[MaxRss::Mib(300.0)],
        ));
        accounting.extend(accounted(
            job("203", "mixed"),
            Allocation::Exclusive,
            &[MaxRss::Missing],
        ));
        accounting.extend(accounted(
            job("204", "zero"),
            Allocation::Exclusive,
            &[MaxRss::Mib(0.0)],
        ));
        let completions = lines(vec![
            done(
                "100",
                "none",
                Some(60),
                "2026-10-05T10:00:00",
                "2026-10-05T10:10:00",
                "timed",
            ),
            done(
                "101",
                "zero",
                Some(60),
                "2026-10-05T10:00:00",
                "2026-10-05T10:10:00",
                "timed",
            ),
        ]);
        let out = table(&accounting, &completions, true).unwrap();
        assert!(row(&out, "none").ends_with("        -"), "{out}");
        assert!(row(&out, "mixed").ends_with("     300M"), "{out}");
        assert!(row(&out, "zero").ends_with("       0M"), "{out}");
    }

    #[test]
    fn shared_jobs_are_counted_and_jobs_of_unknown_allocation_enter_the_table() {
        let mut accounting = accounted(
            sacct_job(
                "202",
                "lattice-shared",
                "2026-11-01T11:00:00",
                "2026-11-01T11:27:00",
            ),
            Allocation::Cpus("8".to_string()),
            &[MaxRss::Mib(100.0)],
        );
        accounting.extend(accounted(
            sacct_job(
                "203",
                "manual",
                "2026-11-01T11:00:00",
                "2026-11-01T11:27:00",
            ),
            Allocation::Unknown("--wrap".to_string()),
            &[],
        ));
        let out = table(&accounting, &[], false).unwrap();
        assert!(!out.contains("lattice-shared"), "{out}");
        assert!(out.contains("manual"), "{out}");
        assert!(out.contains("SHARED: 1"), "{out}");
    }

    #[test]
    fn a_step_with_zero_bytes_of_memory_stops_with_its_sacct_line_number() {
        // 0 バイトは読めない値として止まる（直すのは #376）。丸めて 0 になる数バイトは読める
        let accounting = accounted(
            sacct_job(
                "201",
                "lattice",
                "2026-11-01T10:00:00",
                "2026-11-01T10:30:00",
            ),
            Allocation::Exclusive,
            &[MaxRss::Mib(512.0), MaxRss::Zero],
        );
        assert_eq!(
            table(&accounting, &[], false).unwrap_err(),
            "ERROR: sacct の 3 行目を読めません（MaxRSS を読めません）"
        );
        let tiny = accounted(
            sacct_job("201", "tiny", "2026-11-01T10:00:00", "2026-11-01T10:30:00"),
            Allocation::Exclusive,
            &[MaxRss::Mib(0.0)],
        );
        assert!(row(&table(&tiny, &[], false).unwrap(), "201").ends_with(" 0M"));
    }

    #[test]
    fn an_unreadable_accounted_job_stops_before_the_jobcomp_lines_are_read() {
        let accounting = accounted(
            PastJob {
                end: None,
                ..sacct_job("201", "lattice", "2026-11-01T10:00:00", "")
            },
            Allocation::Exclusive,
            &[],
        );
        let completions = vec![CompletionLine::Unreadable("JobId がありません")];
        assert_eq!(
            table(&accounting, &completions, false).unwrap_err(),
            "ERROR: sacct の 1 行目を読めません（EndTime がありません）"
        );
    }

    fn entry(at: i64, hash: &str, subject: &str) -> ReflogEntry {
        ReflogEntry {
            at,
            hash: hash.to_string(),
            subject: subject.to_string(),
        }
    }

    /// 2026-11-18T09:00:00 JST（今は 2026-11-20T12:00:00）
    const T9: i64 = 1_794_960_000;
    const H: i64 = 3600;
    const NOW: i64 = T9 + 2 * 86_400 + 3 * H;

    /// commit= の無い、ブランチの reflog の届くジョブ（10:05 に始まり 11:00 に終わった）
    fn evidence<'a>(branch_log: &'a [ReflogEntry], head_log: &'a [ReflogEntry]) -> Evidence<'a> {
        Evidence {
            branch: Some("feat/x"),
            commit: None,
            repo_known: true,
            commit_hash: None,
            branch_log,
            head_log,
            submitted: Some(T9 + H),
            started: Some(T9 + H + 300),
            ended: Some(T9 + 2 * H),
            now: NOW,
        }
    }

    /// 09:30 にブランチを作り、09:40 にコミットした reflog
    fn branch_log() -> Vec<ReflogEntry> {
        vec![
            entry(T9 + 40 * 60, "c1", "commit: b"),
            entry(T9 + 30 * 60, "c0", "branch: Created from HEAD"),
        ]
    }

    fn reasons(version: Version) -> Vec<String> {
        match version {
            Version::Unknown { reasons } => reasons,
            ran => panic!("{ran:?}"),
        }
    }

    fn notes(version: Version) -> (String, Vec<String>) {
        match version {
            Version::Ran { hash, notes, .. } => (hash, notes),
            unknown => panic!("{unknown:?}"),
        }
    }

    #[test]
    fn the_commit_in_the_contact_is_the_version_when_the_reflogs_do_not_move_while_it_runs() {
        let log = branch_log();
        let e = Evidence {
            commit: Some("c1abc"),
            commit_hash: Some("c1"),
            ..evidence(&log, &log)
        };
        assert_eq!(
            decide(&e),
            Version::Ran {
                hash: "c1".to_string(),
                from_reflog: None,
                notes: vec!["投げた後のコミットしない編集は確かめていない（reflog はコミットしない編集を記録せず、-dirty は投げた時点しか示さない）".to_string()],
            }
        );
        // worktree が消えて HEAD の reflog が無ければ、その動きは確かめていない
        let (_, notes) = notes(decide(&Evidence { head_log: &[], ..e }));
        assert_eq!(notes[1], super::HEAD_NOTE);
    }

    #[test]
    fn a_dirty_or_missing_commit_or_an_unknown_repository_leaves_the_version_undecided() {
        let log = branch_log();
        let base = Evidence {
            commit: Some("c1abc"),
            commit_hash: Some("c1"),
            ..evidence(&log, &log)
        };
        assert_eq!(
            reasons(decide(&Evidence {
                commit: Some("c1abc-dirty"),
                ..base
            })),
            ["コミットしていない変更で走った（commit= の -dirty）"]
        );
        assert_eq!(
            reasons(decide(&Evidence {
                commit_hash: None,
                ..base
            })),
            ["commit= のコミット c1abc が本体のリポジトリに無い（rebase で外れ、gc で消えた）"]
        );
        assert_eq!(
            reasons(decide(&Evidence {
                repo_known: false,
                commit_hash: None,
                ..base
            })),
            [super::REPO_UNKNOWN]
        );
    }

    #[test]
    fn a_reflog_entry_to_another_commit_between_submission_and_end_leaves_the_version_undecided() {
        let mut log = branch_log();
        log.insert(0, entry(T9 + 90 * 60, "c2", "commit: c"));
        let head = vec![entry(
            T9 + 80 * 60,
            "c0",
            "checkout: moving from feat/x to c0",
        )];
        let e = Evidence {
            commit: Some("c1abc"),
            commit_hash: Some("c1"),
            ..evidence(&log, &head)
        };
        assert_eq!(
            reasons(decide(&e)),
            [
                "版を取った時点（2026-11-18T10:00:00）から End までに reflog の項がある: 2026-11-18T10:20:00 checkout: moving from feat/x to c0",
                "版を取った時点（2026-11-18T10:00:00）から End までに reflog の項がある: 2026-11-18T10:30:00 commit: c",
            ]
        );
        // 同じ項が両方の reflog にあれば 1 つにまとめる
        let same = vec![entry(T9 + 90 * 60, "c2", "commit: c")];
        assert_eq!(
            reasons(decide(&Evidence {
                head_log: &same,
                ..e
            }))
            .len(),
            1
        );
        // 走っている間は今までを見る
        let running = Evidence {
            ended: None,
            head_log: &[],
            ..e
        };
        assert_eq!(reasons(decide(&running)).len(), 1);
        assert_eq!(
            notes(decide(&Evidence {
                ended: Some(T9 + 80 * 60),
                head_log: &[],
                ..e
            }))
            .0,
            "c1"
        );
    }

    #[test]
    fn with_a_commit_the_notes_say_what_the_reflogs_could_not_show() {
        let log = branch_log();
        let base = Evidence {
            commit: Some("c1abc"),
            commit_hash: Some("c1"),
            ..evidence(&log, &log)
        };
        let note = |e: Evidence| notes(decide(&e)).1;
        assert_eq!(
            note(Evidence {
                branch_log: &[],
                head_log: &[],
                ..base
            }),
            ["投げた後の変化は確かめていない（ブランチと worktree の reflog が無い）"]
        );
        assert_eq!(
            // 版のコミットだけの reflog（30 日の前からの項は、版と違うコミットへ移っていない）
            note(Evidence {
                submitted: Some(NOW - 31 * 86_400),
                branch_log: &log[..1],
                head_log: &log[..1],
                ..base
            }),
            [
                "投げた後の変化は確かめていない（版を取った時点が 30 日より前で、rebase で外れたコミットの reflog の項は黙って消えうる）"
            ]
        );
        let late = vec![entry(T9 + 3 * H, "c1", "branch: Created from c1")];
        assert_eq!(
            note(Evidence {
                branch_log: &late,
                head_log: &[],
                ..base
            }),
            ["投げた後の変化は確かめていない（reflog が版を取った時点まで届かない）"]
        );
    }

    #[test]
    fn without_a_commit_the_version_is_the_last_branch_entry_before_the_start() {
        let log = branch_log();
        assert_eq!(
            decide(&evidence(&log, &log)),
            Version::Ran {
                hash: "c1".to_string(),
                from_reflog: Some(T9 + 40 * 60),
                notes: vec!["コミットしていない変更の有無は確かめていない（nix を通らないジョブには dirty の印が出ない）".to_string()],
            }
        );
        let (_, notes) = notes(decide(&evidence(&log, &[])));
        assert_eq!(notes[1], super::HEAD_NOTE);
    }

    #[test]
    fn without_a_commit_a_move_after_the_start_or_a_reflog_that_does_not_reach_it_leaves_it_undecided()
     {
        let mut moved = branch_log();
        moved.insert(0, entry(T9 + 90 * 60, "c2", "commit: c"));
        assert_eq!(
            reasons(decide(&evidence(&moved, &[]))),
            [
                "版を取った時点（2026-11-18T10:05:00）から End までに reflog の項がある: 2026-11-18T10:30:00 commit: c"
            ]
        );
        let late = vec![entry(T9 + 3 * H, "c1", "branch: Created from c1")];
        assert_eq!(
            reasons(decide(&evidence(&late, &[]))),
            ["reflog が Start まで届かない（最も古い項は 2026-11-18T12:00:00）"]
        );
        let log = branch_log();
        let head = vec![entry(
            T9 + H + 120,
            "c0",
            "checkout: moving from feat/x to main",
        )];
        assert_eq!(
            reasons(decide(&evidence(&log, &head))),
            [
                "Start の時点の worktree の HEAD がブランチの版と違う（2026-11-18T10:02:00 checkout: moving from feat/x to main）"
            ]
        );
    }

    #[test]
    fn without_a_commit_the_branch_repository_log_and_start_must_be_known() {
        let log = branch_log();
        let base = evidence(&log, &log);
        assert_eq!(
            reasons(decide(&Evidence {
                branch: None,
                ..base
            })),
            ["commit= もブランチも無い（HEAD を切り離して投げたか、git の外から投げた）"]
        );
        assert_eq!(
            reasons(decide(&Evidence {
                repo_known: false,
                ..base
            })),
            [super::REPO_UNKNOWN]
        );
        assert_eq!(
            reasons(decide(&Evidence {
                branch_log: &[],
                ..base
            })),
            ["ブランチ feat/x か、その reflog が無い（消えた）"]
        );
        assert_eq!(
            reasons(decide(&Evidence {
                started: None,
                ..base
            })),
            ["Start が無い（走り始めていない）"]
        );
        let old = Evidence {
            started: Some(NOW - 31 * 86_400),
            ended: Some(NOW - 31 * 86_400 + H),
            ..base
        };
        assert_eq!(
            reasons(decide(&old))[0],
            "Start が 30 日より前（rebase で外れたコミットの reflog の項は黙って消えうる）"
        );
    }

    #[test]
    fn the_contact_gives_the_branch_first_and_the_last_commit() {
        assert_eq!(
            contact("feat/x /w commit=abc claude=s"),
            Contact {
                branch: Some("feat/x"),
                commit: Some("abc")
            }
        );
        assert_eq!(
            contact("/w claude=s"),
            Contact {
                branch: None,
                commit: None
            }
        );
        assert_eq!(contact("claude=s commit=abc-dirty").branch, None);
        assert_eq!(contact("feat/x /w commit=abc commit=").commit, None);
        assert_eq!(
            contact(""),
            Contact {
                branch: None,
                commit: None
            }
        );
    }

    #[test]
    fn a_reflog_line_gives_the_time_the_hash_and_the_subject() {
        assert_eq!(
            reflog_entry("feat/x@{1795107430}\tabc\tcommit: b"),
            Some(entry(1_795_107_430, "abc", "commit: b"))
        );
        assert_eq!(
            reflog_entry("HEAD@{1795107430}\tabc\t"),
            Some(entry(1_795_107_430, "abc", ""))
        );
        assert_eq!(reflog_entry("HEAD@{yesterday}\tabc\tx"), None);
        assert_eq!(reflog_entry(""), None);
    }
}
