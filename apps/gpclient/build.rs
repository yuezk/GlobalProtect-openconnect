use std::{env, fs, path::PathBuf, process::Command};

const UNKNOWN_COMMIT: &str = "unknown";
const DISPLAY_COMMIT_LEN: usize = 9;

fn main() {
  println!("cargo:rerun-if-env-changed=SOURCE_GIT_COMMIT");
  println!("cargo:rerun-if-env-changed=GPCLIENT_GIT_COMMIT");
  println!("cargo:rerun-if-env-changed=GITHUB_SHA");

  let repo_root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set")).join("../..");
  let source_commit = repo_root.join("SOURCE_COMMIT");
  println!("cargo:rerun-if-changed={}", source_commit.display());

  let commit = env_commit("GPCLIENT_GIT_COMMIT")
    .or_else(|| env_commit("SOURCE_GIT_COMMIT"))
    .or_else(|| env_commit("GITHUB_SHA"))
    .or_else(|| file_commit(&source_commit))
    .or_else(|| git_commit(&repo_root))
    .unwrap_or_else(|| UNKNOWN_COMMIT.to_string());

  println!("cargo:rustc-env=GPCLIENT_GIT_COMMIT={commit}");
}

fn env_commit(key: &str) -> Option<String> {
  env::var(key).ok().and_then(|value| normalize_commit(&value))
}

fn file_commit(path: &PathBuf) -> Option<String> {
  fs::read_to_string(path).ok().and_then(|value| normalize_commit(&value))
}

fn git_commit(repo_root: &PathBuf) -> Option<String> {
  let output = Command::new("git")
    .args(["rev-parse", "--short=12", "HEAD"])
    .current_dir(repo_root)
    .output()
    .ok()?;

  if !output.status.success() {
    return None;
  }

  String::from_utf8(output.stdout)
    .ok()
    .and_then(|value| normalize_commit(&value))
}

fn normalize_commit(value: &str) -> Option<String> {
  let value = value.trim();
  if value.is_empty() {
    return None;
  }

  Some(value.chars().take(DISPLAY_COMMIT_LEN).collect())
}
