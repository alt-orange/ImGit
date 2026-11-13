//! The git commands behind the Gitflow and branch cleanup workflows, free of the repository and the UI.

/// One `git` invocation's arguments.
pub type Run = Vec<String>;

fn run<const N: usize>(args: [&str; N]) -> Run {
    args.map(str::to_owned).into()
}

pub mod gitflow {
    use super::{Run, run};
    use std::fmt;
    use std::str::FromStr;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub enum Kind {
        Feature,
        Release,
        Hotfix,
    }

    impl Kind {
        /// In the order a branch is matched against the prefixes.
        pub const ALL: [Self; 3] = [Self::Feature, Self::Release, Self::Hotfix];

        /// "feature", "release", "hotfix".
        #[must_use]
        pub const fn name(self) -> &'static str {
            match self {
                Self::Feature => "feature",
                Self::Release => "release",
                Self::Hotfix => "hotfix",
            }
        }

        /// "Feature", "Release", "Hotfix".
        #[must_use]
        pub const fn title(self) -> &'static str {
            match self {
                Self::Feature => "Feature",
                Self::Release => "Release",
                Self::Hotfix => "Hotfix",
            }
        }
    }

    impl fmt::Display for Kind {
        /// The [`name`](Self::name).
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(self.name())
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
    #[error("\"{0}\" is not a gitflow branch kind (feature, release or hotfix)")]
    pub struct ParseKindError(String);

    impl FromStr for Kind {
        type Err = ParseKindError;

        /// From its [`name`](Self::name).
        fn from_str(name: &str) -> Result<Self, Self::Err> {
            Self::ALL
                .into_iter()
                .find(|kind| kind.name() == name)
                .ok_or_else(|| ParseKindError(name.to_owned()))
        }
    }

    /// The repository's `gitflow.*` configuration (what Initialize Gitflow writes).
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Config {
        /// `gitflow.branch.master`
        pub master: String,
        /// `gitflow.branch.develop`
        pub develop: String,
        pub feature_prefix: String,
        pub release_prefix: String,
        pub hotfix_prefix: String,
        /// `gitflow.prefix.versiontag`
        pub version_tag: String,
    }

    impl Default for Config {
        /// Not initialized, with git-flow's own prefixes.
        fn default() -> Self {
            Self {
                master: String::new(),
                develop: String::new(),
                feature_prefix: "feature/".to_owned(),
                release_prefix: "release/".to_owned(),
                hotfix_prefix: "hotfix/".to_owned(),
                version_tag: String::new(),
            }
        }
    }

    /// A gitflow branch: its kind and its name without the prefix.
    #[derive(Debug, Clone, PartialEq, Eq, Hash)]
    pub struct Branch {
        pub kind: Kind,
        pub name: String,
    }

    impl Config {
        /// Both of gitflow's long-lived branches are configured.
        #[must_use]
        pub fn initialized(&self) -> bool {
            !self.master.is_empty() && !self.develop.is_empty()
        }

        #[must_use]
        pub fn prefix(&self, kind: Kind) -> &str {
            match kind {
                Kind::Feature => &self.feature_prefix,
                Kind::Release => &self.release_prefix,
                Kind::Hotfix => &self.hotfix_prefix,
            }
        }

        /// The gitflow branch `branch` is (a non-empty name under one of the prefixes), if any.
        #[must_use]
        pub fn classify(&self, branch: &str) -> Option<Branch> {
            if !self.initialized() {
                return None;
            }
            Kind::ALL.into_iter().find_map(|kind| {
                let prefix = self.prefix(kind);
                let name = branch.strip_prefix(prefix)?;
                (!prefix.is_empty() && !name.is_empty()).then(|| Branch {
                    kind,
                    name: name.to_owned(),
                })
            })
        }

        /// Where a branch of that kind starts: develop for features and releases, main for hotfixes.
        #[must_use]
        pub fn base_branch(&self, kind: Kind) -> &str {
            match kind {
                Kind::Hotfix => &self.master,
                Kind::Feature | Kind::Release => &self.develop,
            }
        }

        /// The tag a finished release or hotfix `name` gets.
        #[must_use]
        pub fn tag_name(&self, name: &str) -> String {
            format!("{}{name}", self.version_tag)
        }

        /// The full branch name of the `kind` branch `name`.
        #[must_use]
        pub fn branch_name(&self, kind: Kind, name: &str) -> String {
            format!("{}{name}", self.prefix(kind))
        }

        /// `git checkout -b <prefix><name> <base>`.
        #[must_use]
        pub fn start_runs(&self, kind: Kind, name: &str) -> Vec<Run> {
            let branch = self.branch_name(kind, name);
            vec![run(["checkout", "-b", &branch, self.base_branch(kind)])]
        }

        /// `git flow <kind> finish`: a feature merges into develop (`--no-ff`); a release or hotfix merges into
        /// main, is tagged `<versiontag><name>` (unless `tag_exists`: a finish run again after resolving
        /// conflicts), and merges into develop. Then the branch is deleted. Merges already done are no-ops, so
        /// after a conflict is resolved and committed the same runs finish the job.
        #[must_use]
        pub fn finish_runs(&self, kind: Kind, name: &str, tag_exists: bool) -> Vec<Run> {
            let branch = self.branch_name(kind, name);
            let merge_into = |target: &str| {
                [
                    run(["checkout", target]),
                    run(["merge", "--no-ff", "--no-edit", &branch]),
                ]
            };
            let mut runs = Vec::new();
            if kind == Kind::Feature {
                runs.extend(merge_into(&self.develop));
            } else {
                runs.extend(merge_into(&self.master));
                if !tag_exists {
                    let tag = self.tag_name(name);
                    runs.push(run(["tag", "-a", &tag, "-m", &tag]));
                }
                runs.extend(merge_into(&self.develop));
            }
            runs.push(run(["branch", "-d", &branch]));
            runs
        }
    }
}

