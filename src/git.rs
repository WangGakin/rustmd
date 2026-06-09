use std::path::Path;

use crate::inline::GitHubContext;

/// Detect GitHub repository context by discovering the git repo
/// and reading the origin remote URL.
pub fn detect_github_context(start_path: &Path) -> Option<GitHubContext> {
    let repo = gix::discover(start_path).ok()?;
    let remote = repo.find_remote("origin").ok()?;
    let url = remote.url(gix::remote::Direction::Fetch)?;
    parse_github_url(url.to_bstring().to_string().as_str())
}

/// Parse a GitHub URL (SSH or HTTPS) into GitHubContext.
fn parse_github_url(url: &str) -> Option<GitHubContext> {
    // SSH format: git@github.com:owner/repo.git
    if let Some(rest) = url.strip_prefix("git@github.com:") {
        let repo_path = rest.strip_suffix(".git").unwrap_or(rest);
        let (owner, repo) = repo_path.split_once('/')?;
        return Some(GitHubContext {
            owner: owner.to_string(),
            repo: repo.to_string(),
        });
    }

    // HTTPS format: https://github.com/owner/repo.git
    if url.contains("github.com") {
        let url = url
            .strip_prefix("https://")
            .or_else(|| url.strip_prefix("http://"))?;
        let url = url.strip_prefix("github.com/")?;
        let repo_path = url.strip_suffix(".git").unwrap_or(url);
        let (owner, repo) = repo_path.split_once('/')?;
        // Handle URLs with trailing path components (e.g., .../owner/repo/pulls)
        let repo = repo.split('/').next()?;
        return Some(GitHubContext {
            owner: owner.to_string(),
            repo: repo.to_string(),
        });
    }

    None
}

/// Parse a "owner/repo" string into GitHubContext.
pub fn parse_github_repo_string(s: &str) -> Option<GitHubContext> {
    let (owner, repo) = s.split_once('/')?;
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    Some(GitHubContext {
        owner: owner.to_string(),
        repo: repo.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_github_ssh_url() {
        let ctx = parse_github_url("git@github.com:wilfred/writ.git").unwrap();
        assert_eq!(ctx.owner, "wilfred");
        assert_eq!(ctx.repo, "writ");

        // Without .git suffix
        let ctx = parse_github_url("git@github.com:wilfred/writ").unwrap();
        assert_eq!(ctx.owner, "wilfred");
        assert_eq!(ctx.repo, "writ");
    }

    #[test]
    fn test_parse_github_https_url() {
        let ctx = parse_github_url("https://github.com/wilfred/writ.git").unwrap();
        assert_eq!(ctx.owner, "wilfred");
        assert_eq!(ctx.repo, "writ");

        // Without .git suffix
        let ctx = parse_github_url("https://github.com/wilfred/writ").unwrap();
        assert_eq!(ctx.owner, "wilfred");
        assert_eq!(ctx.repo, "writ");
    }

    #[test]
    fn test_parse_non_github_url() {
        assert!(parse_github_url("git@gitlab.com:owner/repo.git").is_none());
        assert!(parse_github_url("https://gitlab.com/owner/repo").is_none());
    }

    #[test]
    fn test_parse_github_repo_string() {
        let ctx = parse_github_repo_string("wilfred/writ").unwrap();
        assert_eq!(ctx.owner, "wilfred");
        assert_eq!(ctx.repo, "writ");

        assert!(parse_github_repo_string("invalid").is_none());
        assert!(parse_github_repo_string("/repo").is_none());
        assert!(parse_github_repo_string("owner/").is_none());
    }

    #[test]
    fn test_detect_github_context_in_repo() {
        // This test runs from within the writ repo itself
        let ctx = detect_github_context(std::path::Path::new(".")).unwrap();
        assert_eq!(ctx.owner, "wilfreddenton");
        assert_eq!(ctx.repo, "writ");
    }
}
