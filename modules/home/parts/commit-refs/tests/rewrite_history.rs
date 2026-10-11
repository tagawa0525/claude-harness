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
fn a_reference_to_a_commit_not_yet_rewritten_stops_without_moving_refs() {
    // c1 の文書が、子の c2 を書き換える前の番号（X2）で指す。親から順に流すので、c1 を渡すときに c2 の最後の番号は
    // まだ無い
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
    assert!(stderr(&out).contains(&X2[..8]), "{}", stderr(&out));
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
