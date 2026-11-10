//! cap-review-rounds: ローカルの /code-review の周を、ブランチごとに上限で止める
//!
//! レビューは周ごとに新しい候補を出すので、直す指摘の基準だけでは周が終わらない（2026-11-07〜08 の 36 周は、ほぼ
//! 毎回 8〜10 件を返した）。上限を数で決め、超えたら止めてユーザーに聞かせる。
//!
//! - 数える呼び出し: Skill の `code-review` だけ。対象は引数の中で最初にある、ディレクトリとして在る絶対パス（無ければ
//!   hook の cwd）で、その worktree がチェックアウトしているブランチを数える。親が Agent で担当にレビューを頼む周も、
//!   担当が呼ぶ Skill で数えて止める。Agent の prompt は見ない（自然文から「/code-review を頼む」を見分ける判定は、
//!   否定や別のスキルを呼ぶ文に当たるか、頼む文を落とした）
//! - 上限: `review-level.sh` の `ROUNDS:`（1 か 3）に記録の周 1 を足し、3 のときはさらに記録の周の後の例外の周 3 を
//!   足した数（1 なら 2、3 なら 7）。ブランチで担当と親が回した周の合計で数える
//! - 置き場: リポジトリの共通の git ディレクトリの `claude-review-rounds/<ブランチ名>`（`/` は `%2F`）。1 行が 1 周で、
//!   回したときの HEAD を書く。worktree を消して作り直しても、rebase で HEAD が変わっても数は残る（rebase の後の周も
//!   数える規則のため）。ブランチの reflog の最も古い時刻より前の行は、消した前の同じ名前のブランチのものとして
//!   数えない（ブランチを消すと reflog も消え、作り直すと新しく始まる。rebase では続く。マージの前に rebase した
//!   HEAD は既定ブランチの祖先にならないので、祖先かでは見分けられない）
//! - 呼び出しの結果は見ないので、対象を見つけられなかった回とエラーの回も数える（上限は記録の周と例外の周の分だけ
//!   余裕がある）
//!
//! `review-level.sh` の store のパスは package.nix が環境変数で渡す。無いとき（cargo で直接ビルドしたもの）と、
//! スクリプトが失敗したときは、判定できなかった理由を利用者に見せて通す（ADR-0033）。エスケープは無い（周の数は
//! ユーザーの判断で変えるもので、モデルが外す理由が無い）。

use super::Rule;
use crate::git;
use crate::input::Input;
use crate::output::{Finding, notice};
use crate::shell::Shell;
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct CapReviewRounds;

/// review-level.sh の store のパスを持つ環境変数。package.nix の包む側が固定する（cargo で直接ビルドしたものには無い）
const REVIEW_LEVEL: &str = "CLAUDE_HOOKS_REVIEW_LEVEL";

/// 数を置く、共通の git ディレクトリの下のディレクトリ
const STATE_DIR: &str = "claude-review-rounds";

/// 記録の周（3 周目か、文書だけのブランチの 1 周目で blocking を直した head に回す 1 周）
const RECORD_ROUNDS: u32 = 1;
/// 記録の周の後の例外の周（blocking が (c) だけのとき。文書だけのブランチには無い）
const EXCEPTION_ROUNDS: u32 = 3;

/// 文字列の中で最初の、ディレクトリとして在る絶対パス。囲みの `` ` ``、引用符、括弧、和文の句読点で切る
fn first_directory(text: &str) -> Option<PathBuf> {
    let stop = |c: char| c.is_whitespace() || "`'\"「」（）()。、,;".contains(c);
    text.split(stop)
        .filter(|w| w.starts_with('/'))
        .map(PathBuf::from)
        .find(|p| p.is_dir())
}

/// ブランチ名をファイル名にする（`%` と `/` を符号にする）
fn state_file_name(branch: &str) -> String {
    branch.replace('%', "%25").replace('/', "%2F")
}

/// 周の上限（記録の周と例外の周を足した数）
fn limit(rounds: u32) -> u32 {
    if rounds <= 1 {
        rounds + RECORD_ROUNDS
    } else {
        rounds + RECORD_ROUNDS + EXCEPTION_ROUNDS
    }
}

