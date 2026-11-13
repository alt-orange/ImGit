//! OS services behind portable interfaces: child processes, pseudo consoles, shell integration and facts about
//! the user's system. Everything platform-specific in ImGit lives in this crate (and the native font loader).

pub mod process;
pub mod pseudo_console;
pub mod shell;
pub mod system;
