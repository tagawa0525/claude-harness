//! 結合テストの共通の部品（git hook と git の起動、`claude-hooks script` の偽の gh）

#[allow(dead_code)]
pub mod fake_gh;

use std::path::{Path, PathBuf};
use std::process::Command;

/// examples/fake_tool の実行ファイル（target/<profile>/examples にある）。`cargo test` は examples も
/// ビルドする（`--test` で絞ると作らないので、先に `cargo build --examples` を流す）。
/// profile の場所は、cargo が渡す claude-hooks の実行ファイルの隣とする。テストの実行ファイルの場所から数えない
/// （cargo 1.100 は、テストの実行ファイルを target/<profile>/deps でなく target/<profile>/build/<crate>/<hash>/out に置く）
#[allow(dead_code)]
pub fn fake_tool() -> PathBuf {
    let profile = Path::new(env!("CARGO_BIN_EXE_claude-hooks"))
        .parent()
        .unwrap();
    profile
        .join("examples")
        .join(format!("fake_tool{}", std::env::consts::EXE_SUFFIX))
}

/// 偽のコマンド（[`fake_tool`]）を dest にハードリンクで置く。コピーすると書き込みに開いた fd が、並行するテストが
/// その間に fork した子に exec まで残り、置いたコマンドの起動が ETXTBSY（Text file busy）で失敗しうる（リンクは書き込みに
/// 開かない）。置き場（CARGO_TARGET_TMPDIR）は fake_tool と同じ target の下にあるので、リンクできる。置いたパスは
/// fake_tool と同じファイルなので、置いた後に書き込まない
#[allow(dead_code)]
pub fn place_fake_tool(dest: &Path) {
    match std::fs::remove_file(dest) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            panic!("{} を置き直せない: {e}", dest.display())
        }
        _ => {}
    }
    std::fs::hard_link(fake_tool(), dest)
        .unwrap_or_else(|e| panic!("{} にリンクできない: {e}", dest.display()));
}

/// sh に書くパスと、git の書くパス（Windows でも `/` で区切る）と比べるパス。Windows の `Path::display()` は `\` で
/// 区切り、`\` は sh の中で消えるので `/` にする（tests/test_source_paths.rs が、format! に display() を直に使う形を止める）
#[allow(dead_code)]
pub fn sh_path(path: &Path) -> String {
    path.display().to_string().replace('\\', "/")
}

/// 実行の権限を付ける（Windows には無い）
#[allow(dead_code)]
pub fn executable(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    #[cfg(not(unix))]
    let _ = path;
}

/// 実行するファイルを書いて実行の権限を付け、起動できるまで待つ。並行するテストが fork した子が、書き込みに開いた
/// fd を exec まで持つと、書いた直後の起動が ETXTBSY（Text file busy）で失敗しうる（Linux）。
///
/// 待ち方は、実際に起動を試して ETXTBSY（ErrorKind::ExecutableFileBusy）が消えるまで繰り返す（check_commit_message.rs のテストと同じ）。
/// 起動の試しで hook の副作用（マーカーの追記、呼び出しの記録）を起こさないために、次の作りにする。
/// - shebang のあるスクリプトは、1 行目の後ろに `CLAUDE_HOOKS_TEST_PROBE` が設定されていたら exit 0 する行を足して書く。
///   試しはこの変数つきで起動するので、本文は走らない（試しの後の本物の起動は変数が無く、本文が走る）
/// - shebang の無いファイルは、書いた内容に足さない。カーネルは ETXTBSY を shebang や形式の判定より先に返すので、
///   busy でなければ ENOEXEC で失敗し、何も走らない
///
/// 代わりの案: /proc で書き込みに開いたファイルを探す方式は Linux 専用で、他の OS で待てない。短い固定の sleep は
/// 待ちきれないことがある。どちらも採らない
#[allow(dead_code)]
pub fn write_executable(path: &Path, content: impl AsRef<str>) {
    let content = content.as_ref();
    let content = match content.split_once('\n') {
        Some((first, rest)) if first.starts_with("#!") => {
            format!("{first}\n[ -z \"$CLAUDE_HOOKS_TEST_PROBE\" ] || exit 0\n{rest}")
        }
        _ => content.to_string(),
    };
    std::fs::write(path, content).unwrap();
    executable(path);
    #[cfg(unix)]
    for _ in 0..200 {
        match Command::new(path)
            .env("CLAUDE_HOOKS_TEST_PROBE", "1")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
        {
            Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy => {
                std::thread::sleep(std::time::Duration::from_millis(10))
            }
            _ => return,
        }
    }
    #[cfg(unix)]
    panic!("{} が 2 秒たっても起動できない（ETXTBSY）", path.display());
}

/// 呼んだ側のリポジトリ（run-checks.sh を実行する git の hook の中の GIT_DIR、GIT_INDEX_FILE など）を見せない。
/// 残すと、テストの git が外のリポジトリに当たる
#[allow(dead_code)]
pub fn forget_git_env(cmd: &mut Command) -> &mut Command {
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            cmd.env_remove(key);
        }
    }
    cmd
}

/// 待っているジョブ（番号、名前、パーティション、優先度、コメント）の待ち行列を、`squeue --json=v0.0.45` の形で
/// （slurm-queue と slurm-run の偽の squeue の出力）。見積もりは 10 分、開始の見込みは無い
#[allow(dead_code)]
pub fn squeue_json(jobs: &[(u64, &str, &str, u64, &str)]) -> String {
    let number = |n: u64| serde_json::json!({"set": true, "infinite": false, "number": n});
    let unset = serde_json::json!({"set": false, "infinite": false, "number": 0});
    let jobs: Vec<_> = jobs
        .iter()
        .map(|(id, name, partition, priority, comment)| {
            serde_json::json!({
                "job_id": id,
                "name": name,
                "job_state": ["PENDING"],
                "partition": partition,
                "priority": number(*priority),
                "time_limit": number(10),
                "start_time": number(0),
                "comment": comment,
                "array_job_id": number(0),
                "array_task_id": unset,
                "array_task_string": "",
                "het_job_id": number(0),
                "het_job_offset": unset,
            })
        })
        .collect();
    serde_json::json!({"jobs": jobs}).to_string()
}
