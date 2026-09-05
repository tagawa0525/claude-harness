//! guard-git-push（未移植: 次のコミットで bash 版から移す）

use super::Rule;
use crate::input::Input;
use crate::output::Finding;
use crate::shell::Shell;

pub struct GuardGitPush;

impl Rule for GuardGitPush {
    fn name(&self) -> &'static str {
        "guard-git-push"
    }

    fn check(&self, _input: &Input, _shell: &Shell) -> Vec<Finding> {
        Vec::new()
    }
}
