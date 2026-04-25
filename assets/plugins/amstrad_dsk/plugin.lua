local kkc = require("kkc")
local dsk = require("dsk")

local function trim_right(value)
    return (value:gsub("%s+$", ""))
end

local function amsdos_name(raw_name)
    local base = trim_right(raw_name:sub(1, 8))
    local ext = trim_right(raw_name:sub(9, 11))
    if ext == "" then
        return base
    end
    return base .. "." .. ext
end

local function sector_data(track, side, id)
    return dsk.getsector(track, side, id) or ""
end

local function read_block(block_num)
    local sector_num = block_num * 2
    local track_num = math.floor(sector_num / 9)
    local sector_id = 0xc1 + (sector_num % 9)
    local first = sector_data(track_num, 0, sector_id)

    sector_num = sector_num + 1
    track_num = math.floor(sector_num / 9)
    sector_id = 0xc1 + (sector_num % 9)
    local second = sector_data(track_num, 0, sector_id)

    return first .. second
end

local function strip_amsdos_header(raw_name, data)
    if #data < 128 then
        return data
    end

    if data:sub(2, 12) ~= raw_name then
        return data
    end

    local lo = data:byte(25) or 0
    local hi = data:byte(26) or 0
    local length = lo + hi * 256
    if length <= 0 or length > (#data - 128) then
        return data:sub(129)
    end

    return data:sub(129, 128 + length)
end

local function read_catalog_file(entries)
    table.sort(entries, function(left, right)
        return (left.numextension or 0) < (right.numextension or 0)
    end)

    local data = ""
    for _, entry in ipairs(entries) do
        local part = ""
        for _, block in ipairs(entry.blocks or {}) do
            part = part .. read_block(block)
        end

        local records = entry.nbrecords or 0
        if records > 0 and records < 128 then
            part = part:sub(1, records * 128)
        end
        data = data .. part
    end

    return strip_amsdos_header(entries[1].filename, data)
end

local function extract_dsk(path, destination)
    dsk.init()
    dsk.verbose = false
    assert(dsk.read(path), "unable to read DSK")
    dsk.cat()

    local grouped = {}
    for _, entry in pairs(dsk.catalog or {}) do
        if type(entry) == "table" and entry.filename then
            grouped[entry.filename] = grouped[entry.filename] or {}
            table.insert(grouped[entry.filename], entry)
        end
    end

    for raw_name, entries in pairs(grouped) do
        local name = amsdos_name(raw_name)
        if name ~= "" then
            kkc.write_file(kkc.path_join(destination, name), read_catalog_file(entries))
        end
    end

    return true
end

kkc.register_archive_plugin({
    name = "amstrad_dsk",
    description = "Amstrad CPC DSK archive plugin",
    extensions = { "dsk" },
    can_handle = function(path)
        return path:lower():match("%.dsk$") ~= nil
    end,
    extract = extract_dsk,
})
