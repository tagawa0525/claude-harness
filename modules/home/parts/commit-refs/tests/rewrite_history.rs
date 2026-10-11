//! `commit-refs rewrite-history` を一時的な git リポジトリで走らせ、履歴の中の文書と本文の番号を、書き換えた後の
//! 最後の番号に付け直すことを確かめる。
//!
//! filter-repo で書き換えた後の状態を、テストの中で作る。文書と本文には書き換える前の番号（架空の X0、X1…）を書き、
//! 対応表で今のコミットに結ぶ。

mod common;

use std::fs;
use std::process::Output;

use common::*;

const X0: &str = "1111111111111111111111111111111111111111";
const X1: &str = "2222222222222222222222222222222222222222";
const X2: &str = "3333333333333333333333333333333333333333";

/// c0（初期コミット）、c1（X0 を指す文書と本文）、c2（X1 を指す文書）の履歴と、X0 → c0、X1 → c1 の対応表
struct History {
    t: TempRepo,
    c0: String,
    c1: String,
    c2: String,
    map: String,
}

impl History {
    fn new(name: &str) -> Self {
        let t = TempRepo::new(name);
        let c0 = t.head();
        t.write("a.md", &format!("orig of c0: {}\n", &X0[..8]));
        t.commit_all(&format!("docs: c1 refers to {}", &X0[..10]));
        let c1 = t.head();
        t.write("notes.md", &format!("at {}\n", &X1[..8]));
        t.commit_all("docs: c2");
        let c2 = t.head();
        let map = t.map_file(&format!("old new\n{X0} {c0}\n{X1} {c1}\n"));
        History { t, c0, c1, c2, map }
    }

    fn rewrite(&self) -> Output {
        self.rewrite_with(&self.map)
    }

    fn rewrite_with(&self, map: &str) -> Output {
        run(&self.t.dir, &["rewrite-history", map], "")
    }

    fn show(&self, rev: &str, path: &str) -> String {
        self.t.git(&["show", &format!("{rev}:{path}")])
    }

    fn message(&self, rev: &str) -> String {
        self.t.git(&["log", "-1", "--format=%B", rev])
    }

    fn refs(&self) -> String {
        self.t
            .git(&["for-each-ref", "--format=%(refname) %(objectname)"])
    }
}

#[test]
fn rewrites_references_in_files_and_messages_to_the_final_hashes() {
    let h = History::new("basic");
    let out = h.rewrite();
    assert_ok(&out);
    // c0 は何も指さないので変わらない。c1 は c0 を指すよう変わり、c2 は新しい c1 を指す
    assert_eq!(h.t.rev("main~2"), h.c0);
    let new_c1 = h.t.rev("main~1");
    assert_ne!(new_c1, h.c1);
    assert_eq!(
        h.show("main~1", "a.md"),
        format!("orig of c0: {}\n", &h.c0[..8])
    );
    assert!(
        h.message("main~1")
            .contains(&format!("refers to {}", &h.c0[..10])),
        "{}",
        h.message("main~1")
    );
    assert_eq!(h.show("main", "notes.md"), format!("at {}\n", &new_c1[..8]));
}

#[test]
fn keeps_authors_committers_and_dates() {
    let h = History::new("metadata");
    assert_ok(&h.rewrite());
    let format = "--format=%an|%ae|%ad|%cn|%ce|%cd";
    assert_eq!(
        h.t.git(&["log", "-1", format, "main"]),
        h.t.git(&["log", "-1", format, &h.c2])
    );
}

#[test]
fn keeps_the_original_refs_and_writes_the_final_map() {
    let h = History::new("original");
    let out = h.rewrite();
    assert_ok(&out);
    assert_eq!(h.t.rev("refs/commit-refs/original/heads/main"), h.c2);
    let new_c1 = h.t.rev("main~1");
    let new_c2 = h.t.rev("main");
    let map = fs::read_to_string(h.t.path(".git/commit-refs/commit-map")).unwrap();
    assert!(map.starts_with("old new\n"), "{map}");
    for line in [
        format!("{X0} {}", h.c0),
        format!("{X1} {new_c1}"),
        format!("{} {new_c2}", h.c2),
    ] {
        assert!(map.lines().any(|l| l == line), "{line} not in\n{map}");
    }
    assert!(!h.refs().contains("refs/commit-refs/new/"), "{}", h.refs());
}

#[test]
fn updates_the_working_tree_to_the_new_head() {
    let h = History::new("worktree");
    assert_ok(&h.rewrite());
    let new_c1 = h.t.rev("main~1");
    assert_eq!(h.t.read("notes.md"), format!("at {}\n", &new_c1[..8]));
    assert_eq!(h.t.git(&["status", "--porcelain"]), "");
}