pub mod cleanup {
    /// What the cleanup needs to know about a local branch.
    #[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
    pub struct BranchFacts {
        pub name: String,
        /// Checked out here or in another worktree.
        pub is_head: bool,
        /// Reachable from the default branch.
        pub merged: bool,
        /// Its upstream was deleted.
        pub gone: bool,
    }

    impl BranchFacts {
        /// Why the branch is a candidate: "merged", "upstream gone", "merged, upstream gone".
        #[must_use]
        pub fn reason(&self) -> &'static str {
            match (self.merged, self.gone) {
                (true, true) => "merged, upstream gone",
                (true, false) => "merged",
                (false, _) => "upstream gone",
            }
        }
    }

    /// The branches worth deleting, by name: merged into the default branch or with their upstream gone; never
    /// the checked-out branch or a `protected_branches` one (the default branch, gitflow's main and develop).
    #[must_use]
    pub fn candidates<S: AsRef<str>>(
        branches: &[BranchFacts],
        protected_branches: &[S],
    ) -> Vec<BranchFacts> {
        let is_protected = |name: &str| {
            protected_branches
                .iter()
                .any(|protected| protected.as_ref() == name)
        };
        let mut result: Vec<BranchFacts> = branches
            .iter()
            .filter(|branch| {
                !branch.is_head && (branch.merged || branch.gone) && !is_protected(&branch.name)
            })
            .cloned()
            .collect();
        result.sort_by(|a, b| a.name.cmp(&b.name));
        result
    }
}

#[cfg(test)]
mod tests {
    use super::cleanup::{self, BranchFacts};
    use super::gitflow::{self, Kind};

    fn config() -> gitflow::Config {
        gitflow::Config {
            master: "main".to_owned(),
            develop: "develop".to_owned(),
            version_tag: "v".to_owned(),
            ..gitflow::Config::default()
        }
    }

    fn runs<const N: usize>(runs: [&[&str]; N]) -> Vec<Vec<String>> {
        runs.iter()
            .map(|run| run.iter().map(|&arg| arg.to_owned()).collect())
            .collect()
    }

