pub(crate) const DOCS_URL: &str = "https://openipc-rs.neels.dev";
pub(crate) const WEB_APP_URL: &str = "https://nebulus.openipc-rs.neels.dev";
pub(crate) const REPOSITORY_URL: &str = "https://github.com/neelsani/openipc-rs";
pub(crate) const RELEASES_URL: &str = "https://github.com/neelsani/openipc-rs/releases";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BuildInfo {
    pub(crate) version: &'static str,
    pub(crate) commit: Option<&'static str>,
    pub(crate) tag: Option<&'static str>,
}

impl BuildInfo {
    pub(crate) fn release_label(self) -> String {
        self.tag
            .map(str::to_owned)
            .unwrap_or_else(|| format!("v{}", self.version))
    }

    pub(crate) fn short_commit(self) -> Option<&'static str> {
        self.commit
            .map(|commit| commit.get(..commit.len().min(8)).unwrap_or(commit))
    }

    pub(crate) fn commit_url(self) -> String {
        self.commit.map_or_else(
            || REPOSITORY_URL.to_owned(),
            |commit| format!("{REPOSITORY_URL}/commit/{commit}"),
        )
    }

    pub(crate) fn description(self) -> String {
        let mut parts = vec![format!("Nebulus v{}", self.version)];
        if let Some(commit) = self.commit {
            parts.push(format!("commit {commit}"));
        }
        if let Some(tag) = self
            .tag
            .filter(|tag| tag.strip_prefix('v') != Some(self.version))
        {
            parts.push(format!("tag {tag}"));
        }
        parts.join(" | ")
    }
}

pub(crate) fn current() -> BuildInfo {
    BuildInfo {
        version: env!("CARGO_PKG_VERSION"),
        commit: non_empty(option_env!("NEBULUS_GIT_COMMIT")),
        tag: non_empty(option_env!("NEBULUS_GIT_TAG")),
    }
}

fn non_empty(value: Option<&'static str>) -> Option<&'static str> {
    value.filter(|value| !value.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::BuildInfo;

    #[test]
    fn build_info_always_contains_the_package_version() {
        assert_eq!(super::current().version, env!("CARGO_PKG_VERSION"));
        assert!(!super::current().description().is_empty());
    }

    #[test]
    fn matching_release_tag_replaces_the_duplicate_version_label() {
        let build = BuildInfo {
            version: "1.2.3",
            commit: Some("0123456789abcdef"),
            tag: Some("v1.2.3"),
        };

        assert_eq!(build.release_label(), "v1.2.3");
        assert_eq!(
            build.description(),
            "Nebulus v1.2.3 | commit 0123456789abcdef"
        );
    }

    #[test]
    fn untagged_build_uses_the_package_version() {
        let build = BuildInfo {
            version: "1.2.3",
            commit: None,
            tag: None,
        };

        assert_eq!(build.release_label(), "v1.2.3");
    }
}
