local kkc = require("kkc")

local sectors_per_track = {
    21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21,
    19, 19, 19, 19, 19, 19, 19,
    18, 18, 18, 18, 18, 18,
    17, 17, 17, 17, 17,
}

local file_types = {
    [0] = "del",
    [1] = "seq",
    [2] = "prg",
    [3] = "usr",
    [4] = "rel",
}

local function sector_offset(track, sector)
    assert(track >= 1 and track <= #sectors_per_track, "invalid D64 track")
    assert(sector >= 0 and sector < sectors_per_track[track], "invalid D64 sector")

    local sectors = sector
    for idx = 1, track - 1 do
        sectors = sectors + sectors_per_track[idx]
    end
    return sectors * 256 + 1
end

local function read_sector(image, track, sector)
    local offset = sector_offset(track, sector)
    local data = image:sub(offset, offset + 255)
    assert(#data == 256, "truncated D64 image")
    return data
end

local function petscii_filename(raw)
    local out = {}
    for idx = 1, #raw do
        local byte = raw:byte(idx)
        if byte and byte ~= 0xa0 and byte ~= 0x00 then
            local ch = string.char(byte)
            if ch == "/" or ch == "\\" or ch == ":" then
                ch = "_"
            end
            table.insert(out, ch)
        end
    end
    local name = table.concat(out):gsub("%s+$", "")
    if name == "" then
        return "unnamed"
    end
    return name
end

local function unique_name(used, name)
    if not used[name] then
        used[name] = true
        return name
    end

    local stem, ext = name:match("^(.*)(%.[^%.]+)$")
    if not stem then
        stem = name
        ext = ""
    end

    local idx = 2
    while true do
        local candidate = string.format("%s_%d%s", stem, idx, ext)
        if not used[candidate] then
            used[candidate] = true
            return candidate
        end
        idx = idx + 1
    end
end

local function read_file(image, start_track, start_sector)
    local chunks = {}
    local track = start_track
    local sector = start_sector
    local visited = {}

    while track ~= 0 do
        local key = tostring(track) .. "/" .. tostring(sector)
        assert(not visited[key], "cyclic D64 file sector chain")
        visited[key] = true

        local data = read_sector(image, track, sector)
        local next_track = data:byte(1) or 0
        local next_sector = data:byte(2) or 0

        if next_track == 0 then
            local last = math.max(2, math.min(256, next_sector))
            table.insert(chunks, data:sub(3, last))
            break
        end

        table.insert(chunks, data:sub(3))
        track = next_track
        sector = next_sector
    end

    return table.concat(chunks)
end

local function read_directory(image)
    local entries = {}
    local track = 18
    local sector = 1
    local visited = {}

    while track ~= 0 do
        local key = tostring(track) .. "/" .. tostring(sector)
        assert(not visited[key], "cyclic D64 directory sector chain")
        visited[key] = true

        local data = read_sector(image, track, sector)
        for slot = 0, 7 do
            local base = 2 + slot * 32
            local raw_type = data:byte(base + 1) or 0
            local type_id = raw_type & 0x07
            local start_track = data:byte(base + 2) or 0
            local start_sector = data:byte(base + 3) or 0

            if type_id ~= 0 and start_track ~= 0 then
                local raw_name = data:sub(base + 4, base + 19)
                local type_name = file_types[type_id] or "file"
                table.insert(entries, {
                    name = petscii_filename(raw_name) .. "." .. type_name,
                    type = type_name,
                    track = start_track,
                    sector = start_sector,
                })
            end
        end

        track = data:byte(1) or 0
        sector = data:byte(2) or 0
    end

    return entries
end

local function extract_d64(path, destination)
    local file = assert(io.open(path, "rb"))
    local image = file:read("*all")
    file:close()

    assert(#image >= 174848, "D64 image is too small")

    local used = {}
    for _, entry in ipairs(read_directory(image)) do
        local name = unique_name(used, entry.name)
        local content = read_file(image, entry.track, entry.sector)
        kkc.write_file(kkc.path_join(destination, name), content)
    end

    return true
end

kkc.register_archive_plugin({
    name = "commodore_d64",
    description = "Commodore 64 D64 disk image plugin",
    extensions = { "d64" },
    can_handle = function(path)
        return path:lower():match("%.d64$") ~= nil
    end,
    extract = extract_d64,
})
