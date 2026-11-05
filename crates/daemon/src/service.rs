//! Run `bantud` as a per-user background service so agents keep working
//! after the terminal or app that started them closes: a launchd agent on
//! macOS, a systemd user unit on Linux.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

pub const LABEL: &str = "dev.bantu.bantud";

/// Settings copied from the installing shell into the service, so e.g.
/// `BANTU_PUBLIC_URL=… bantud service install` sticks across restarts.
const PERSISTED_ENV: &[&str] = &[
    "BANTUD_PORT",
    "BANTU_PUBLIC_URL",
    "BANTU_APP_URL",
    "BANTUD_PREVIEW_PORT",
    "BANTU_PREVIEW_URL",
    "BANTU_ALLOWED_HOSTS",
    "BANTU_WORKSPACES_DIR",
    "BANTU_CLAUDE_BIN",
    "BANTU_CODEX_BIN",
    "BANTU_OPENCODE_BIN",
    "BANTU_GEMINI_BIN",
    "BANTU_CURSOR_BIN",
    "BANTU_BRANCH_PREFIX",
];

fn persisted_env(home: &Path) -> Vec<(String, String)> {
    let mut env = vec![("BANTU_HOME".to_string(), home.to_string_lossy().to_string())];
    for key in PERSISTED_ENV {
        if let Ok(v) = std::env::var(key) {
            env.push((key.to_string(), v));
        }
    }
    env
}

pub fn install(exe: &Path, home: &Path) -> Result<()> {
    let user_home = user_home()?;
    let logs = home.join("logs");
    std::fs::create_dir_all(&logs)?;
    if cfg!(target_os = "macos") {
        let plist = launchd_plist_path(&user_home);
        std::fs::create_dir_all(plist.parent().unwrap())?;
        std::fs::write(&plist, launchd_plist(exe, &persisted_env(home), &logs))?;
        let domain = launchd_domain()?;
        // Replace any previous version; bootout fails harmlessly if absent.
        let _ = Command::new("launchctl")
            .args(["bootout", &format!("{domain}/{LABEL}")])
            .output();
        run(
            "launchctl",
            &["bootstrap", &domain, &plist.to_string_lossy()],
        )?;
        println!("Installed launchd agent {}", plist.display());
    } else if cfg!(target_os = "linux") {
        let unit = systemd_unit_path(&user_home);
        std::fs::create_dir_all(unit.parent().unwrap())?;
        std::fs::write(&unit, systemd_unit(exe, &persisted_env(home)))?;
        run("systemctl", &["--user", "daemon-reload"])?;
        run(
            "systemctl",
            &["--user", "enable", "--now", "bantud.service"],
        )?;
        println!("Installed systemd user unit {}", unit.display());
        println!(
            "On a server you log out of, also run `sudo loginctl enable-linger $USER` \
             so bantud keeps running."
        );
    } else {
        bail!("`bantud service` supports macOS and Linux");
    }
    println!("Logs: {}", logs.display());
    Ok(())
}

pub fn uninstall() -> Result<()> {
    let user_home = user_home()?;
    if cfg!(target_os = "macos") {
        let _ = Command::new("launchctl")
            .args(["bootout", &format!("{}/{LABEL}", launchd_domain()?)])
            .output();
        let plist = launchd_plist_path(&user_home);
        if plist.exists() {
            std::fs::remove_file(&plist)?;
        }
    } else if cfg!(target_os = "linux") {
        let _ = Command::new("systemctl")
            .args(["--user", "disable", "--now", "bantud.service"])
            .output();
        let unit = systemd_unit_path(&user_home);
        if unit.exists() {
            std::fs::remove_file(&unit)?;
        }
        let _ = Command::new("systemctl")
            .args(["--user", "daemon-reload"])
            .output();
    }
    println!("Removed the bantud service. Your data in ~/.bantu is untouched.");
    Ok(())
}

