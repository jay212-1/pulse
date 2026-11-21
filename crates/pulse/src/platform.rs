//! The few things Pulse asks the operating system for: opening a file or
//! folder with its usual program, and a short alert sound (show mode, only
//! when switched on). Failures are ignored: neither may stop the window.

use std::path::Path;
use std::process::{Command, Stdio};

fn spawn(prog: &str, args: &[&str]) -> bool {
    Command::new(prog)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .is_ok()
}

/// Open a file or folder as a double-click would.
pub fn open(path: &Path) {
    let p = path.to_string_lossy();
    if cfg!(target_os = "macos") {
        spawn("open", &[&p]);
    } else if cfg!(windows) {
        spawn("explorer", &[&p]);
    } else {
        spawn("xdg-open", &[&p]);
    }
}

/// A short alert sound.
pub fn alert() {
    if cfg!(target_os = "macos") {
        spawn("afplay", &["/System/Library/Sounds/Funk.aiff"]);
    } else if cfg!(windows) {
        spawn(
            "powershell",
            &[
                "-NoProfile",
                "-Command",
                "[System.Media.SystemSounds]::Exclamation.Play()",
            ],
        );
    } else if !spawn("canberra-gtk-play", &["-i", "dialog-warning"]) {
        spawn(
            "paplay",
            &["/usr/share/sounds/freedesktop/stereo/dialog-warning.oga"],
        );
    }
}
