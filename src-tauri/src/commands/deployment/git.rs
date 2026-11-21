use serde::Serialize;
use std::{
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitCommit {
    pub hash: String,
    pub message: String,
    pub timestamp: i64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitActivity {
    pub repository_found: bool,
    pub observed_at: i64,
    pub repository_root: Option<String>,
    pub branch: Option<String>,
    pub remote: Option<String>,
    pub staged_files: Vec<String>,
    pub modified_files: Vec<String>,
    pub untracked_files: Vec<String>,
    pub ahead: Option<u32>,
    pub behind: Option<u32>,
    pub synchronization: String,
    pub latest_commit: Option<GitCommit>,
}

pub fn inspect(selected_path: &str) -> Result<GitActivity, String> {
    let path = super::project::canonical_project_path(selected_path)?;
    let root = run_git(&path, &["rev-parse", "--show-toplevel"])?;
    if !root.status.success() {
        return Ok(GitActivity {
            repository_found: false,
            observed_at: now_seconds(),
            repository_root: None,
            branch: None,
            remote: None,
            staged_files: Vec::new(),
            modified_files: Vec::new(),
            untracked_files: Vec::new(),
            ahead: None,
            behind: None,
            synchronization: "No Git repository".to_string(),
            latest_commit: None,
        });
    }

    let repository_root = String::from_utf8_lossy(&root.stdout).trim().to_string();
    let repository_path = Path::new(&repository_root);
    let branch_output = run_git(repository_path, &["branch", "--show-current"])?;
    let branch_value = String::from_utf8_lossy(&branch_output.stdout)
        .trim()
        .to_string();
    let branch = if branch_value.is_empty() {
        Some("Detached HEAD".to_string())
    } else {
        Some(branch_value)
    };

    let status = run_git(
        repository_path,
        &[
            "status",
            "--porcelain=v1",
            "--branch",
            "--untracked-files=all",
            "-z",
        ],
    )?;
    if !status.status.success() {
        return Err(format!(
            "Git could not inspect the selected repository: {}",
            String::from_utf8_lossy(&status.stderr).trim()
        ));
    }
    let status_text = String::from_utf8_lossy(&status.stdout);
    let (ahead, behind) = tracking_counts(&status_text);
    let (staged_files, modified_files, untracked_files) = classify_files(&status_text);

    let remote_output = run_git(repository_path, &["remote", "-v"])?;
    let remote = String::from_utf8_lossy(&remote_output.stdout)
        .lines()
        .find_map(|line| line.split_whitespace().nth(1))
        .map(str::to_string);
    let commit_output = run_git(repository_path, &["log", "-1", "--format=%H%x1f%s%x1f%ct"])?;
    let latest_commit = if commit_output.status.success() {
        parse_commit(&String::from_utf8_lossy(&commit_output.stdout))
    } else {
        None
    };

    let synchronization = if remote.is_none() {
        "No origin remote configured".to_string()
    } else if ahead.is_none() && behind.is_none() {
        "No upstream tracking branch configured".to_string()
    } else {
        match (ahead.unwrap_or_default(), behind.unwrap_or_default()) {
            (0, 0) => "In sync with last fetched upstream".to_string(),
            (ahead, 0) => format!("{ahead} local commit(s) ahead; push not confirmed"),
            (0, behind) => format!("{behind} commit(s) behind last fetched upstream"),
            (ahead, behind) => format!("{ahead} ahead and {behind} behind last fetched upstream"),
        }
    };

    Ok(GitActivity {
        repository_found: true,
        observed_at: now_seconds(),
        repository_root: Some(repository_root),
        branch,
        remote,
        staged_files,
        modified_files,
        untracked_files,
        ahead,
        behind,
        synchronization,
        latest_commit,
    })
}

fn now_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn run_git(path: &Path, args: &[&str]) -> Result<std::process::Output, String> {
    Command::new("git")
        .args(args)
        .current_dir(path)
        .output()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                "Git CLI was not found on PATH.".to_string()
            } else {
                format!("Could not run a read-only Git inspection: {error}")
            }
        })
}

fn tracking_counts(status: &str) -> (Option<u32>, Option<u32>) {
    let header = status.lines().find(|line| line.starts_with("## "));
    let Some(header) = header else {
        return (None, None);
    };
    let Some((_, upstream)) = header.split_once("...") else {
        return (None, None);
    };
    let ahead = upstream
        .split_once("[ahead ")
        .and_then(|(_, count)| {
            count
                .split(|character: char| !character.is_ascii_digit())
                .next()
        })
        .and_then(|count| count.parse().ok())
        .unwrap_or(0);
    let behind = upstream
        .split_once("behind ")
        .and_then(|(_, count)| {
            count
                .split(|character: char| !character.is_ascii_digit())
                .next()
        })
        .and_then(|count| count.parse().ok())
        .unwrap_or(0);
    (Some(ahead), Some(behind))
}

