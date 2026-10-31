//! check-merge-review: 既定ブランチへのローカルの git merge の本文に、最後にレビューの周を回した HEAD と指摘ごとの行を
//! 求め、その HEAD より後のコミットが挙動を変えないものだけかを確かめる

use super::Rule;
use crate::input::Input;
use crate::output::Finding;
use crate::shell::Shell;

pub struct CheckMergeReview;

/// 16 進の語（コミットの hash）を印に置き換えた行
fn mask_hashes(line: &str) -> String {
    line.to_string()
}

/// `git diff -U0` の出力が、hunk の対の行で 16 進の語だけを変えたものか
fn only_hash_changes(_diff: &str) -> bool {
    false
}

impl Rule for CheckMergeReview {
    fn name(&self) -> &'static str {
        "check-merge-review"
    }

    fn check(&self, _input: &Input, _shell: &Shell) -> Vec<Finding> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    /// テストの間だけある一時のディレクトリ（テストが panic しても消す）。テストは並行に走るので、呼ぶたびに別のディレクトリにする
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> TempDir {
            static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "claude-hooks-merge-review-{tag}-{}-{seq}",
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

    fn git_out(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn git(dir: &Path, args: &[&str]) {
        git_out(dir, args);
    }

    /// main に 1 コミット、feat/x に main から 1 コミット（src/a.rs）を持つリポジトリ。今のブランチは main。
    /// git の hook（手元のグローバルの core.hooksPath）は実行しない
    struct Repo {
        root: TempDir,
    }

    impl Repo {
        fn new(tag: &str) -> Repo {
            let root = TempDir::new(tag);
            let repo = Repo { root };
            let dir = repo.dir();
            std::fs::create_dir_all(&dir).unwrap();
            git(&dir, &["init", "-q", "-b", "main"]);
            for (k, v) in [
                ("user.name", "t"),
                ("user.email", "t@example.com"),
                ("core.hooksPath", "/nonexistent/hooks"),
                ("commit.gpgsign", "false"),
            ] {
                git(&dir, &["config", k, v]);
            }
            repo.write("README.md", "readme\n");
            repo.commit("init");
            git(&dir, &["switch", "-q", "-c", "feat/x"]);
            repo.write("src/a.rs", "fn a() {}\n");
            repo.commit("feat: a");
            git(&dir, &["switch", "-q", "main"]);
            repo
        }

        fn dir(&self) -> PathBuf {
            self.root.0.join("repo")
        }

        fn write(&self, path: &str, text: &str) {
            let p = self.dir().join(path);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }

        fn commit(&self, msg: &str) -> String {
            git(&self.dir(), &["add", "-A"]);
            git(&self.dir(), &["commit", "-q", "-m", msg]);
            git_out(&self.dir(), &["rev-parse", "HEAD"])
        }

        /// feat/x に移ってファイルを書き、コミットして main に戻る。コミットの hash
        fn commit_on_branch(&self, files: &[(&str, &str)]) -> String {
            git(&self.dir(), &["switch", "-q", "feat/x"]);
            for (p, t) in files {
                self.write(p, t);
            }
            let h = self.commit("change");
            git(&self.dir(), &["switch", "-q", "main"]);
            h
        }

        fn tip(&self) -> String {
            git_out(&self.dir(), &["rev-parse", "feat/x"])
        }

        /// 本文を -F のファイル（リポジトリの外）で渡して判定する
        fn check_body(&self, body: &str) -> Vec<Finding> {
            let msg = self.root.0.join("msg.txt");
            std::fs::write(&msg, body).unwrap();
            self.check(&format!(
                "git merge --no-ff feat/x -F {}",
                msg.to_string_lossy()
            ))
        }

        fn check(&self, command: &str) -> Vec<Finding> {
            let json = serde_json::json!({
                "tool_name": "Bash",
                "tool_input": {"command": command},
                "cwd": self.dir(),
            });
            let input = Input::parse(&json.to_string()).unwrap();
            CheckMergeReview.check(&input, &Shell::parse(command))
        }
    }

    fn body(reviewed: &str, findings: &str) -> String {
        format!(
            "Merge: x\n\n## Why\nw\n\n## Impact\n{findings}\n\nReviewed: {reviewed}\nBranch: feat/x\n"
        )
    }

    const FINDINGS: &str = "- 再現: `a_test` — 境界の誤り\n- 再現なし → #12: 効率";

    fn denies(findings: &[Finding]) -> Vec<String> {
        findings
            .iter()
            .filter_map(|f| match f {
                Finding::Deny(r) => Some(r.clone()),
                _ => None,
            })
            .collect()
    }

    fn only_deny(findings: &[Finding]) -> String {
        match denies(findings).as_slice() {
            [r] => r.clone(),
            other => panic!("deny が 1 件のはず: {other:?}"),
        }
    }

    #[test]
    fn passes_when_the_reviewed_head_is_the_tip() {
        let repo = Repo::new("tip");
        let tip = repo.tip();
        assert!(denies(&repo.check_body(&body(&tip, FINDINGS))).is_empty());
        // 短い hash も解く
        assert!(denies(&repo.check_body(&body(&tip[..9], FINDINGS))).is_empty());
    }

    #[test]
    fn passes_documents_dvc_records_and_remap_after_the_reviewed_head() {
        let repo = Repo::new("after-ok");
        let reviewed = repo.commit_on_branch(&[("src/b.rs", "// see 0123abcd4\n")]);
        repo.commit_on_branch(&[
            ("docs/notes.md", "notes\n"),
            ("docs/issues/0001-x.md", "issue\n"),
            ("examples/a/dvc.lock", "outs: 1\n"),
            ("examples/a/out.dvc", "md5: 1\n"),
            ("src/b.rs", "// see 9876fedc1\n"),
        ]);
        let r = repo.check_body(&body(&reviewed, FINDINGS));
        assert!(denies(&r).is_empty(), "{:?}", denies(&r));
    }

    #[test]
    fn denies_code_and_test_changes_after_the_reviewed_head() {
        let repo = Repo::new("after-code");
        let reviewed = repo.tip();
        repo.commit_on_branch(&[("src/a.rs", "fn a() { b() }\n"), ("docs/n.md", "n\n")]);
        let r = only_deny(&repo.check_body(&body(&reviewed, FINDINGS)));
        assert!(r.contains("src/a.rs"), "{r}");
        assert!(!r.contains("docs/n.md"), "{r}");
        assert!(r.contains("/code-review"), "{r}");
        assert!(r.contains("ALLOW_UNREVIEWED_HEAD=<理由>"), "{r}");
    }

    #[test]
    fn counts_skill_hook_documents_and_adrs_as_behavior() {
        for path in [
            ".claude/skills/a/SKILL.md",
            ".claude/README.md",
            "CLAUDE.md",
            "sub/CLAUDE.md",
            "skills/b/SKILL.md",
            "docs/adr/0001-x.md",
            "examples/a/dvc.yaml",
        ] {
            let repo = Repo::new("after-docs");
            let reviewed = repo.tip();
            repo.commit_on_branch(&[(path, "text\n")]);
            let r = only_deny(&repo.check_body(&body(&reviewed, FINDINGS)));
            assert!(r.contains(path), "{path}: {r}");
        }
    }

    #[test]
    fn escape_passes_unreviewed_changes_but_not_a_missing_form() {
        let repo = Repo::new("escape");
        let reviewed = repo.tip();
        repo.commit_on_branch(&[("src/a.rs", "fn a() { b() }\n")]);
        let msg = repo.root.0.join("msg.txt");
        std::fs::write(&msg, body(&reviewed, FINDINGS)).unwrap();
        let cmd = format!(
            "ALLOW_UNREVIEWED_HEAD='歯止めの 3 周を使い切った後の変更で、Impact に未レビューと書いた' git merge --no-ff feat/x -F {}",
            msg.to_string_lossy()
        );
        assert!(denies(&repo.check(&cmd)).is_empty());
        // 値が短ければ理由を求める
        let short = format!(
            "ALLOW_UNREVIEWED_HEAD=1 git merge --no-ff feat/x -F {}",
            msg.to_string_lossy()
        );
        let d = denies(&repo.check(&short));
        assert!(d.iter().any(|r| r.contains("理由を記載")), "{d:?}");
        // 形の誤りはエスケープで外れない
        std::fs::write(&msg, body(&reviewed, "")).unwrap();
        assert_eq!(denies(&repo.check(&cmd)).len(), 1);
    }

    #[test]
    fn denies_a_body_without_one_reviewed_line() {
        let repo = Repo::new("form");
        let tip = repo.tip();
        let none = only_deny(&repo.check_body(&format!("Merge: x\n\n{FINDINGS}\n")));
        assert!(none.contains("Reviewed: <"), "{none}");
        let two =
            only_deny(&repo.check_body(&format!("{}Reviewed: {tip}\n", body(&tip, FINDINGS))));
        assert!(two.contains("Reviewed:"), "{two}");
        for bad in ["", "abc", "HEAD", "ABCDEF1234", "feat/x"] {
            let r = only_deny(&repo.check_body(&body(bad, FINDINGS)));
            assert!(r.contains("Reviewed:"), "{bad}: {r}");
        }
    }

    #[test]
    fn denies_a_body_without_finding_lines() {
        let repo = Repo::new("findings");
        let tip = repo.tip();
        for findings in [
            "",
            "レビューは 3 周（再現: `a_test`）",
            "- 再現:",
            "- 再現なし → 効率",
            "- 前提が誤り:",
        ] {
            let r = only_deny(&repo.check_body(&body(&tip, findings)));
            assert!(r.contains("再現なし → #"), "{findings}: {r}");
            assert!(r.contains("指摘なし"), "{findings}: {r}");
        }
    }

    #[test]
    fn accepts_each_finding_form() {
        let repo = Repo::new("forms");
        let tip = repo.tip();
        for findings in [
            "再現: `a_test`",
            "- 再現なし → #0190: 効率",
            "* 前提が誤り: 関数は既に検査している",
            "  - 抜け道: 作為的な書き方",
            "指摘なし",
            "- 指摘なし（high で 1 周）",
        ] {
            let r = repo.check_body(&body(&tip, findings));
            assert!(denies(&r).is_empty(), "{findings}: {:?}", denies(&r));
        }
    }

    #[test]
    fn denies_a_reviewed_hash_off_the_branch() {
        let repo = Repo::new("off");
        // rebase で書き換わったときと同じく、ブランチの先端の祖先でないコミット
        git(&repo.dir(), &["switch", "-q", "-c", "other"]);
        repo.write("src/c.rs", "c\n");
        let other = repo.commit("other");
        git(&repo.dir(), &["switch", "-q", "main"]);
        let r = only_deny(&repo.check_body(&body(&other, FINDINGS)));
        assert!(r.contains("range-diff"), "{r}");
        // 既定ブランチのコミット
        let main = git_out(&repo.dir(), &["rev-parse", "main"]);
        let r = only_deny(&repo.check_body(&body(&main, FINDINGS)));
        assert!(r.contains("ブランチのコミット"), "{r}");
        // コミットに解けない
        let r = only_deny(&repo.check_body(&body("0000000deadbeef", FINDINGS)));
        assert!(r.contains("0000000deadbeef"), "{r}");
    }

    #[test]
    fn denies_a_merge_whose_body_is_not_known() {
        let repo = Repo::new("nobody");
        for cmd in [
            "git merge --no-ff feat/x",
            "git merge --no-ff feat/x -m \"$MSG\"",
            "git merge --no-ff feat/x -F /nonexistent/msg.txt",
        ] {
            let r = only_deny(&repo.check(cmd));
            assert!(r.contains("本文"), "{cmd}: {r}");
        }
    }

    #[test]
    fn reads_a_heredoc_body() {
        let repo = Repo::new("heredoc");
        let tip = repo.tip();
        let cmd = format!(
            "git merge --no-ff feat/x -m \"$(cat <<'EOF'\n{}EOF\n)\"",
            body(&tip, FINDINGS)
        );
        assert!(denies(&repo.check(&cmd)).is_empty());
    }

    #[test]
    fn denies_an_octopus_merge() {
        let repo = Repo::new("octopus");
        git(&repo.dir(), &["branch", "feat/y", "feat/x"]);
        let tip = repo.tip();
        let msg = repo.root.0.join("msg.txt");
        std::fs::write(&msg, body(&tip, FINDINGS)).unwrap();
        let r = only_deny(&repo.check(&format!(
            "git merge --no-ff feat/x feat/y -F {}",
            msg.to_string_lossy()
        )));
        assert!(r.contains("1 本ずつ"), "{r}");
    }

    #[test]
    fn ignores_merges_outside_the_default_branch_and_merge_control() {
        let repo = Repo::new("ignore");
        for cmd in [
            "git merge --abort",
            "git merge --continue",
            "git switch feat/x && git merge main",
        ] {
            assert!(denies(&repo.check(cmd)).is_empty(), "{cmd}");
        }
    }

    #[test]
    fn masks_only_whole_hex_words_with_a_letter() {
        assert_eq!(
            mask_hashes("see b7e01d512。"),
            mask_hashes("see 0123abcd4。")
        );
        // 数字だけの語は数の変更と見分けられない
        assert_ne!(mask_hashes("k = 1234567"), mask_hashes("k = 1234568"));
        // 語の一部は置き換えない
        assert_ne!(mask_hashes("xabcdef12"), mask_hashes("xabcdef13"));
        assert_ne!(mask_hashes("abc12"), mask_hashes("abc13"));
    }

    #[test]
    fn hash_only_diff_needs_paired_hunks() {
        let remap = "diff --git a/r.md b/r.md\n--- a/r.md\n+++ b/r.md\n@@ -3 +3 @@\n-at b7e01d512\n+at 0123abcd4\n";
        assert!(only_hash_changes(remap));
        let text = "--- a/r.md\n+++ b/r.md\n@@ -3 +3 @@\n-at b7e01d512\n+on 0123abcd4\n";
        assert!(!only_hash_changes(text));
        let added = "--- a/r.md\n+++ b/r.md\n@@ -3,0 +4 @@\n+at 0123abcd4\n";
        assert!(!only_hash_changes(added));
        // hunk の無い変更（モードだけ、バイナリ）
        assert!(!only_hash_changes(
            "diff --git a/x b/x\nold mode 100644\nnew mode 100755\n"
        ));
        assert!(!only_hash_changes(""));
    }
}
