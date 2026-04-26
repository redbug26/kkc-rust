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

local function basename(path)
    return path:match("([^/\\]+)$") or path
end

local function span(text, fg, bold)
    return { text = text, fg = fg or "white", bg = "black", bold = bold or false }
end

local function line(...)
    return { ... }
end

local function pad_right(value, width)
    value = tostring(value or "")
    if #value >= width then
        return value:sub(1, width)
    end
    return value .. string.rep(" ", width - #value)
end

local function pad_left(value, width)
    value = tostring(value or "")
    if #value >= width then
        return value:sub(1, width)
    end
    return string.rep(" ", width - #value) .. value
end

local function block_list(entry)
    local out = {}
    local range_start = nil
    local previous = nil
    local function flush_range()
        if range_start == nil then
            return
        end
        if range_start == previous then
            table.insert(out, tostring(range_start))
        else
            table.insert(out, tostring(range_start) .. "-" .. tostring(previous))
        end
        range_start = nil
        previous = nil
    end

    for _, block in ipairs(entry.blocks or {}) do
        if range_start == nil then
            range_start = block
            previous = block
        elseif block == previous + 1 then
            previous = block
        else
            flush_range()
            range_start = block
            previous = block
        end
    end
    flush_range()
    return table.concat(out, " ")
end

local function catalog_rows()
    local rows = {}
    for _, entry in pairs(dsk.catalog or {}) do
        if type(entry) == "table" and entry.filename then
            table.insert(rows, entry)
        end
    end
    table.sort(rows, function(left, right)
        if left.filename == right.filename then
            return (left.numextension or 0) < (right.numextension or 0)
        end
        return left.filename < right.filename
    end)
    return rows
end

local function free_block_count()
    local free = 0
    local total = 0
    for _, available in pairs(dsk.freeblocks or {}) do
        total = total + 1
        if available then
            free = free + 1
        end
    end
    return free, total
end

local function render_dsk_directory(path, mode)
    if mode ~= "text" or not path:lower():match("%.dsk$") then
        return nil
    end

    dsk.init()
    dsk.verbose = false
    assert(dsk.read(path), "unable to read DSK")
    dsk.cat()

    local rows = catalog_rows()
    local free, total = free_block_count()
    local lines = {}
    table.insert(lines, line(span("Amstrad CPC DSK directory", "yellow", true)))
    table.insert(lines, line(span("Image: ", "gray"), span(basename(path), "white", true)))
    table.insert(lines, line(
        span("Format: ", "gray"),
        span("DSK v" .. tostring(dsk.version or "?"), "cyan"),
        span("  Tracks: ", "gray"),
        span(tostring(dsk.tracksnumber or "?"), "cyan"),
        span("  Sides: ", "gray"),
        span(tostring(dsk.sidesnumber or "?"), "cyan"),
        span("  Track size: ", "gray"),
        span(tostring(dsk.tracksize or "?"), "cyan")
    ))
    table.insert(lines, line(
        span("Entries: ", "gray"),
        span(tostring(#rows), "cyan"),
        span("  Free blocks: ", "gray"),
        span(tostring(free) .. "/" .. tostring(total), "cyan")
    ))
    table.insert(lines, line(span("")))
    table.insert(lines, line(
        span("Usr ", "yellow", true),
        span("Ext ", "yellow", true),
        span("Name         ", "yellow", true),
        span("Rec  ", "yellow", true),
        span("Blk  ", "yellow", true),
        span("Size   ", "yellow", true),
        span("Blocks", "yellow", true)
    ))
    table.insert(lines, line(span(string.rep("-", 72), "gray")))

    if #rows == 0 then
        table.insert(lines, line(span("Empty directory", "gray")))
        return lines
    end

    for _, entry in ipairs(rows) do
        local records = entry.nbrecords or 0
        local blocks = entry.blocks or {}
        local size = records * 128
        table.insert(lines, line(
            span(pad_left(entry.user or 0, 3) .. " ", "white"),
            span(pad_left(entry.numextension or 0, 3) .. " ", "white"),
            span(pad_right(amsdos_name(entry.filename), 13), "lightcyan", true),
            span(pad_left(records, 3) .. "  ", "cyan"),
            span(pad_left(#blocks, 3) .. "  ", "cyan"),
            span(pad_left(size, 5) .. "  ", "green"),
            span(block_list(entry), "white")
        ))
    end

    return lines
end

local function amsdos_import_name(path)
    local name = basename(path):upper()
    local stem, ext = name:match("^([^%.]+)%.([^%.]+)$")
    if not stem then
        stem = name
        ext = "BIN"
    end
    stem = stem:gsub("[^A-Z0-9_%-]", "_"):sub(1, 8)
    ext = ext:gsub("[^A-Z0-9_%-]", "_"):sub(1, 3)
    if stem == "" then
        stem = "FILE"
    end
    if ext == "" then
        ext = "BIN"
    end
    return stem .. "." .. ext
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

local function normalize_tracks_for_write()
    for track = 0, (dsk.tracksnumber or 0) - 1 do
        for side = 0, (dsk.sidesnumber or 1) - 1 do
            local track_data = dsk.tracks and dsk.tracks[track] and dsk.tracks[track][side]
            if track_data and track_data.sector then
                local filler = string.char(track_data.filler or 0xe5)
                for sector = 0, (track_data.sectorsnumber or 0) - 1 do
                    local sector_data = track_data.sector[sector]
                    if sector_data and not sector_data.data then
                        local size = sector_data.size or track_data.sectorssize or 2
                        sector_data.data = string.rep(filler, 256 << (size - 1))
                    end
                end
            end
        end
    end
end

local function add_files_to_dsk(path, files)
    dsk.init()
    dsk.verbose = false
    assert(dsk.read(path), "unable to read DSK")
    dsk.cat()

    for _, source in ipairs(files) do
        local handle = assert(io.open(source, "rb"))
        handle:close()
        assert(
            dsk.saveexternalfile(source, amsdos_import_name(source), dsk.AMSDOS_FILETYPE_BINARY, 0x0000, 0x0000),
            "unable to add file to DSK"
        )
    end

    normalize_tracks_for_write()
    assert(dsk.write(path), "unable to write DSK")
    return true
end

kkc.register_archive_plugin({
    name = "amstrad_dsk",
    version = "1.0.0",
    description = "Amstrad CPC DSK archive plugin",
    extensions = { "dsk" },
    can_handle = function(path)
        return path:lower():match("%.dsk$") ~= nil
    end,
    extract = extract_dsk,
    add_files = add_files_to_dsk,
})

kkc.register_viewer_plugin({
    name = "amstrad_dsk_directory",
    version = "1.0.0",
    description = "Amstrad CPC DSK directory viewer",
    modes = { "text" },
    extensions = { "dsk" },
    render = render_dsk_directory,
})
