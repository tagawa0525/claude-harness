//! guard-git-add（未移植: 次のコミットで bash 版から移す）

use super::Rule;
use crate::input::Input;
use crate::output::Finding;
use crate::shell::Shell;

pub struct GuardGitAdd;

impl Rule for GuardGitAdd {
    fn name(&self) -> &'static str {
        "guard-git-add"
    }

    fn check(&self, _input: &Input, _shell: &Shell) -> Vec<Finding> {
        Vec::new()
    }
}