#[test]
fn rewrites_references_to_current_commits_missing_from_the_map() {
    let h = History::new("current");
    h.t.write("more.md", &format!("see {}\n", &h.c1[..9]));
    h.t.commit_all("docs: more");
    assert_ok(&h.rewrite());
    let new_c1 = h.t.rev("main~2");
    assert_eq!(h.show("main", "more.md"), format!("see {}\n", &new_c1[..9]));
}

#[test]
fn rewrites_every_branch() {
    let h = History::new("branches");
    h.t.git(&["switch", "-q", "-c", "feat", &h.c1]);
    h.t.write("feat.md", &format!("from {}\n", &X1[..8]));
    h.t.commit_all("docs: feat");
    h.t.git(&["switch", "-q", "main"]);
    assert_ok(&h.rewrite());
    let new_c1 = h.t.rev("main~1");
    assert_eq!(h.t.rev("feat~1"), new_c1);
    assert_eq!(
        h.show("feat", "feat.md"),
        format!("from {}\n", &new_c1[..8])
    );
}

#[test]
fn moves_lightweight_and_annotated_tags() {
    let h = History::new("tags");
    h.t.git(&["tag", "light", &h.c1]);
    h.t.git(&[
        "tag",
        "-a",
        "v1",
        "-m",
        &format!("release after {}", &X1[..8]),
        &h.c2,
    ]);
    assert_ok(&h.rewrite());
    let new_c1 = h.t.rev("main~1");
    assert_eq!(h.t.rev("light"), new_c1);
    assert_eq!(h.t.rev("v1^{commit}"), h.t.rev("main"));
    let tag = h.t.git(&["cat-file", "-p", "v1"]);
    assert!(
        tag.contains(&format!("release after {}", &new_c1[..8])),
        "{tag}"
    );
    assert!(!h.refs().contains("refs/tags/commit-refs"), "{}", h.refs());
}

#[test]
fn leaves_binary_blobs_alone() {
    let h = History::new("binary");
    h.t.write("blob.bin", &format!("bin\0{}\n", &X1[..8]));
    h.t.commit_all("chore: binary");
    let before = h.t.rev("main:blob.bin");
    assert_ok(&h.rewrite());
    assert_eq!(h.t.rev("main:blob.bin"), before);
}

#[test]
fn a_map_that_changes_nothing_keeps_every_commit() {
    let t = TempRepo::new("identity");
    let c0 = t.head();
    t.write("a.md", "no hashes here\n");
    t.commit_all("docs: a");
    let c1 = t.head();
    let map = t.map_file(&format!("old new\n{c0} {c0}\n{c1} {c1}\n"));
    let out = run(&t.dir, &["rewrite-history", &map], "");
    assert_ok(&out);
    assert_eq!(t.head(), c1);
    assert!(stdout(&out).contains("CHANGED: 0\n"), "{}", stdout(&out));
}

#[test]
fn a_reference_to_a_descendant_stops_as_a_cycle_without_moving_refs() {
    // c1 の文書が、子の c2 を書き換える前の番号（X2）で指す。c2 は親の c1 を先に要し、c1 は文書が指す c2 を先に
    // 要するので、どちらも先に流せない
    let t = TempRepo::new("forward");
    t.write("a.md", &format!("child {}\n", &X2[..8]));
    t.commit_all("docs: c1");
    t.write("b.md", "c2\n");
    t.commit_all("docs: c2");
    let c2 = t.head();
    let map = t.map_file(&format!("old new\n{X2} {c2}\n"));
    let before = t.git(&["for-each-ref", "--format=%(refname) %(objectname)"]);
    let out = run(&t.dir, &["rewrite-history", &map], "");
    assert_error(&out);
    assert!(stderr(&out).contains("循環"), "{}", stderr(&out));
    assert_eq!(
        t.git(&["for-each-ref", "--format=%(refname) %(objectname)"]),
        before
    );
}

#[test]
fn a_reference_to_a_pruned_commit_stops_without_moving_refs() {
    let h = History::new("pruned");
    let map = h.t.map_file(&format!(
        "old new\n{X0} {}\n{X1} 0000000000000000000000000000000000000000\n",
        h.c0
    ));
    let before = h.refs();
    let out = h.rewrite_with(&map);
    assert_error(&out);
    assert!(stderr(&out).contains("消えたコミット"), "{}", stderr(&out));
    assert_eq!(h.refs(), before);
}

