use std::path::Path;

#[derive(Debug, Clone, Default)]
pub struct GitContext {
    pub branch: String,
    /// List of (relative_path, status_char) — 'M', 'A', 'D', '?'
    pub status: Vec<(String, char)>,
    /// Truncated `git diff HEAD` (max 4000 chars)
    pub recent_diff: String,
    /// Last 5 commit messages
    pub last_commits: Vec<String>,
}

impl GitContext {
    pub fn is_empty(&self) -> bool {
        self.branch.is_empty() && self.status.is_empty()
    }

    /// Render a compact summary for LLM system prompts.
    pub fn to_prompt_section(&self) -> String {
        if self.is_empty() { return String::new(); }

        let mut s = String::from("## Git context\n");
        s.push_str(&format!("Branch: {}\n", self.branch));

        if !self.status.is_empty() {
            s.push_str("Changed files:\n");
            for (path, status) in &self.status {
                s.push_str(&format!("  {} {}\n", status, path));
            }
        }

        if !self.last_commits.is_empty() {
            s.push_str("Recent commits:\n");
            for c in &self.last_commits {
                s.push_str(&format!("  {}\n", c));
            }
        }

        if !self.recent_diff.is_empty() {
            s.push_str(&format!("```diff\n{}\n```\n", self.recent_diff));
        }

        s
    }
}

/// Build a `GitContext` for the repository at `path`.
/// Returns `GitContext::default()` if not a git repo or git2 fails.
pub fn git_context(path: &Path) -> GitContext {
    let repo = match git2::Repository::discover(path) {
        Ok(r) => r,
        Err(_) => return GitContext::default(),
    };

    let branch = head_branch(&repo);
    let status = file_status(&repo);
    let recent_diff = diff_head(&repo);
    let last_commits = log_messages(&repo, 5);

    GitContext { branch, status, recent_diff, last_commits }
}

fn head_branch(repo: &git2::Repository) -> String {
    repo.head()
        .ok()
        .and_then(|h| h.shorthand().map(|s| s.to_string()))
        .unwrap_or_else(|| "HEAD".to_string())
}

fn file_status(repo: &git2::Repository) -> Vec<(String, char)> {
    let mut opts = git2::StatusOptions::new();
    opts.include_untracked(false).include_ignored(false);

    let statuses = match repo.statuses(Some(&mut opts)) {
        Ok(s) => s,
        Err(_) => return vec![],
    };

    statuses.iter()
        .filter_map(|entry| {
            let path = entry.path()?.to_string();
            let s = entry.status();
            let ch = if s.contains(git2::Status::INDEX_NEW) || s.contains(git2::Status::WT_NEW) {
                'A'
            } else if s.contains(git2::Status::INDEX_DELETED) || s.contains(git2::Status::WT_DELETED) {
                'D'
            } else {
                'M'
            };
            Some((path, ch))
        })
        .take(20)
        .collect()
}

fn diff_head(repo: &git2::Repository) -> String {
    let head = repo.head().ok().and_then(|h| h.peel_to_tree().ok());
    let diff = match head {
        Some(tree) => repo.diff_tree_to_workdir_with_index(Some(&tree), None),
        None => repo.diff_index_to_workdir(None, None),
    };

    let diff = match diff {
        Ok(d) => d,
        Err(_) => return String::new(),
    };

    let mut patch = String::new();
    let _ = diff.print(git2::DiffFormat::Patch, |_, _, line| {
        if patch.len() < 4000 {
            let origin = line.origin();
            if matches!(origin, '+' | '-' | ' ' | '@') {
                patch.push(origin);
                if let Ok(s) = std::str::from_utf8(line.content()) {
                    patch.push_str(s);
                }
            }
        }
        true
    });

    if patch.len() >= 4000 {
        patch.push_str("\n[truncated…]");
    }
    patch
}

fn log_messages(repo: &git2::Repository, n: usize) -> Vec<String> {
    let mut revwalk = match repo.revwalk() {
        Ok(r) => r,
        Err(_) => return vec![],
    };
    let _ = revwalk.push_head();

    revwalk.set_sorting(git2::Sort::TIME).ok();

    revwalk.take(n)
        .filter_map(|oid| oid.ok())
        .filter_map(|oid| repo.find_commit(oid).ok())
        .map(|c| {
            let short = &oid_short(c.id());
            let msg = c.summary().unwrap_or("").chars().take(72).collect::<String>();
            format!("{} {}", short, msg)
        })
        .collect()
}

fn oid_short(oid: git2::Oid) -> String {
    oid.to_string()[..7].to_string()
}