/// review-level.sh を dir で実行し、`ROUNDS:` の値を返す。失敗は理由を利用者に見せて None
fn rounds_of(script: &str, dir: &Path, branch: &str) -> Option<u32> {
    // bash で起動する（store のファイルの shebang の /usr/bin/env に頼らず、書いたばかりのファイルを exec して
    // ETXTBSY になることも避ける）
    let out = match Command::new("bash")
        .arg(script)
        .arg(branch)
        .current_dir(dir)
        .output()
    {
        Ok(out) => out,
        Err(e) => {
            notice(format!(
                "cap-review-rounds: review-level.sh を起動できないので、/code-review の周を数えていません: {e}"
            ));
            return None;
        }
    };
    let stdout = String::from_utf8_lossy(&out.stdout);
    let rounds = stdout
        .lines()
        .find_map(|l| l.strip_prefix("ROUNDS: "))
        .and_then(|v| v.trim().parse().ok());
    if !out.status.success() || rounds.is_none() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        notice(format!(
            "cap-review-rounds: review-level.sh が {branch} の ROUNDS を出さないので、/code-review の周を数えていません: {}",
            stderr.trim()
        ));
        return None;
    }
    rounds
}

/// ブランチの reflog の最も古い記録の時刻（UNIX 秒）。reflog が無ければ None
fn branch_created(dir: &Path, branch: &str) -> Option<u64> {
    let refname = format!("refs/heads/{branch}");
    let out = git::git(
        dir,
        &["reflog", "show", "--date=unix", "--format=%gd", &refname],
    )?;
    // 行は `refs/heads/<branch>@{<UNIX 秒>}`。最後の行が最も古い
    let last = out.lines().last()?;
    last.rsplit_once("@{")?.1.strip_suffix('}')?.parse().ok()
}

/// 数の置き場の行のうち、今のブランチのもの（ブランチの reflog の最も古い時刻より前の行は、消した前の同じ名前の
/// ブランチのものとして除く。reflog が無ければ全部を数える）
fn live_rounds(dir: &Path, branch: &str, path: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let created = branch_created(dir, branch);
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .filter(|l| {
            let at: Option<u64> = l.split_whitespace().nth(1).and_then(|t| t.parse().ok());
            match (created, at) {
                (Some(c), Some(t)) => t >= c,
                _ => true,
            }
        })
        .map(str::to_string)
        .collect()
}

fn deny_reason(branch: &str, count: usize, limit: u32, rounds: u32) -> String {
    let breakdown = if rounds <= 1 {
        format!("ROUNDS {rounds} + 記録の周 {RECORD_ROUNDS}")
    } else {
        format!("ROUNDS {rounds} + 記録の周 {RECORD_ROUNDS} + 例外の周 {EXCEPTION_ROUNDS}")
    };
    format!(
        "ブランチ {branch} のローカルの /code-review は {count} 周を回し、上限 {limit}（review-level.sh の {breakdown}）に達しました。\n\
         理由: レビューは周ごとに新しい指摘を出し続けるので、直す指摘の基準だけでは周が終わりません。数で止めて、続けるかをユーザーが決めます（担当と親の周の合計で、rebase の後の周も数えます）。\n\
         修正方法: 残った blocking を issue に書き、ユーザーに判断を求めてください。/delegate の担当は、報告の「マージの前後で人の手が要ること」に blocking と issue を書いて止まります。"
    )
}

