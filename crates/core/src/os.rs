//! Launching things, per platform.

use std::ffi::OsStr;
use std::io;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::Run;

pub fn run(run: &Run) -> io::Result<()> {
    match run {
        Run::OpenPath(path) => open(path.as_os_str()),
        Run::OpenUrl(url) => open(OsStr::new(url)),
        Run::RevealPath(path) => reveal(path),
        Run::Spawn { program, args } => spawn(Command::new(program).args(args)),
        Run::CopyText(_) | Run::Quit | Run::Ask(_) => Err(io::Error::other("must be handled by the UI")),
    }
}

/// Starts the command detached from our stdio and reaps it in the
/// background so it doesn't linger as a zombie.
fn spawn(command: &mut Command) -> io::Result<()> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    std::thread::spawn(move || child.wait());
    Ok(())
}

#[cfg(target_os = "macos")]
fn open(target: &OsStr) -> io::Result<()> {
    spawn(Command::new("open").arg(target))
}

#[cfg(target_os = "macos")]
fn reveal(path: &Path) -> io::Result<()> {
    spawn(Command::new("open").arg("-R").arg(path))
}

#[cfg(target_os = "windows")]
fn open(target: &OsStr) -> io::Result<()> {
    spawn(Command::new("explorer").arg(target))
}

#[cfg(target_os = "windows")]
fn reveal(path: &Path) -> io::Result<()> {
    let mut arg = std::ffi::OsString::from("/select,");
    arg.push(path);
    spawn(Command::new("explorer").arg(arg))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn open(target: &OsStr) -> io::Result<()> {
    spawn(Command::new("xdg-open").arg(target))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn reveal(path: &Path) -> io::Result<()> {
    // No portable "select this file"; open the containing folder instead.
    open(path.parent().unwrap_or(path).as_os_str())
}
