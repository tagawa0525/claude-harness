//! hook が呼ぶハーネスの外のコマンド（ja-terms、commit-refs）の見つけ方（ADR-0085、ADR-0086）

/// var の環境変数のパスが先（空の値はパスでない）。Linux では package.nix のラッパーが store のパスを固定する（hook は
/// Claude Code や利用者の端末の git が起動するので、利用者の PATH に頼らない）。無ければ、Nix のラッパーの無い Windows
/// （cargo install で入れる。ADR-0084）は name で起動する（Windows の起動は、PATH の前に claude-hooks の置き場と
/// システムのディレクトリも探す。cargo install の置き場は同じ）。Linux で環境変数が無いのは cargo で直接ビルドした
/// もので、None を返す（PATH の別の版で判定しない）。None のときの扱いは呼び出し元が決める
pub fn find(var: &str, name: &str) -> Option<String> {
    from(std::env::var(var).ok(), cfg!(windows), name)
}

pub fn from(env: Option<String>, windows: bool, name: &str) -> Option<String> {
    env.filter(|path| !path.is_empty())
        .or_else(|| windows.then(|| name.to_string()))
}