impl CapReviewRounds {
    fn check_with(&self, input: &Input, review_level: Option<&str>) -> Vec<Finding> {
        if input.tool_name != "Skill" || input.skill != "code-review" {
            return Vec::new();
        }
        // 対象を書かない /code-review は cwd の差分を見る
        let dir = first_directory(&input.args).unwrap_or_else(|| input.cwd.clone());
        let branch = git::current_branch(&dir);
        if branch.is_empty() {
            return Vec::new();
        }
        let Some(script) = review_level else {
            notice(format!(
                "cap-review-rounds: review-level.sh のパス（環境変数 {REVIEW_LEVEL}）が無いので、/code-review の周を数えていません（nix で包んだものだけが数える）"
            ));
            return Vec::new();
        };
        let Some(rounds) = rounds_of(script, &dir, &branch) else {
            return Vec::new();
        };
        let Some(common) = git::git(
            &dir,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        ) else {
            return Vec::new();
        };
        let state_dir = Path::new(&common).join(STATE_DIR);
        let path = state_dir.join(state_file_name(&branch));
        let mut lines = live_rounds(&dir, &branch, &path);
        let limit = limit(rounds);
        if lines.len() >= limit as usize {
            return vec![Finding::Deny(deny_reason(
                &branch,
                lines.len(),
                limit,
                rounds,
            ))];
        }
        let head = git::git(&dir, &["rev-parse", "HEAD"]).unwrap_or_default();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        lines.push(format!("{head} {now}"));
        let written = std::fs::create_dir_all(&state_dir)
            .and_then(|()| std::fs::write(&path, lines.join("\n") + "\n"));
        if let Err(e) = written {
            notice(format!(
                "cap-review-rounds: {} に周の数を書けません: {e}",
                path.display()
            ));
            return Vec::new();
        }
        vec![Finding::Context(format!(
            "ブランチ {branch} のローカルの /code-review の {} 周目です（上限 {limit}。review-level.sh の ROUNDS {rounds} に記録の周{}を足した数で、担当と親の周の合計）。",
            lines.len(),
            if rounds <= 1 { "" } else { "と例外の周" },
        ))]
    }
}