    #[test]
    fn gitflow_classify() {
        let feature = config().classify("feature/login").unwrap();
        assert_eq!(feature.kind, Kind::Feature);
        assert_eq!(feature.name, "login");
        let hotfix = config().classify("hotfix/1.0.1").unwrap();
        assert_eq!(hotfix.kind, Kind::Hotfix);
        assert_eq!(config().classify("feature/"), None);
        assert_eq!(config().classify("main"), None);
        assert_eq!(gitflow::Config::default().classify("feature/login"), None); // not initialized
        // An empty prefix matches nothing.
        let unprefixed = gitflow::Config {
            feature_prefix: String::new(),
            ..config()
        };
        assert_eq!(unprefixed.classify("login"), None);
        assert_eq!(
            unprefixed.classify("release/1.0").unwrap().kind,
            Kind::Release
        );
    }

    #[test]
    fn gitflow_kinds() {
        assert_eq!(Kind::Release.name(), "release");
        assert_eq!(Kind::Release.title(), "Release");
        assert_eq!(Kind::Hotfix.to_string(), "hotfix");
        for kind in Kind::ALL {
            assert_eq!(kind.name().parse(), Ok(kind));
            assert_eq!(kind.title().to_lowercase(), kind.name());
        }
        assert!("Feature".parse::<Kind>().is_err());
        assert_eq!(config().prefix(Kind::Hotfix), "hotfix/");
        assert_eq!(config().base_branch(Kind::Release), "develop");
        assert_eq!(config().tag_name("1.0"), "v1.0");
    }

    #[test]
    fn gitflow_start() {
        assert_eq!(
            config().start_runs(Kind::Feature, "x"),
            runs([&["checkout", "-b", "feature/x", "develop"]])
        );
        assert_eq!(
            config().start_runs(Kind::Release, "1.0"),
            runs([&["checkout", "-b", "release/1.0", "develop"]])
        );
        assert_eq!(
            config().start_runs(Kind::Hotfix, "1.0.1"),
            runs([&["checkout", "-b", "hotfix/1.0.1", "main"]])
        );
    }

    #[test]
    fn gitflow_finish_feature() {
        assert_eq!(
            config().finish_runs(Kind::Feature, "x", false),
            runs([
                &["checkout", "develop"],
                &["merge", "--no-ff", "--no-edit", "feature/x"],
                &["branch", "-d", "feature/x"],
            ])
        );
    }

    #[test]
    fn gitflow_finish_release() {
        assert_eq!(
            config().finish_runs(Kind::Release, "1.0", false),
            runs([
                &["checkout", "main"],
                &["merge", "--no-ff", "--no-edit", "release/1.0"],
                &["tag", "-a", "v1.0", "-m", "v1.0"],
                &["checkout", "develop"],
                &["merge", "--no-ff", "--no-edit", "release/1.0"],
                &["branch", "-d", "release/1.0"],
            ])
        );
        // Finished again after a conflict: the tag is there already.
        assert_eq!(
            config().finish_runs(Kind::Hotfix, "1.0.1", true),
            runs([
                &["checkout", "main"],
                &["merge", "--no-ff", "--no-edit", "hotfix/1.0.1"],
                &["checkout", "develop"],
                &["merge", "--no-ff", "--no-edit", "hotfix/1.0.1"],
                &["branch", "-d", "hotfix/1.0.1"],
            ])
        );
    }

    #[test]
    fn branch_cleanup_candidates() {
        let facts = |name: &str, is_head, merged, gone| BranchFacts {
            name: name.to_owned(),
            is_head,
            merged,
            gone,
        };
        let branches = [
            facts("main", false, true, false),
            facts("develop", false, true, false),
            facts("current", true, true, true),
            facts("zeta", false, true, false),
            facts("alpha", false, false, true),
            facts("wip", false, false, false),
            facts("both", false, true, true),
        ];
        let found = cleanup::candidates(&branches, &["main", "develop"]);
        assert_eq!(found.len(), 3);
        assert_eq!(found[0].name, "alpha");
        assert_eq!(found[0].reason(), "upstream gone");
        assert_eq!(found[1].name, "both");
        assert_eq!(found[1].reason(), "merged, upstream gone");
        assert_eq!(found[2].name, "zeta");
        assert_eq!(found[2].reason(), "merged");
    }
}