pub fn status() -> Result<()> {
    let ok = if cfg!(target_os = "macos") {
        Command::new("launchctl")
            .args(["print", &format!("{}/{LABEL}", launchd_domain()?)])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    } else {
        Command::new("systemctl")
            .args(["--user", "is-active", "--quiet", "bantud.service"])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    };
    println!(
        "{}",
        if ok {
            "bantud service: running"
        } else {
            "bantud service: not running"
        }
    );
    Ok(())
}

fn launchd_plist_path(user_home: &Path) -> PathBuf {
    user_home.join(format!("Library/LaunchAgents/{LABEL}.plist"))
}

fn systemd_unit_path(user_home: &Path) -> PathBuf {
    user_home.join(".config/systemd/user/bantud.service")
}

pub fn launchd_plist(exe: &Path, env: &[(String, String)], logs: &Path) -> String {
    let env: String = env
        .iter()
        .map(|(k, v)| {
            format!(
                "<key>{}</key><string>{}</string>",
                xml_escape(k),
                xml_escape(v)
            )
        })
        .collect();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{LABEL}</string>
  <key>ProgramArguments</key>
  <array><string>{exe}</string></array>
  <key>EnvironmentVariables</key>
  <dict>{env}</dict>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>ProcessType</key><string>Interactive</string>
  <key>StandardOutPath</key><string>{logs}/bantud.log</string>
  <key>StandardErrorPath</key><string>{logs}/bantud.log</string>
</dict>
</plist>
"#,
        exe = xml_escape(&exe.to_string_lossy()),
        logs = xml_escape(&logs.to_string_lossy()),
    )
}

pub fn systemd_unit(exe: &Path, env: &[(String, String)]) -> String {
    let env: String = env
        .iter()
        .map(|(k, v)| format!("Environment=\"{k}={}\"\n", v.replace('"', "\\\"")))
        .collect();
    format!(
        "[Unit]\n\
         Description=Bantu daemon (coding agents in git worktrees)\n\
         After=network-online.target\n\n\
         [Service]\n\
         ExecStart=\"{exe}\"\n\
         {env}\
         Restart=on-failure\n\
         RestartSec=3\n\n\
         [Install]\n\
         WantedBy=default.target\n",
        exe = exe.display(),
    )
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn user_home() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME is not set")
}

fn launchd_domain() -> Result<String> {
    let out = Command::new("id")
        .arg("-u")
        .output()
        .context("running `id -u`")?;
    Ok(format!(
        "gui/{}",
        String::from_utf8_lossy(&out.stdout).trim()
    ))
}

fn run(cmd: &str, args: &[&str]) -> Result<()> {
    let out = Command::new(cmd)
        .args(args)
        .output()
        .with_context(|| format!("running {cmd}"))?;
    if !out.status.success() {
        bail!(
            "{cmd} {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_service_files() {
        let env = vec![
            ("BANTU_HOME".to_string(), "/u/.bantu".to_string()),
            (
                "BANTU_PUBLIC_URL".to_string(),
                "https://m.ts.net".to_string(),
            ),
        ];
        let plist = launchd_plist(
            Path::new("/opt/b&b/bantud"),
            &env,
            Path::new("/u/.bantu/logs"),
        );
        assert!(plist.contains("<string>/opt/b&amp;b/bantud</string>"));
        assert!(plist.contains("<key>BANTU_PUBLIC_URL</key><string>https://m.ts.net</string>"));
        assert!(plist.contains("<key>KeepAlive</key><true/>"));
        assert!(plist.contains("/u/.bantu/logs/bantud.log"));

        let env = vec![("BANTU_HOME".to_string(), "/home/u/.bantu".to_string())];
        let unit = systemd_unit(Path::new("/home/u/.local/bin/bantud"), &env);
        assert!(unit.contains(
            "ExecStart=\"/home/u/.local/bin/bantud\"\nEnvironment=\"BANTU_HOME=/home/u/.bantu\"\nRestart"
        ));
        assert!(unit.contains("WantedBy=default.target"));
    }
}
