local kkc = require("kkc")

local MAX_COMMITS = 120

local function trim(text)
    return (text or ""):gsub("^%s+", ""):gsub("%s+$", "")
end

local function run_git(cwd, args)
    return kkc.exec("git", args, cwd)
end

local function run_tar(args)
    return kkc.exec("tar", args, nil)
end

local function sanitize_name(text, max_len)
    local value = (text or ""):gsub("[%c%z]", " "):gsub("%s+", " ")
    value = trim(value)
    value = value:gsub("[^%w%._%- ]", "_")
    value = value:gsub(" ", "_")
    if value == "" then
        value = "no_message"
    end
    if max_len and #value > max_len then
        value = value:sub(1, max_len)
    end
    return value
end

local function dirname(path)
    if not path then
        return nil
    end
    local normalized = path:gsub("\\", "/")
    local parent = normalized:match("^(.*)/[^/]+/?$")
    return parent
end

local function is_dot_git_dir(path)
    local normalized = (path or ""):gsub("\\", "/")
    normalized = normalized:gsub("/+$", "")
    return normalized:lower():match("/%.git$") ~= nil or normalized:lower() == ".git"
end

local function format_commit_date(epoch_text)
    local ts = tonumber(epoch_text)
    if not ts then
        return trim(epoch_text)
    end
    local local_date = os.date("%Y-%m-%d %H:%M", ts)
    if local_date then
        return local_date
    end
    return tostring(ts)
end

local function touch_timestamp(ts)
    if not ts then
        return nil
    end
    return os.date("%Y%m%d%H%M.%S", ts)
end

local function set_path_mtime(path, ts)
    local stamp = touch_timestamp(ts)
    if not stamp then
        return
    end
    local result = kkc.exec("touch", { "-t", stamp, path }, nil)
    if not result.success then
        kkc.debug_log("git_commits: touch failed for " .. path .. ": " .. trim(result.stderr))
    end
end

local function delete_file(path)
    local result = kkc.exec("rm", { "-f", path }, nil)
    if not result.success then
        kkc.debug_log("git_commits: rm failed for " .. path .. ": " .. trim(result.stderr))
    end
end

local function list_commits(repo_root)
    local fmt = "%H%x1f%h%x1f%ct%x1f%s"
    local args = {
        "log",
        "--format=" .. fmt,
        "-n",
        tostring(MAX_COMMITS),
    }
    local result = run_git(repo_root, args)
    if not result.success then
        error("git log failed: " .. trim(result.stderr), 0)
    end

    local commits = {}
    for line in (result.stdout or ""):gmatch("[^\r\n]+") do
        local full, short, commit_ts, subject = line:match("^([^\31]+)\31([^\31]+)\31([^\31]+)\31(.*)$")
        local commit_epoch = tonumber(commit_ts)
        if full and short and commit_ts then
            table.insert(commits, {
                full = full,
                short = short,
                epoch = commit_epoch,
                date = format_commit_date(commit_ts),
                subject = subject or "",
            })
        end
    end
    return commits
end

local function write_index(source_path, destination, commits)
    local lines = {
        "Git commits virtual archive",
        "",
        "Source: " .. source_path,
        "Entries: " .. tostring(#commits),
        "Limit: " .. tostring(MAX_COMMITS),
        "",
        "Each commit is extracted in its own directory:",
        "<order>_<short_sha>_<subject>/",
        "",
    }
    for idx, commit in ipairs(commits) do
        lines[#lines + 1] = string.format(
            "%03d  %s  %s  %s",
            idx,
            commit.short,
            commit.date,
            commit.subject
        )
    end
    kkc.write_file(kkc.path_join(destination, "README.txt"), table.concat(lines, "\n") .. "\n")
end

local function extract_commit_tree(repo_root, destination, index, commit)
    local dirname_commit = string.format(
        "%03d_%s_%s",
        index,
        sanitize_name(commit.short, 12),
        sanitize_name(commit.subject, 48)
    )
    local commit_dir = kkc.path_join(destination, dirname_commit)
    kkc.create_dir_all(commit_dir)

    local archive_path = kkc.path_join(destination, ".git_commit_" .. index .. ".tar")
    local archive_result = run_git(repo_root, {
        "archive",
        "--format=tar",
        "-o",
        archive_path,
        commit.full,
    })
    if not archive_result.success then
        error("git archive failed for " .. commit.short .. ": " .. trim(archive_result.stderr), 0)
    end

    local tar_result = run_tar({ "-xf", archive_path, "-C", commit_dir })
    if not tar_result.success then
        error("tar extraction failed for " .. commit.short .. ": " .. trim(tar_result.stderr), 0)
    end

    local meta = table.concat({
        "commit=" .. commit.full,
        "short=" .. commit.short,
        "date=" .. commit.date,
        "subject=" .. commit.subject,
    }, "\n") .. "\n"
    kkc.write_file(kkc.path_join(commit_dir, ".commit_meta.txt"), meta)
    set_path_mtime(kkc.path_join(commit_dir, ".commit_meta.txt"), commit.epoch)
    delete_file(archive_path)
    set_path_mtime(commit_dir, commit.epoch)
end

local function extract_git_commits(path, destination)
    if not kkc.is_dir(path) then
        error("git_commits expects a directory path", 0)
    end
    if not is_dot_git_dir(path) then
        error("git_commits can only open a .git directory", 0)
    end

    local repo_root = dirname(path)
    if not repo_root or repo_root == "" then
        error("cannot locate repository root for " .. path, 0)
    end

    local probe = run_git(repo_root, { "rev-parse", "--is-inside-work-tree" })
    if not probe.success or trim(probe.stdout) ~= "true" then
        error("directory is not attached to a valid git work tree", 0)
    end

    local commits = list_commits(repo_root)
    write_index(path, destination, commits)

    for idx, commit in ipairs(commits) do
        extract_commit_tree(repo_root, destination, idx, commit)
    end

    return true
end

kkc.register_archive_plugin({
    extensions = { "git" },
    can_handle = function(path)
        return is_dot_git_dir(path)
    end,
    extract = extract_git_commits,
})