#[test]
fn a_dirty_working_tree_stops_before_rewriting() {
    let h = History::new("dirty");
    h.t.write("notes.md", "edited\n");
    let before = h.refs();
    let out = h.rewrite();
    assert_error(&out);
    assert_eq!(h.refs(), before);
    assert_eq!(h.t.read("notes.md"), "edited\n");
}

#[test]
fn refs_left_by_an_earlier_rewrite_stop_before_rewriting() {
    let h = History::new("leftover");
    h.t.git(&["update-ref", "refs/commit-refs/original/heads/main", &h.c2]);
    let before = h.refs();
    let out = h.rewrite();
    assert_error(&out);
    assert!(
        stderr(&out).contains("refs/commit-refs/"),
        "{}",
        stderr(&out)
    );
    assert_eq!(h.refs(), before);
}

#[test]
fn reports_counts_and_the_map_path() {
    let h = History::new("report");
    let out = h.rewrite();
    assert_ok(&out);
    let text = stdout(&out);
    assert!(text.contains("COMMITS: 3\n"), "{text}");
    assert!(text.contains("CHANGED: 2\n"), "{text}");
    assert!(text.contains("REFS: 3\n"), "{text}");
    assert!(
        text.contains("MAP: .git/commit-refs/commit-map\n"),
        "{text}"
    );
}

#[test]
fn rewrite_history_without_a_map_is_a_usage_error() {
    let t = TempRepo::new("usage");
    let out = run(&t.dir, &["rewrite-history"], "");
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
}

/// 日時を決めて、作業ツリーの全てをコミットする
fn commit_at(t: &TempRepo, date: &str, message: &str) -> String {
    let env = [("GIT_AUTHOR_DATE", date), ("GIT_COMMITTER_DATE", date)];
    t.git(&["add", "-A"]);
    t.git_env(&env, &["commit", "-q", "-m", message]);
    t.head()
}

#[test]
fn rewrites_references_to_commits_on_other_branches_whatever_the_export_order() {
    // main の cm が、別のブランチの cs を（書き換える前の番号 X0 で）指す。cs の日時を後にして、親子の順では
    // 決まらない 2 つのコミットのどちらが先に流れても、付け直せることを確かめる
    for (name, side_date, main_date) in [
        ("side-later", "1700000200 +0000", "1700000100 +0000"),
        ("side-earlier", "1700000100 +0000", "1700000200 +0000"),
    ] {
        let t = TempRepo::new(name);
        let c0 = t.head();
        t.git(&["switch", "-q", "-c", "side"]);
        t.write("side.md", "side\n");
        let cs = commit_at(&t, side_date, "docs: side");
        t.git(&["switch", "-q", "main"]);
        t.write("main.md", &format!("cherry picked from {}\n", &X0[..8]));
        commit_at(&t, main_date, "docs: main");
        let map = t.map_file(&format!("old new\n{X0} {cs}\n"));
        let out = run(&t.dir, &["rewrite-history", &map], "");
        assert_ok(&out);
        assert_eq!(t.rev("main~1"), c0);
        assert_eq!(t.rev("side"), cs);
        assert_eq!(
            t.git(&["show", "main:main.md"]),
            format!("cherry picked from {}\n", &cs[..8])
        );
    }
}

#[test]
fn creates_a_tag_that_points_at_another_tag() {
    let h = History::new("tag-of-tag");
    h.t.git(&[
        "tag",
        "-a",
        "inner",
        "-m",
        &format!("inner {}", &X1[..8]),
        &h.c2,
    ]);
    h.t.git(&["tag", "-a", "outer", "-m", "outer", "inner"]);
    assert_ok(&h.rewrite());
    let inner = h.t.rev("inner");
    assert_eq!(h.t.rev("outer^{tag}"), h.t.rev("outer"));
    let outer = h.t.git(&["cat-file", "-p", "outer"]);
    assert!(outer.contains(&format!("object {inner}")), "{outer}");
    assert_eq!(h.t.rev("outer^{commit}"), h.t.rev("main"));
}

#[test]
fn leaves_symlink_targets_alone() {
    let h = History::new("symlink");
    std::os::unix::fs::symlink(format!("x{}", &X1[..8]), h.t.path("link")).unwrap();
    std::os::unix::fs::symlink(&X1[..8], h.t.path("link2")).unwrap();
    h.t.commit_all("chore: links");
    let before = h.t.rev("main:link2");
    assert_ok(&h.rewrite());
    assert_eq!(h.t.rev("main:link2"), before);
}

