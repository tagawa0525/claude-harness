//! require-background-wait（未移植: 次のコミットで bash 版から移す）

use super::Rule;
use crate::input::Input;
use crate::output::Finding;
use crate::shell::Shell;

pub struct RequireBackgroundWait;

impl Rule for RequireBackgroundWait {
    fn name(&self) -> &'static str {
        "require-background-wait"
    }

    fn check(&self, _input: &Input, _shell: &Shell) -> Vec<Finding> {
        Vec::new()
    }
}
