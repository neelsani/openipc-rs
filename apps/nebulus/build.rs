use std::{env, process::Command};

fn main() {
    println!("cargo:rerun-if-env-changed=OPENIPC_GIT_COMMIT");
    println!("cargo:rerun-if-env-changed=OPENIPC_GIT_TAG");

    let commit = env_value("OPENIPC_GIT_COMMIT").or_else(|| git(&["rev-parse", "HEAD"]));
    let tag = env_value("OPENIPC_GIT_TAG")
        .or_else(|| git(&["describe", "--tags", "--exact-match", "HEAD"]));

    if let Some(commit) = commit {
        println!("cargo:rustc-env=NEBULUS_GIT_COMMIT={commit}");
    }
    if let Some(tag) = tag {
        println!("cargo:rustc-env=NEBULUS_GIT_TAG={tag}");
    }

    track_git_ref();
}

fn env_value(name: &str) -> Option<String> {
    env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn git(arguments: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(env::var_os("CARGO_MANIFEST_DIR")?)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn track_git_ref() {
    let Some(git_dir) = git(&["rev-parse", "--absolute-git-dir"]) else {
        return;
    };
    println!("cargo:rerun-if-changed={git_dir}/HEAD");
    if let Some(reference) = git(&["symbolic-ref", "-q", "HEAD"]) {
        println!("cargo:rerun-if-changed={git_dir}/{reference}");
    }
}