impl Rule for CapReviewRounds {
    fn name(&self) -> &'static str {
        "cap-review-rounds"
    }

    fn tools(&self) -> &'static [&'static str] {
        &["Skill"]
    }

    fn check(&self, input: &Input, _shell: &Shell) -> Vec<Finding> {
        let script = std::env::var(REVIEW_LEVEL).ok();
        self.check_with(input, script.as_deref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    /// テストの間だけある一時のディレクトリ。テストは並行に走るので、呼ぶたびに別のディレクトリにする
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> TempDir {
            static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "claude-hooks-review-rounds-{tag}-{}-{seq}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn git(dir: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .args(args)
            .current_dir(dir)
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?}");
    }

    fn commit(dir: &Path, file: &str) {
        std::fs::write(dir.join(file), file).unwrap();
        git(dir, &["add", file]);
        git(dir, &["commit", "-q", "-m", file]);
    }

    /// main に 1 コミット、`branch` に 1 コミットのリポジトリ（`branch` をチェックアウトしたまま）と、
    /// `ROUNDS: <rounds>` を出す偽の review-level.sh。偽のスクリプトは渡されたブランチ名も書き残す
    struct Repo {
        _tmp: TempDir,
        dir: PathBuf,
        script: String,
    }

    fn repo(branch: &str, rounds: u32) -> Repo {
        let tmp = TempDir::new("repo");
        let dir = tmp.0.join("app");
        std::fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init", "-q", "-b", "main"]);
        // 手元のグローバルの hook（コミットの件名の検査）を使わない
        git(&dir, &["config", "core.hooksPath", "/nonexistent/hooks"]);
        commit(&dir, "init");
        git(&dir, &["switch", "-q", "-c", branch]);
        commit(&dir, "x");
        let script = tmp.0.join("review-level.sh");
        std::fs::write(
            &script,
            format!("echo \"$1\" >>\"$(dirname \"$0\")/called\"\necho 'LEVEL: high'\necho 'ROUNDS: {rounds}'\n"),
        )
        .unwrap();
        Repo {
            dir,
            script: script.to_string_lossy().into_owned(),
            _tmp: tmp,
        }
    }

    fn skill(name: &str, args: &str, cwd: &Path) -> Input {
        let json = serde_json::json!({
            "tool_name": "Skill",
            "tool_input": {"skill": name, "args": args},
            "cwd": cwd,
        });
        Input::parse(&json.to_string()).unwrap()
    }

    fn agent(prompt: &str, cwd: &Path) -> Input {
        let json = serde_json::json!({
            "tool_name": "Agent",
            "tool_input": {"prompt": prompt, "description": "d"},
            "cwd": cwd,
        });
        Input::parse(&json.to_string()).unwrap()
    }

    fn review(r: &Repo) -> Input {
        skill(
            "code-review",
            &format!("high {}", r.dir.display()),
            Path::new("/"),
        )
    }

    fn run(r: &Repo, input: &Input) -> Vec<Finding> {
        CapReviewRounds.check_with(input, Some(&r.script))
    }

    /// 文書だけのブランチ（`ROUNDS: 1`）の上限: 1 周、記録の周 1、例外の周 3
    const DOCS_LIMIT: usize = 5;

    /// 文書だけのブランチの上限まで周を回す
    fn use_up(r: &Repo, input: &Input) {
        for _ in 0..DOCS_LIMIT {
            run(r, input);
        }
    }

    fn denied(findings: &[Finding]) -> Option<String> {
        findings.iter().find_map(|f| match f {
            Finding::Deny(s) => Some(s.clone()),
            Finding::Context(_) => None,
        })
    }

    #[test]
    fn three_round_branch_allows_seven_rounds_and_denies_the_eighth() {
        let r = repo("feat/x", 3);
        for n in 1..=7 {
            assert!(
                denied(&run(&r, &review(&r))).is_none(),
                "{n} 周目は通すはず"
            );
        }
        let reason = denied(&run(&r, &review(&r))).expect("8 周目は止めるはず");
        assert!(reason.contains("feat/x"), "{reason}");
        assert!(reason.contains("上限 7"), "{reason}");
        assert!(reason.contains("issue"), "{reason}");
        assert!(reason.contains("ユーザー"), "{reason}");
    }

    #[test]
    fn one_round_branch_allows_the_round_the_record_round_and_three_exception_rounds() {
        let r = repo("docs/y", 1);
        for n in 1..=5 {
            assert!(
                denied(&run(&r, &review(&r))).is_none(),
                "{n} 周目は通すはず"
            );
        }
        let reason = denied(&run(&r, &review(&r))).expect("6 周目は止めるはず");
        assert!(reason.contains("上限 5"), "{reason}");
        assert!(reason.contains("例外の周 3"), "{reason}");
    }

    #[test]
    fn allowed_round_tells_its_number_and_the_limit() {
        let r = repo("feat/x", 1);
        let findings = run(&r, &review(&r));
        let context = findings
            .iter()
            .find_map(|f| match f {
                Finding::Context(s) => Some(s.clone()),
                Finding::Deny(_) => None,
            })
            .expect("周の数を伝えるはず");
        assert!(context.contains("1 周目"), "{context}");
        assert!(context.contains("上限 5"), "{context}");
        assert!(context.contains("例外の周"), "{context}");
    }

    #[test]
    fn passes_the_checked_out_branch_to_review_level() {
        let r = repo("feat/x", 3);
        run(&r, &review(&r));
        let called = std::fs::read_to_string(Path::new(&r.script).with_file_name("called"))
            .expect("review-level.sh を呼ぶはず");
        assert_eq!(called.trim(), "feat/x");
    }

    #[test]
    fn agent_is_neither_counted_nor_denied_even_at_the_limit() {
        let r = repo("docs/y", 1);
        let wt = r.dir.display();
        let prompts = [
            format!("/code-review を Skill で呼び、次の引数で回す: `high {wt}`。"),
            format!("`/code-review high {wt}` を Skill で呼ぶ。"),
            format!("`/code-review high {wt}` を Skill で呼びません。"),
            format!("/code-review の指摘を直す: {wt}"),
        ];
        for prompt in &prompts {
            assert!(
                run(&r, &agent(prompt, Path::new("/"))).is_empty(),
                "数えないはず: {prompt}"
            );
        }
        run(&r, &review(&r));
        assert!(
            denied(&run(&r, &review(&r))).is_none(),
            "Agent を数えていなければ 2 周目は通すはず"
        );
        for prompt in &prompts {
            assert!(
                run(&r, &agent(prompt, Path::new("/"))).is_empty(),
                "上限でも止めないはず: {prompt}"
            );
        }
    }

    #[test]
    fn other_skills_are_not_counted() {
        let r = repo("docs/y", 1);
        let simplify = skill("simplify", &r.dir.display().to_string(), Path::new("/"));
        for _ in 0..3 {
            assert!(run(&r, &simplify).is_empty());
        }
        assert!(denied(&run(&r, &review(&r))).is_none());
    }

    #[test]
    fn review_without_a_path_counts_the_branch_of_cwd() {
        let r = repo("docs/y", 1);
        let here = skill("code-review", "high", &r.dir);
        use_up(&r, &here);
        assert!(denied(&run(&r, &review(&r))).is_some());
    }

    #[test]
    fn path_in_backquotes_and_followed_by_punctuation_is_found() {
        let r = repo("docs/y", 1);
        let quoted = skill(
            "code-review",
            &format!("high `{}`。焦点: a", r.dir.display()),
            Path::new("/"),
        );
        use_up(&r, &quoted);
        assert!(denied(&run(&r, &review(&r))).is_some());
    }

    #[test]
    fn branches_are_counted_separately() {
        let r = repo("docs/y", 1);
        use_up(&r, &review(&r));
        let other = r.dir.with_file_name("other");
        git(
            &r.dir,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "docs/z",
                other.to_str().unwrap(),
                "main",
            ],
        );
        commit(&other, "z");
        let input = skill(
            "code-review",
            &format!("high {}", other.display()),
            Path::new("/"),
        );
        assert!(denied(&run(&r, &input)).is_none());
    }

    #[test]
    fn count_survives_a_rebase_of_the_branch() {
        let r = repo("docs/y", 1);
        use_up(&r, &review(&r));
        git(&r.dir, &["switch", "-q", "main"]);
        commit(&r.dir, "on-main");
        git(&r.dir, &["switch", "-q", "docs/y"]);
        git(&r.dir, &["rebase", "-q", "main"]);
        assert!(denied(&run(&r, &review(&r))).is_some());
    }

    #[test]
    fn rounds_of_a_merged_branch_do_not_count_for_a_new_branch_of_the_same_name() {
        let r = repo("docs/y", 1);
        use_up(&r, &review(&r));
        git(&r.dir, &["switch", "-q", "main"]);
        git(&r.dir, &["merge", "-q", "--no-ff", "-m", "merge", "docs/y"]);
        git(&r.dir, &["branch", "-q", "-D", "docs/y"]);
        // 同じ秒の中で作り直すと、ブランチの作成の時刻と前の行の時刻を見分けられない
        std::thread::sleep(std::time::Duration::from_millis(1100));
        git(&r.dir, &["switch", "-q", "-c", "docs/y"]);
        commit(&r.dir, "again");
        assert!(denied(&run(&r, &review(&r))).is_none());
    }

    #[test]
    fn rounds_before_a_rebase_of_a_merged_branch_do_not_count_for_a_new_branch_of_the_same_name() {
        let r = repo("docs/y", 1);
        use_up(&r, &review(&r));
        git(&r.dir, &["switch", "-q", "main"]);
        commit(&r.dir, "on-main");
        git(&r.dir, &["switch", "-q", "docs/y"]);
        git(&r.dir, &["rebase", "-q", "main"]);
        git(&r.dir, &["switch", "-q", "main"]);
        git(&r.dir, &["merge", "-q", "--no-ff", "-m", "merge", "docs/y"]);
        git(&r.dir, &["branch", "-q", "-D", "docs/y"]);
        // 同じ秒の中で作り直すと、ブランチの作成の時刻と前の行の時刻を見分けられない
        std::thread::sleep(std::time::Duration::from_millis(1100));
        git(&r.dir, &["switch", "-q", "-c", "docs/y"]);
        commit(&r.dir, "again");
        assert!(denied(&run(&r, &review(&r))).is_none());
    }

    #[test]
    fn detached_head_is_not_counted() {
        let r = repo("docs/y", 1);
        git(&r.dir, &["switch", "-q", "--detach"]);
        for _ in 0..3 {
            assert!(run(&r, &review(&r)).is_empty());
        }
    }

    #[test]
    fn missing_script_is_reported_and_passes() {
        let _lock = crate::output::NOTICES_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::output::take_notices();
        let r = repo("docs/y", 1);
        for _ in 0..3 {
            assert!(CapReviewRounds.check_with(&review(&r), None).is_empty());
        }
        let notices = crate::output::take_notices();
        assert!(
            notices
                .iter()
                .any(|n| n.contains("CLAUDE_HOOKS_REVIEW_LEVEL")),
            "{notices:?}"
        );
    }
}
