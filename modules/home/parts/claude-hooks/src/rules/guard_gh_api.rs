//! guard-gh-api（未移植: 次のコミットで bash 版から移す）

use super::Rule;
use crate::input::Input;
use crate::output::Finding;
use crate::shell::Shell;

pub struct GuardGhApi;

impl Rule for GuardGhApi {
    fn name(&self) -> &'static str {
        "guard-gh-api"
    }

    fn check(&self, _input: &Input, _shell: &Shell) -> Vec<Finding> {
        Vec::new()
    }
}
