local kkc = require("kkc")

local function trim(text)
    return (text or ""):gsub("^%s+", ""):gsub("%s+$", "")
end

local function git(cwd, args)
    return kkc.exec("git", args, cwd)
end

local function in_git_repo(cwd)
    local result = git(cwd, { "rev-parse", "--is-inside-work-tree" })
    return result.success and trim(result.stdout) == "true"
end

local function status_short(cwd)
    local result = git(cwd, { "status", "--short", "--branch" })
    if not result.success then
        return "Git status unavailable: " .. trim(result.stderr)
    end
    local text = trim(result.stdout)
    if text == "" then
        return "Working tree clean"
    end
    return text
end

local function run_git_action(cwd, args, success_message)
    local result = git(cwd, args)
    local out = trim(result.stdout)
    local err = trim(result.stderr)
    if result.success then
        if out ~= "" then
            return out
        end
        return success_message
    end
    if err ~= "" then
        error(err, 0)
    end
    error("git command failed", 0)
end

kkc.register_action_plugin({
    name = "git_action",
    version = "1.0.0",
    description = "Git repository actions",

    discover = function(cwd)
        if not in_git_repo(cwd) then
            return {}
        end
        return {
            {
                id = "status",
                title = "Git status",
                description = status_short(cwd),
            },
            {
                id = "stage_all",
                title = "Git stage all",
                description = "Stage modified, deleted and untracked files",
            },
            {
                id = "commit",
                title = "Git commit",
                description = "Commit staged changes",
                prompt = "Commit message:",
            },
            {
                id = "push",
                title = "Git push",
                description = "Push the current branch to its upstream",
            },
        }
    end,

    run = function(cwd, action_id, input)
        if action_id == "status" then
            return status_short(cwd)
        elseif action_id == "stage_all" then
            return run_git_action(cwd, { "add", "-A" }, "Staged all changes")
        elseif action_id == "commit" then
            local message = trim(input)
            if message == "" then
                error("Commit message is empty", 0)
            end
            return run_git_action(cwd, { "commit", "-m", message }, "Commit created")
        elseif action_id == "push" then
            return run_git_action(cwd, { "push" }, "Push complete")
        end
        error("Unknown git action: " .. tostring(action_id), 0)
    end,
})
