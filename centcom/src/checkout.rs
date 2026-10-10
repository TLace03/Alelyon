//! The repository checkout Alelyon runs from: the fleet's sources and Sinai's voice tools are read from the main
//! checkout, whichever worktree this build was made in.

use std::path::{Component, Path, PathBuf};

/// The main checkout of the repository `start` lies in. A worktree's `.git` is a file naming
/// `<main>/.git/worktrees/<name>`, whose `commondir` leads back to `<main>/.git`.
pub fn main_checkout(start: &Path) -> Option<PathBuf> {
    for dir in start.ancestors() {
        let git = dir.join(".git");
        if git.is_dir() {
            return Some(dir.to_path_buf());
        }
        if git.is_file() {
            let pointer = std::fs::read_to_string(&git).ok()?;
            let gitdir = PathBuf::from(pointer.trim().strip_prefix("gitdir:")?.trim());
            let gitdir = if gitdir.is_absolute() { gitdir } else { dir.join(gitdir) };
            let common = PathBuf::from(std::fs::read_to_string(gitdir.join("commondir")).ok()?.trim());
            let common = lexical(&if common.is_absolute() { common } else { gitdir.join(common) });
            return common.parent().map(Path::to_path_buf);
        }
    }
    None
}

/// `path` with `.` and `..` resolved by name: no file system access, and no `\\?\` prefix.
fn lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// The files and folders of the repository's own stores that Alelyon's pages read (the Data page, and the
/// Fleet page of the builds that carry it).
#[derive(Clone, Debug, PartialEq)]
pub struct Places {
    pub repo: PathBuf,
    pub bus: PathBuf,
    pub relay: PathBuf,
    pub slots: PathBuf,
    /// `~/.alelyon`, when `USERPROFILE` is set.
    pub alelyon: Option<PathBuf>,
}

impl Places {
    pub fn under(repo: PathBuf) -> Places {
        let git = repo.join(".git");
        Places {
            bus: repo.join("globals").join("worktree_cache.db"),
            relay: git.join("alelyon-pr-relay.db"),
            slots: git.join("alelyon-ci-slots"),
            alelyon: std::env::var_os("USERPROFILE").map(|home| PathBuf::from(home).join(".alelyon")),
            repo,
        }
    }

    /// The main checkout Alelyon runs from, or `CENTCOM_FLEET_REPO`.
    pub fn find() -> Result<Places, String> {
        if let Some(repo) = std::env::var_os("CENTCOM_FLEET_REPO") {
            return Ok(Places::under(PathBuf::from(repo)));
        }
        let exe = std::env::current_exe().map_err(|e| format!("where Alelyon runs from is unknown: {e}"))?;
        let repo = main_checkout(&exe)
            .ok_or_else(|| "Alelyon is not running from a checkout of the repository, so there is no fleet to show.".to_string())?;
        Ok(Places::under(repo))
    }

    /// A path under `~/.alelyon`, when `USERPROFILE` is set.
    pub fn in_alelyon(&self, parts: &[&str]) -> Option<PathBuf> {
        self.alelyon.as_ref().map(|base| parts.iter().fold(base.clone(), |p, part| p.join(part)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_worktree_finds_the_main_checkout() {
        let root = std::env::temp_dir().join(format!("centcom-checkout-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let main = root.join("main-repo");
        let gitdir = main.join(".git").join("worktrees").join("wt");
        std::fs::create_dir_all(&gitdir).unwrap();
        std::fs::write(gitdir.join("commondir"), "../..\n").unwrap();
        let wt = root.join("wt");
        let exe = wt.join("alelyon").join("frontend").join("centcom").join("target").join("release");
        std::fs::create_dir_all(&exe).unwrap();
        // git writes the pointer with forward slashes
        std::fs::write(wt.join(".git"), format!("gitdir: {}\n", gitdir.display().to_string().replace('\\', "/"))).unwrap();
        assert_eq!(main_checkout(&exe.join("centcom.exe")), Some(main.clone()));
        assert_eq!(main_checkout(&main.join("alelyon")), Some(main.clone()), "the main checkout is its own");
        let _ = std::fs::remove_dir_all(&root);
    }
}
