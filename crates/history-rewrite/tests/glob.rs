//! `--exclude` の glob が、Python の `PurePosixPath.full_match` と同じパスに当たること（ADR-0088 の案 は）。期待値は
//! Python 3.14.8 の full_match の結果

use history_rewrite::glob::full_match;

#[test]
fn globs_match_like_full_match() {
    let cases = [
        ("dict/terms.yml", "**/terms.yml", true),
        ("terms.yml", "**/terms.yml", true),
        ("a/b/terms.yml", "**/terms.yml", true),
        ("a/b.md", "*.md", false),
        ("b.md", "*.md", true),
        ("a/b/c", "a/**", true),
        ("a", "a/**", false),
        ("a/b", "a/*", true),
        ("a/b/c", "a/*", false),
        ("x.md", "[!y].md", true),
        ("y.md", "[!y].md", false),
        // 選択（{a,b}）とエスケープ（\）は持たない
        ("{a,b}.md", "{a,b}.md", true),
        ("a.md", "{a,b}.md", false),
        ("a\\b", "a\\b", true),
        // 隠れた名前にも当たる
        ("a/.hidden", "a/*", true),
        ("a/b/c.md", "a/**/*.md", true),
        ("a/c.md", "a/**/*.md", true),
        // 成分の一部の ** は * と同じ
        ("ab", "a**", true),
        ("a/b", "a**", false),
        ("a-c", "a[a-c]c", false),
        ("abc", "a[a-c]c", true),
        ("a]c", "a[]]c", true),
        // pattern は PurePosixPath として読み直す（. の成分、重なった /、末尾の / を外す）
        ("a.md", "./a.md", true),
        ("a/b", "a//b", true),
        ("a/b", "a/b/", true),
        // 文字クラスの中の集合の演算の字は、ただの字
        ("x&&y", "x[&]&y", true),
        ("b", "[a-c&&b]", true),
        ("^", "[^]", true),
        ("a", "[^]", false),
        // path も PurePosixPath として読み直す
        ("a//b", "a/b", true),
        ("./a.md", "a.md", true),
        ("a/b/", "a/b", true),
        ("a/./b", "a/b", true),
    ];
    for (path, pattern, want) in cases {
        assert_eq!(full_match(path, pattern), want, "{path:?} {pattern:?}");
    }
}
