//! run の終わりに書く、古いオブジェクトと控えの掃除の issue の本文（ADR-0045）。掃除は取り消せないので、ツールは
//! 行わず、ユーザーが決める issue にする

use crate::{Stop, Words, git, sh_quote};
use std::path::Path;

/// issue の本文。1 行目の見出し（`#` の後）が題
pub fn body(source: &Path, workdir: &Path, words: &Words) -> Result<String, Stop> {
    let (source_q, workdir_q) = (sh_quote(source), sh_quote(workdir));
    let mut out = String::new();
    out.push_str("# 履歴の書き換えの控えと古いオブジェクトを掃除する\n\n");
    out.push_str(&format!(
        "{} のすべてのブランチとタグの語を history-rewrite で直し、番号を付け直した。書き換える前のオブジェクトは、reflog と作業ディレクトリの控えに残っている。\n\n",
        source.display()
    ));
    out.push_str(&format!(
        "- 書き換えた日: {}（history-rewrite run を実行した日）\n",
        today(source)
    ));
    out.push_str(&format!("- 作業ディレクトリ: {}\n", workdir.display()));
    out.push_str(&format!(
        "- 控え（書き換える前のミラー）: {}\n",
        workdir.join("original.git").display()
    ));
    out.push_str(&format!(
        "- 番号の対応表: {}\n",
        workdir.join("commit-map").display()
    ));
    let shown = |paths: &[std::path::PathBuf]| {
        paths
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join("、")
    };
    let dictionaries = if words.dictionaries.is_empty() {
        "ja-terms の共通の辞書（claude-setup sync が配ったもの）".to_string()
    } else {
        shown(&words.dictionaries)
    };
    out.push_str(&format!("- 辞書: {dictionaries}\n"));
    if !words.repo_dictionaries.is_empty() {
        out.push_str(&format!(
            "- リポジトリの辞書（辞書に重ねたもの）: {}\n",
            shown(&words.repo_dictionaries)
        ));
    }
    let excludes = if words.excludes.is_empty() {
        "なし".to_string()
    } else {
        words
            .excludes
            .iter()
            .map(|g| format!("`{g}`"))
            .collect::<Vec<_>>()
            .join("、")
    };
    out.push_str(&format!("- 除くパス: {excludes}\n"));
    if words.extra_rules.is_empty() {
        out.push_str("- 追加の規則: なし\n");
    }
    for path in &words.extra_rules {
        // 追加の規則は 1 回限りの一時のファイルのことが多いので、消えても分かるよう中身をコピーする
        let rules = std::fs::read_to_string(path)
            .map_err(|e| Stop(format!("{}: 読めない: {e}", path.display())))?;
        out.push_str(&format!(
            "- 追加の規則: {}\n\n````yaml\n{}\n````\n",
            path.display(),
            rules.trim_end()
        ));
    }
    out.push_str(&format!(
        "\n## 完了の条件\n\n\
         ユーザーが、書き換えた後の履歴で困らないと判断したら、次を行う。どちらも取り消せない（書き換える前の履歴に戻せなくなる）。\n\n\
         ```sh\n\
         git -C {source_q} reflog expire --expire=now --all && git -C {source_q} gc --prune=now && rm -rf {workdir_q}\n\
         ```\n\n\
         作業ディレクトリを消す前に、ほかのリポジトリとメモリの番号を、対応表で付け直し終えていることを確かめる（消すと対応表も無くなる）。\n"
    ));
    Ok(out)
}

/// 今日の日付（YYYY-MM-DD）。git の committer の時刻と時差（`git var GIT_COMMITTER_IDENT`）で、利用者の暦の日にする。
/// committer を引けなければ、UTC の日付に「（UTC）」を添える
fn today(repo: &Path) -> String {
    let ident = git(repo, &["var", "GIT_COMMITTER_IDENT"])
        .ok()
        .map(|out| String::from_utf8_lossy(&out).trim().to_string());
    let parsed = ident.as_deref().and_then(|line| {
        let mut fields = line.rsplitn(3, ' ');
        let offset = fields.next()?;
        let seconds: i64 = fields.next()?.parse().ok()?;
        let sign = match offset.as_bytes().first()? {
            b'+' => 1,
            b'-' => -1,
            _ => return None,
        };
        let hours: i64 = offset.get(1..3)?.parse().ok()?;
        let minutes: i64 = offset.get(3..5)?.parse().ok()?;
        Some(seconds + sign * (hours * 3600 + minutes * 60))
    });
    match parsed {
        Some(local) => date(local),
        None => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs() as i64);
            format!("{}（UTC）", date(now))
        }
    }
}

/// 1970-01-01 からの秒を暦の日付にする（Howard Hinnant の civil_from_days）
pub fn date(seconds: i64) -> String {
    let z = seconds.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}
