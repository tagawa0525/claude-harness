//! コマンドを包むもの（env など）をほどく

/// `env -S` の文字列を、env と同じ決まりで語に分ける
#[allow(dead_code)]
pub fn split_env_string(_s: &str) -> Vec<String> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_env_strings_like_env() {
        assert_eq!(
            split_env_string("git\\_commit\\_--no-verify"),
            ["git", "commit", "--no-verify"]
        );
        assert_eq!(
            split_env_string("git commit -m 'x -n'"),
            ["git", "commit", "-m", "x -n"]
        );
        assert_eq!(split_env_string("a \"b\\_c\" d\\c e"), ["a", "b c", "d"]);
    }
}