#[test]
fn a_branch_checked_out_in_another_worktree_stops_before_rewriting() {
    let h = History::new("worktrees");
    let other = h.t.dir.with_file_name(format!(
        "{}-other",
        h.t.dir.file_name().unwrap().to_string_lossy()
    ));
    h.t.git(&[
        "worktree",
        "add",
        "-q",
        "-b",
        "other",
        other.to_str().unwrap(),
    ]);
    let before = h.refs();
    let out = h.rewrite();
    let _ = fs::remove_dir_all(&other);
    assert_error(&out);
    assert!(stderr(&out).contains("worktree"), "{}", stderr(&out));
    assert_eq!(h.refs(), before);
}

#[test]
fn a_detached_head_outside_the_rewritten_refs_stops_before_rewriting() {
    let h = History::new("detached");
    h.t.git(&["switch", "-q", "--detach"]);
    h.t.write("loose.md", "loose\n");
    h.t.commit_all("docs: loose");
    let before = h.refs();
    let out = h.rewrite();
    assert_error(&out);
    assert!(stderr(&out).contains("HEAD"), "{}", stderr(&out));
    assert_eq!(h.refs(), before);
}

#[test]
fn reports_refs_that_are_not_rewritten() {
    let h = History::new("other-refs");
    h.t.git(&["update-ref", "refs/remotes/origin/main", &h.c2]);
    let out = h.rewrite();
    assert_ok(&out);
    assert!(
        stdout(&out).contains("NOT_REWRITTEN: refs/remotes/origin/main\n"),
        "{}",
        stdout(&out)
    );
}

#[test]
fn leaves_symbolic_branch_refs_to_follow_their_targets() {
    let h = History::new("symref");
    h.t.git(&["symbolic-ref", "refs/heads/alias", "refs/heads/main"]);
    let out = h.rewrite();
    assert_ok(&out);
    assert_eq!(
        h.t.git(&["symbolic-ref", "refs/heads/alias"]),
        "refs/heads/main\n"
    );
    assert_eq!(h.t.rev("alias"), h.t.rev("main"));
}

#[test]
fn an_old_commit_still_reachable_from_the_rewritten_refs_stops_before_rewriting() {
    // filter-repo を一部の ref だけに当てると、変わったとされる旧のコミットが、ほかのブランチに残る。文書がどちらを
    // 指すのか決まらない
    let h = History::new("partial");
    let map = h.t.map_file(&format!(
        "old new\n{X0} {}\n{X1} {}\n{} 4444444444444444444444444444444444444444\n",
        h.c0, h.c1, h.c2
    ));
    let before = h.refs();
    let out = h.rewrite_with(&map);
    assert_error(&out);
    assert!(stderr(&out).contains(&h.c2), "{}", stderr(&out));
    assert_eq!(h.refs(), before);
}

#[test]
fn strips_tag_signatures_from_the_last_marker_like_git() {
    let h = History::new("signature");
    let message = format!(
        "release after {}\n-----BEGIN PGP SIGNATURE-----\nquoted\n-----END PGP SIGNATURE-----\nmore\n-----BEGIN SIGNED MESSAGE-----\nsig\n",
        &X1[..8]
    );
    let file = h.t.path(".git/tag-message");
    fs::write(&file, &message).unwrap();
    h.t.git(&["tag", "-a", "v1", "-F", file.to_str().unwrap(), &h.c2]);
    assert_ok(&h.rewrite());
    let new_c1 = h.t.rev("main~1");
    let tag = h.t.git(&["cat-file", "-p", "v1"]);
    let body = tag.split_once("\n\n").unwrap().1;
    assert_eq!(
        body,
        format!(
            "release after {}\n-----BEGIN PGP SIGNATURE-----\nquoted\n-----END PGP SIGNATURE-----\nmore\n",
            &new_c1[..8]
        )
    );
}

#[test]
fn reports_reasons_in_tag_messages_together_with_those_in_commits() {
    let h = History::new("tag-reasons");
    h.t.git(&[
        "tag",
        "-a",
        "v1",
        "-m",
        &format!("after {}", &X1[..8]),
        &h.c2,
    ]);
    let map = h.t.map_file(&format!(
        "old new\n{X0} {}\n{X1} 0000000000000000000000000000000000000000\n",
        h.c0
    ));
    let out = h.rewrite_with(&map);
    assert_error(&out);
    let err = stderr(&out);
    assert!(err.contains("タグ v1 の本文"), "{err}");
    assert!(err.contains("notes.md"), "{err}");
}

#[test]
fn a_tracked_path_named_like_a_ref_does_not_confuse_the_revisions() {
    let h = History::new("path-like-ref");
    h.t.write("refs/heads/main", "a file\n");
    h.t.commit_all("chore: path like a ref");
    assert_ok(&h.rewrite());
}
