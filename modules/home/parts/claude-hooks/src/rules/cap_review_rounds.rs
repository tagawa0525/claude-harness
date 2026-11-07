//! cap-review-rounds: ローカルの /code-review の周を、ブランチごとに上限で止める
//!
//! レビューは周ごとに新しい候補を出すので、直す指摘の基準だけでは周が終わらない（2026-11-07〜08 の 36 周は、ほぼ
//! 毎回 8〜10 件を返した）。上限を数で決め、超えたら止めてユーザーに聞かせる。
//!
//! - 数える呼び出し: Skill の `code-review`。対象は引数の中で最初にある、ディレクトリとして在る絶対パス（無ければ
//!   hook の cwd）で、その worktree がチェックアウトしているブランチを数える
//! - 止めるだけの呼び出し: Agent の prompt の、空でない最初の行に `/code-review` があり、その後に在るディレクトリの
//!   絶対パスがあるもの（親が別のモデルの担当にレビューを頼む形）。数えない（その担当が呼ぶ Skill を数えるので、
//!   数えると 1 周が 2 回になる）が、上限に達していれば、担当を起こす前に止める
//! - 上限: `review-level.sh` の `ROUNDS:`（1 か 3）に記録の周 1 を足し、3 のときはさらに記録の周の後の例外の周 3 を
//!   足した数（1 なら 2、3 なら 7）。ブランチで担当と親が回した周の合計で数える
//! - 置き場: リポジトリの共通の git ディレクトリの `claude-review-rounds/<ブランチ名>`（`/` は `%2F`）。1 行が 1 周で、
//!   回したときの HEAD を書く。worktree を消して作り直しても、rebase で HEAD が変わっても数は残る（rebase の後の周も
//!   数える規則のため）。既定ブランチの祖先になった HEAD の行は、マージ済みの前のブランチのものとして数えない
//!   （同じ名前のブランチを作り直したとき）
//! - 呼び出しの結果は見ないので、対象を見つけられなかった回とエラーの回も数える（上限は記録の周と例外の周の分だけ
//!   余裕がある）
//!
//! `review-level.sh` の store のパスは package.nix が環境変数で渡す。無いとき（cargo で直接ビルドしたもの）と、
//! スクリプトが失敗したときは、判定できなかった理由を利用者に見せて通す（ADR-0033）。エスケープは無い（周の数は
//! ユーザーの判断で変えるもので、モデルが外す理由が無い）。

use super::Rule;
use crate::input::Input;
use crate::output::Finding;
use crate::shell::Shell;

pub struct CapReviewRounds;

impl CapReviewRounds {
    fn check_with(&self, _input: &Input, _review_level: Option<&str>) -> Vec<Finding> {
        Vec::new()
    }
}

impl Rule for CapReviewRounds {
    fn name(&self) -> &'static str {
        "cap-review-rounds"
    }

    fn tools(&self) -> &'static [&'static str] {
        &["Skill", "Agent"]
    }

    fn check(&self, input: &Input, _shell: &Shell) -> Vec<Finding> {
        self.check_with(input, None)
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
    fn one_round_branch_allows_the_round_and_the_record_round_only() {
        let r = repo("docs/y", 1);
        assert!(denied(&run(&r, &review(&r))).is_none());
        assert!(denied(&run(&r, &review(&r))).is_none());
        let reason = denied(&run(&r, &review(&r))).expect("3 周目は止めるはず");
        assert!(reason.contains("上限 2"), "{reason}");
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
        assert!(context.contains("上限 2"), "{context}");
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
    fn agent_asking_for_a_review_is_not_counted_but_is_denied_at_the_limit() {
        let r = repo("docs/y", 1);
        let ask = agent(
            &format!(
                "/code-review を Skill で呼び、次の引数で回す: `high {}`。",
                r.dir.display()
            ),
            Path::new("/"),
        );
        for _ in 0..3 {
            assert!(denied(&run(&r, &ask)).is_none(), "数えないので通すはず");
        }
        run(&r, &review(&r));
        run(&r, &review(&r));
        assert!(denied(&run(&r, &ask)).is_some(), "上限に達したら止めるはず");
    }

    #[test]
    fn agent_without_code_review_on_its_first_line_is_ignored() {
        let r = repo("docs/y", 1);
        run(&r, &review(&r));
        run(&r, &review(&r));
        let other = agent(
            &format!(
                "最初に /delegate を呼び、進める。\n周ごとに /code-review を回す: {}",
                r.dir.display()
            ),
            Path::new("/"),
        );
        assert!(denied(&run(&r, &other)).is_none());
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
        run(&r, &here);
        run(&r, &here);
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
        run(&r, &quoted);
        run(&r, &quoted);
        assert!(denied(&run(&r, &review(&r))).is_some());
    }

    #[test]
    fn branches_are_counted_separately() {
        let r = repo("docs/y", 1);
        run(&r, &review(&r));
        run(&r, &review(&r));
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
        run(&r, &review(&r));
        run(&r, &review(&r));
        git(&r.dir, &["switch", "-q", "main"]);
        commit(&r.dir, "on-main");
        git(&r.dir, &["switch", "-q", "docs/y"]);
        git(&r.dir, &["rebase", "-q", "main"]);
        assert!(denied(&run(&r, &review(&r))).is_some());
    }

    #[test]
    fn rounds_of_a_merged_branch_do_not_count_for_a_new_branch_of_the_same_name() {
        let r = repo("docs/y", 1);
        run(&r, &review(&r));
        run(&r, &review(&r));
        git(&r.dir, &["switch", "-q", "main"]);
        git(&r.dir, &["merge", "-q", "--no-ff", "-m", "merge", "docs/y"]);
        git(&r.dir, &["branch", "-q", "-D", "docs/y"]);
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
