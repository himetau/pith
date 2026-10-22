// what pith reads and runs off the machine: pi processes, session files, settings, git, the pty, the extension's bridge.
// none of it knows about the ui
pub mod bridge;
pub mod browser;
pub mod imagegen;
pub mod pi;
pub mod preview;
pub mod sessions;
pub mod sys;
pub mod term;
pub mod web;
