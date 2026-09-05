//! warn-large-commit（未移植: 次のコミットで bash 版から移す）

use super::Rule;
use crate::input::Input;
use crate::output::Finding;
use crate::shell::Shell;

pub struct WarnLargeCommit;

impl Rule for WarnLargeCommit {
    fn name(&self) -> &'static str {
        "warn-large-commit"
    }

    fn check(&self, _input: &Input, _shell: &Shell) -> Vec<Finding> {
        Vec::new()
    }
}
