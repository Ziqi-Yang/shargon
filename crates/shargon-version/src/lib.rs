use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn emit_build_metadata() {
    if let Some(git_dir) = git_stdout(&["rev-parse", "--git-dir"]) {
        println!("cargo:rerun-if-changed={git_dir}/HEAD");
        println!("cargo:rerun-if-changed={git_dir}/index");
    }
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");

    if let Some(git_sha) = git_stdout(&["rev-parse", "--short=12", "HEAD"]) {
        println!("cargo:rustc-env=SHARGON_GIT_SHA={git_sha}");
    }

    if let Some(status) = git_stdout(&["status", "--porcelain"]) {
        let dirty = if status.is_empty() { "false" } else { "true" };
        println!("cargo:rustc-env=SHARGON_GIT_DIRTY={dirty}");
    }

    let build_unix_ts = std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(now_unix_timestamp);
    println!("cargo:rustc-env=SHARGON_BUILD_UNIX_TS={build_unix_ts}");
}

pub fn format_version_line(
    package_name: &str,
    package_version: &str,
    git_sha: Option<&str>,
    git_dirty: Option<&str>,
    build_unix_ts: Option<&str>,
) -> String {
    let mut output = format!("{package_name} {package_version}");

    if let Some(git_sha) = git_sha {
        output.push_str(" (");
        output.push_str(git_sha);

        if let Some(git_dirty) = git_dirty
            && git_dirty == "true"
        {
            output.push_str("-dirty");
        }

        output.push(')');
    }

    if let Some(build_unix_ts) = build_unix_ts {
        output.push_str(" built@");
        output.push_str(build_unix_ts);
    }

    output
}

#[macro_export]
macro_rules! current_version_line {
    () => {
        $crate::format_version_line(
            env!("CARGO_PKG_NAME"),
            env!("CARGO_PKG_VERSION"),
            option_env!("SHARGON_GIT_SHA"),
            option_env!("SHARGON_GIT_DIRTY"),
            option_env!("SHARGON_BUILD_UNIX_TS"),
        )
    };
}

fn git_stdout(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8(output.stdout).ok()?;
    Some(stdout.trim().to_owned())
}

fn now_unix_timestamp() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}
