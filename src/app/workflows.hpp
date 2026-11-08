#pragma once

#include <optional>
#include <string>
#include <vector>

// The git commands behind the Gitflow and branch cleanup workflows, free of the repository and the UI.
namespace imgit::app
{
    namespace gitflow
    {
        enum class Kind
        {
            FEATURE,
            RELEASE,
            HOTFIX,
        };

        // The repository's gitflow.* configuration (what Initialize Gitflow writes).
        struct Config
        {
            std::string master; // gitflow.branch.master
            std::string develop; // gitflow.branch.develop
            std::string feature_prefix = "feature/";
            std::string release_prefix = "release/";
            std::string hotfix_prefix = "hotfix/";
            std::string version_tag; // gitflow.prefix.versiontag

            [[nodiscard]] bool initialized() const noexcept
            {
                return !master.empty() && !develop.empty();
            }
        };

        // "feature", "release", "hotfix".
        [[nodiscard]] const char* kind_name(Kind kind) noexcept;
        // "Feature", "Release", "Hotfix".
        [[nodiscard]] const char* kind_title(Kind kind) noexcept;
        [[nodiscard]] const std::string& prefix(const Config& config, Kind kind) noexcept;

        struct Branch
        {
            Kind kind;
            std::string name; // without the prefix
        };
        // The gitflow branch `branch` is (a non-empty name under one of the prefixes), if any.
        [[nodiscard]] std::optional<Branch> classify(const Config& config, const std::string& branch);
        // Where a branch of that kind starts: develop for features and releases, main for hotfixes.
        [[nodiscard]] const std::string& base_branch(const Config& config, Kind kind) noexcept;
        [[nodiscard]] std::string tag_name(const Config& config, const std::string& name);

        // `git checkout -b <prefix><name> <base>`.
        [[nodiscard]] std::vector<std::vector<std::string>> start_runs(const Config& config, Kind kind,
                                                                       const std::string& name);
        // git flow <kind> finish: a feature merges into develop (--no-ff); a release or hotfix merges into main,
        // is tagged <versiontag><name> (unless `tag_exists`: a finish run again after resolving conflicts), and
        // merges into develop. Then the branch is deleted. Merges already done are no-ops, so after a conflict
        // is resolved and committed the same runs finish the job.
        [[nodiscard]] std::vector<std::vector<std::string>> finish_runs(const Config& config, Kind kind,
                                                                        const std::string& name, bool tag_exists);
    } // namespace gitflow

    namespace cleanup
    {
        struct BranchFacts
        {
            std::string name;
            bool is_head = false; // checked out here or in another worktree
            bool merged = false; // reachable from the default branch
            bool gone = false; // its upstream was deleted
        };

        // The branches worth deleting: merged into the default branch or with their upstream gone; never the
        // checked-out branch or a `protected_branches` one (the default branch, gitflow's main and develop).
        [[nodiscard]] std::vector<BranchFacts> candidates(const std::vector<BranchFacts>& branches,
                                                          const std::vector<std::string>& protected_branches);
        // "merged", "upstream gone", "merged, upstream gone".
        [[nodiscard]] std::string reason(const BranchFacts& branch);
    } // namespace cleanup
} // namespace imgit::app
