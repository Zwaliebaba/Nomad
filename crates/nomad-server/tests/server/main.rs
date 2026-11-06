//! Integration tests of the server shell (N2 plan §7). One test binary, so the bundled
//! `SQLite` is linked once. Every test that runs a server in-process uses a fake clock, so
//! none waits for real time to pass; only the Ctrl+C test runs the real binary, for well
//! under a second.

mod catch_up;
mod checkpoints;
mod cli;
mod common;
mod corrupt;
mod lock;
mod net;
mod recovery;
mod retention;
mod season_end;
