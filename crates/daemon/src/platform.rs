//! The few things that differ between Unix and Windows.

use std::path::{Path, PathBuf};

pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
}

/// The user's shell. On Windows, `$SHELL` still wins (Git Bash and MSYS set it),
/// and the fallback is PowerShell.
fn shell() -> Option<String> {
    std::env::var("SHELL").ok().filter(|s| !s.is_empty())
}

/// How to run `script` in the user's shell, as a login shell where that exists.
pub fn shell_command(script: &str) -> (String, Vec<String>) {
    match shell() {
        Some(shell) => (shell, vec!["-lc".into(), script.into()]),
        None if cfg!(windows) => (
            "powershell.exe".into(),
            vec!["-NoLogo".into(), "-Command".into(), script.into()],
        ),
        None => ("/bin/sh".into(), vec!["-lc".into(), script.into()]),
    }
}

/// How to start the user's shell interactively.
pub fn interactive_shell() -> (String, Vec<String>) {
    match shell() {
        Some(shell) => (shell, vec!["-l".into()]),
        None if cfg!(windows) => ("powershell.exe".into(), vec!["-NoLogo".into()]),
        None => ("/bin/sh".into(), vec!["-l".into()]),
    }
}

/// `script` with stderr folded into stdout, so output reads in order.
pub fn with_merged_output(script: &str) -> String {
    if shell().is_none() && cfg!(windows) {
        format!("& {{ {script} }} 2>&1")
    } else {
        format!("{{ {script}\n}} 2>&1")
    }
}

/// Quotes one word for the shell that [`shell_command`] runs.
pub fn shell_quote(s: &str) -> String {
    if s.chars()
        .all(|c| c.is_ascii_alphanumeric() || "/._-".contains(c))
    {
        s.to_string()
    } else if shell().is_none() && cfg!(windows) {
        format!("'{}'", s.replace('\'', "''"))
    } else {
        format!("'{}'", s.replace('\'', r"'\''"))
    }
}

/// A shell command that runs a program, whose path may be quoted. PowerShell
/// treats a bare quoted string as a value unless told to call it.
pub fn call(quoted_program: &str) -> String {
    if shell().is_none() && cfg!(windows) {
        format!("& {quoted_program}")
    } else {
        quoted_program.to_string()
    }
}

/// The file to execute for an agent CLI. npm installs them on Windows as `.cmd`
/// shims, which process creation won't find from a bare name.
pub fn program(bin: &Path) -> PathBuf {
    if !cfg!(windows) || bin.extension().is_some() {
        return bin.to_path_buf();
    }
    let paths = std::env::var_os("PATH").unwrap_or_default();
    let dirs: Vec<PathBuf> = if bin.parent().is_some_and(|p| !p.as_os_str().is_empty()) {
        vec![bin.parent().map(Path::to_path_buf).unwrap_or_default()]
    } else {
        std::env::split_paths(&paths).collect()
    };
    let Some(name) = bin.file_name() else {
        return bin.to_path_buf();
    };
    for dir in dirs {
        for extension in ["exe", "cmd", "bat"] {
            let candidate = dir.join(name).with_extension(extension);
            if candidate.is_file() {
                return candidate;
            }
        }
    }
    bin.to_path_buf()
}

/// `canonicalize`, without Windows' `\\?\` prefix, which git doesn't understand.
pub fn canonical(path: &Path) -> std::io::Result<PathBuf> {
    let path = path.canonicalize()?;
    let text = path.to_string_lossy();
    Ok(match text.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC") => PathBuf::from(rest),
        _ => path,
    })
}

/// Opens a URL in the default browser.
pub fn open_url(url: &str) {
    let mut command = if cfg!(target_os = "macos") {
        std::process::Command::new("open")
    } else if cfg!(windows) {
        let mut c = std::process::Command::new("rundll32");
        c.arg("url.dll,FileProtocolHandler");
        c
    } else {
        std::process::Command::new("xdg-open")
    };
    let _ = command.arg(url).status();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_nothing_it_should_not() {
        let dir = tempfile::tempdir().unwrap();
        let path = canonical(dir.path()).unwrap();
        assert!(path.is_absolute());
        assert!(!path.to_string_lossy().starts_with(r"\\?\"));
    }

    #[cfg(unix)]
    #[test]
    fn leaves_unix_programs_alone() {
        assert_eq!(program(Path::new("claude")), PathBuf::from("claude"));
        assert_eq!(call("'/my apps/claude'"), "'/my apps/claude'");
    }
}
