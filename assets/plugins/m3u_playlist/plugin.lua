local kkc = require("kkc")

local function dirname(path)
    local normalized = path:gsub("\\", "/")
    local dir = normalized:match("^(.*)/[^/]*$")
    if dir == nil or dir == "" then
        return "."
    end
    return dir
end

local function basename(path)
    local normalized = path:gsub("\\", "/"):gsub("[/]+$", "")
    local name = normalized:match("([^/]+)$")
    if name == nil or name == "" then
        return "entry"
    end
    return name
end

local function is_absolute(path)
    return path:match("^/") ~= nil or path:match("^%a:[/\\]") ~= nil
end

local function has_scheme(path)
    return path:match("^[%w][%w+.-]*://") ~= nil
end

local function sanitize_name(name)
    local out = name:gsub("[%z\r\n\t/\\:]", "_")
    if out == "" then
        return "entry"
    end
    return out
end

local function entry_name(index, target)
    return string.format("%03d - %s", index, sanitize_name(basename(target)))
end

local function local_target(playlist_dir, target)
    if has_scheme(target) then
        local file_path = target:match("^file://(.+)$")
        return file_path
    end
    if is_absolute(target) then
        return target
    end
    return kkc.path_join(playlist_dir, target)
end

local function write_pointer(path, target)
    kkc.write_file(path, target .. "\n")
end

local function link_or_pointer(source, output, target)
    if source ~= nil and kkc.path_exists(source) then
        local linked = kkc.exec("ln", { "-s", source, output }, nil)
        if linked.success then
            return
        end
    end
    write_pointer(output, target)
end

local function extract_playlist(path, destination)
    local handle = assert(io.open(path, "r"))
    local playlist_dir = dirname(path)
    local count = 0

    for line in handle:lines() do
        local target = line:gsub("^\239\187\191", ""):match("^%s*(.-)%s*$")
        if target ~= "" and target:sub(1, 1) ~= "#" then
            count = count + 1
            local output = kkc.path_join(destination, entry_name(count, target))
            link_or_pointer(local_target(playlist_dir, target), output, target)
        end
    end
    handle:close()

    if count == 0 then
        error("playlist has no entries")
    end
    return true
end

kkc.register_archive_plugin({
    extensions = { "m3u", "m3u8" },
    can_handle = function(path)
        local lower = path:lower()
        return lower:match("%.m3u$") ~= nil or lower:match("%.m3u8$") ~= nil
    end,
    extract = extract_playlist,
})