fn classify_files(status: &str) -> (Vec<String>, Vec<String>, Vec<String>) {
    let mut staged = Vec::new();
    let mut modified = Vec::new();
    let mut untracked = Vec::new();
    let mut records = status.split('\0').peekable();
    while let Some(token) = records.next() {
        for record in token.lines().filter(|line| !line.starts_with("## ")) {
            if record.len() < 4 {
                continue;
            }
            let status_code = &record[..2];
            let filename = record[3..].to_string();
            if status_code == "??" {
                untracked.push(filename);
            } else {
                if status_code.as_bytes()[0] != b' ' {
                    staged.push(filename.clone());
                }
                if status_code.as_bytes()[1] != b' ' {
                    modified.push(filename);
                }
                if status_code.bytes().any(|byte| byte == b'R' || byte == b'C') {
                    records.next();
                }
            }
        }
    }
    (staged, modified, untracked)
}

fn parse_commit(value: &str) -> Option<GitCommit> {
    let mut parts = value.trim().splitn(3, '\u{1f}');
    Some(GitCommit {
        hash: parts.next()?.to_string(),
        message: parts.next()?.to_string(),
        timestamp: parts.next()?.parse().ok()?,
    })
}

#[cfg(test)]
mod tests {
    use super::{classify_files, inspect, parse_commit, tracking_counts};

    #[test]
    fn parses_staged_modified_and_untracked_git_status() {
        let status =
            "## main...origin/main [ahead 2, behind 1]\0M  staged.ts\0 M working.ts\0?? new file.ts\0";
        assert_eq!(tracking_counts(status), (Some(2), Some(1)));
        let (staged, modified, untracked) = classify_files(status);
        assert_eq!(staged, vec!["staged.ts"]);
        assert_eq!(modified, vec!["working.ts"]);
        assert_eq!(untracked, vec!["new file.ts"]);
    }

    #[test]
    fn parses_latest_commit_fields_without_claiming_push_state() {
        let commit = parse_commit("abc123\u{1f}Improve deploy flow\u{1f}1700000000\n").unwrap();
        assert_eq!(commit.hash, "abc123");
        assert_eq!(commit.message, "Improve deploy flow");
        assert_eq!(commit.timestamp, 1_700_000_000);
    }

    #[test]
    fn inspects_a_temporary_repository_without_staging_or_committing_changes() {
        use std::{fs, path::Path, process::Command};

        let root = std::env::temp_dir().join(format!(
            "cezecore-git-monitor-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let remote = root.with_extension("git");
        fs::create_dir_all(&root).unwrap();
        let git = |directory: &Path, args: &[&str]| {
            let output = Command::new("git")
                .args(args)
                .current_dir(directory)
                .output()
                .expect("Git is installed for the local repository test");
            assert!(
                output.status.success(),
                "git {args:?} failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            output
        };

        git(&root, &["init"]);
        git(&root, &["config", "user.name", "CEZECORE test"]);
        git(&root, &["config", "user.email", "test@example.invalid"]);
        git(&root, &["branch", "-M", "main"]);
        fs::write(root.join("base.txt"), "base").unwrap();
        git(&root, &["add", "base.txt"]);
        git(&root, &["commit", "-m", "temporary baseline"]);
        fs::create_dir_all(&remote).unwrap();
        git(&remote, &["init", "--bare"]);
        git(
            &root,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        git(&root, &["push", "-u", "origin", "main"]);

        fs::write(root.join("staged.ts"), "staged").unwrap();
        git(&root, &["add", "staged.ts"]);
        fs::write(root.join("base.txt"), "working tree change").unwrap();
        fs::write(root.join("new file.ts"), "untracked").unwrap();

        let activity = inspect(root.to_str().unwrap()).unwrap();
        assert!(activity.repository_found);
        assert_eq!(activity.branch.as_deref(), Some("main"));
        assert_eq!(activity.ahead, Some(0));
        assert_eq!(activity.behind, Some(0));
        assert!(activity.staged_files.iter().any(|file| file == "staged.ts"));
        assert!(activity
            .modified_files
            .iter()
            .any(|file| file == "base.txt"));
        assert!(activity
            .untracked_files
            .iter()
            .any(|file| file == "new file.ts"));
        assert_eq!(
            activity
                .latest_commit
                .as_ref()
                .map(|commit| commit.message.as_str()),
            Some("temporary baseline")
        );
        assert!(activity.synchronization.contains("In sync"));

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(remote);
    }
}
