#include "app/workflows.hpp"
#include <algorithm>

namespace imgit::app
{
    namespace gitflow
    {
        const char* kind_name(Kind kind) noexcept
        {
            switch (kind)
            {
            case Kind::FEATURE:
                return "feature";
            case Kind::RELEASE:
                return "release";
            case Kind::HOTFIX:
                return "hotfix";
            }
            return "";
        }

        const char* kind_title(Kind kind) noexcept
        {
            switch (kind)
            {
            case Kind::FEATURE:
                return "Feature";
            case Kind::RELEASE:
                return "Release";
            case Kind::HOTFIX:
                return "Hotfix";
            }
            return "";
        }

        const std::string& prefix(const Config& config, Kind kind) noexcept
        {
            switch (kind)
            {
            case Kind::RELEASE:
                return config.release_prefix;
            case Kind::HOTFIX:
                return config.hotfix_prefix;
            case Kind::FEATURE:
                break;
            }
            return config.feature_prefix;
        }

        std::optional<Branch> classify(const Config& config, const std::string& branch)
        {
            if (!config.initialized())
            {
                return std::nullopt;
            }
            for (const Kind kind : {Kind::FEATURE, Kind::RELEASE, Kind::HOTFIX})
            {
                const std::string& start = prefix(config, kind);
                if (!start.empty() && branch.size() > start.size() && branch.starts_with(start))
                {
                    return Branch{kind, branch.substr(start.size())};
                }
            }
            return std::nullopt;
        }

        const std::string& base_branch(const Config& config, Kind kind) noexcept
        {
            return kind == Kind::HOTFIX ? config.master : config.develop;
        }

        std::string tag_name(const Config& config, const std::string& name)
        {
            return config.version_tag + name;
        }

        std::vector<std::vector<std::string>> start_runs(const Config& config, Kind kind, const std::string& name)
        {
            return {{"checkout", "-b", prefix(config, kind) + name, base_branch(config, kind)}};
        }

        std::vector<std::vector<std::string>> finish_runs(const Config& config, Kind kind, const std::string& name,
                                                          bool tag_exists)
        {
            const std::string branch = prefix(config, kind) + name;
            std::vector<std::vector<std::string>> runs;
            const auto merge_into = [&](const std::string& target)
            {
                runs.push_back({"checkout", target});
                runs.push_back({"merge", "--no-ff", "--no-edit", branch});
            };
            if (kind == Kind::FEATURE)
            {
                merge_into(config.develop);
            }
            else
            {
                merge_into(config.master);
                if (!tag_exists)
                {
                    const std::string tag = tag_name(config, name);
                    runs.push_back({"tag", "-a", tag, "-m", tag});
                }
                merge_into(config.develop);
            }
            runs.push_back({"branch", "-d", branch});
            return runs;
        }
    } // namespace gitflow

    namespace cleanup
    {
        std::vector<BranchFacts> candidates(const std::vector<BranchFacts>& branches,
                                            const std::vector<std::string>& protected_branches)
        {
            std::vector<BranchFacts> result;
            for (const BranchFacts& branch : branches)
            {
                const bool is_protected = std::find(protected_branches.begin(), protected_branches.end(), branch.name)
                                          != protected_branches.end();
                if (branch.is_head || is_protected || (!branch.merged && !branch.gone))
                {
                    continue;
                }
                result.push_back(branch);
            }
            std::sort(result.begin(), result.end(),
                      [](const BranchFacts& a, const BranchFacts& b)
                      {
                          return a.name < b.name;
                      });
            return result;
        }

        std::string reason(const BranchFacts& branch)
        {
            if (branch.merged && branch.gone)
            {
                return "merged, upstream gone";
            }
            return branch.merged ? "merged" : "upstream gone";
        }
    } // namespace cleanup
} // namespace imgit::app
