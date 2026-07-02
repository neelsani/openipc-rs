const REPOSITORY_URL: &str = "https://github.com/neelsani/openipc-rs";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BuildInfo {
    pub(crate) version: &'static str,
    pub(crate) commit: Option<&'static str>,
    pub(crate) tag: Option<&'static str>,
}

impl BuildInfo {
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
        if let Some(tag) = self.tag {
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
    #[test]
    fn build_info_always_contains_the_package_version() {
        assert_eq!(super::current().version, env!("CARGO_PKG_VERSION"));
        assert!(!super::current().description().is_empty());
    }
}
