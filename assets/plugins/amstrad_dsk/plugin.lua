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

-- Detect AMSDOS header using the CRC checksum stored at bytes 65-66 (1-indexed).
-- The checksum is the 16-bit sum of bytes 1-64, combined with a valid file type check.
local function detect_amsdos_header(data)
    if #data < 128 then
        return false
    end
    -- Verify checksum: sum of bytes 1-64 must equal the 16-bit value at bytes 65-66
    local sum = 0
    for i = 1, 64 do
        sum = sum + (data:byte(i) or 0)
    end
    sum = sum & 0xFFFF
    -- Reject trivial all-zero case (would always match for empty/filler blocks)
    if sum == 0 then
        return false
    end
    local lo = data:byte(65) or 0
    local hi = data:byte(66) or 0
    local stored = lo + hi * 256
    if sum ~= stored then
        return false
    end
    -- File type at byte 13 must be a known AMSDOS type: 0=BASIC, 1=Protected, 2=Binary
    local ftype = data:byte(13) or 0xFF
    return ftype <= 2
end

local function strip_amsdos_header(data)
    if not detect_amsdos_header(data) then
        return data
    end
    -- Actual byte length is at positions 25-26 (logical length field, lo-hi, 1-indexed)
    local lo = data:byte(25) or 0
    local hi = data:byte(26) or 0
    local length = lo + hi * 256
    if length > 0 and length <= (#data - 128) then
        return data:sub(129, 128 + length)
    end
    return data:sub(129)
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

    return strip_amsdos_header(data)
end

local function basename(path)
    return path:match("([^/\\]+)$") or path
end

local function file_size(path)
    local file = io.open(path, "rb")
    if not file then
        return 0
    end
    local size = file:seek("end") or 0
    file:close()
    return size
end

local function validate_dsk_header(path)
    local file = io.open(path, "rb")
    if not file then
        return nil, "unable to open DSK"
    end

    local header = file:read(34) or ""
    if header ~= "MV - CPCEMU Disk-File\r\nDisk-Info\r\n"
        and header ~= "EXTENDED CPC DSK File\r\nDisk-Info\r\n" then
        file:close()
        return nil, "not an Amstrad CPC DSK image"
    end

    file:seek("set", 48)
    local tracks = (file:read(1) or "\0"):byte(1) or 0
    local sides = (file:read(1) or "\0"):byte(1) or 0
    local lo = (file:read(1) or "\0"):byte(1) or 0
    local hi = (file:read(1) or "\0"):byte(1) or 0
    local track_size = lo + hi * 256
    local sizes = file:read(204) or ""
    file:close()

    if tracks < 1 or tracks > 84 then
        return nil, "invalid DSK track count"
    end
    if sides < 1 or sides > 2 then
        return nil, "invalid DSK side count"
    end

    local total_track_size = 0
    if header == "EXTENDED CPC DSK File\r\nDisk-Info\r\n" then
        for idx = 1, math.min(#sizes, tracks * sides) do
            total_track_size = total_track_size + (sizes:byte(idx) or 0) * 256
        end
        if total_track_size == 0 then
            return nil, "invalid extended DSK track table"
        end
    else
        if track_size < 256 or track_size > 65535 then
            return nil, "invalid DSK track size"
        end
        total_track_size = tracks * sides * track_size
    end

    local expected_max = 256 + total_track_size
    local size = file_size(path)
    if size > 0 and expected_max > size + 65536 then
        return nil, "DSK header announces more data than the file contains"
    end
    return true
end

local function read_valid_dsk(path)
    local ok, err = validate_dsk_header(path)
    if not ok then
        error(err, 0)
    end
    dsk.init()
    dsk.verbose = false
    assert(dsk.read(path), "unable to read DSK")
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

    read_valid_dsk(path)
    local catalog_ok, catalog_err = pcall(dsk.cat)

    local rows = {}
    if catalog_ok then
        rows = catalog_rows()
    end
    
    -- Group entries by raw_name (filename) to handle multi-part files
    local grouped = {}
    for _, entry in ipairs(rows) do
        grouped[entry.filename] = grouped[entry.filename] or {}
        table.insert(grouped[entry.filename], entry)
    end
    
    -- Sort grouped entries
    local sorted_groups = {}
    for raw_name, entries in pairs(grouped) do
        table.sort(entries, function(left, right)
            return (left.numextension or 0) < (right.numextension or 0)
        end)
        table.insert(sorted_groups, { name = raw_name, entries = entries })
    end
    table.sort(sorted_groups, function(left, right)
        return left.name < right.name
    end)
    
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
        span(tostring(#sorted_groups), "cyan"),
        span("  Free blocks: ", "gray"),
        span(tostring(free) .. "/" .. tostring(total), "cyan")
    ))
    if not catalog_ok then
        table.insert(lines, line(
            span("Catalog: ", "gray"),
            span(tostring(catalog_err or "not readable"), "red", true)
        ))
    end
    table.insert(lines, line(span("")))
    table.insert(lines, line(
        span("Usr ", "yellow", true),
        span("Ext ", "yellow", true),
        span("Name              ", "yellow", true),
        span("Rec  ", "yellow", true),
        span("Blk  ", "yellow", true),
        span("Size   ", "yellow", true),
        span("Blocks", "yellow", true)
    ))
    table.insert(lines, line(span(string.rep("-", 77), "gray")))

    if #sorted_groups == 0 then
        if not catalog_ok then
            table.insert(lines, line(span("This image can be viewed as a disk image, but not entered as an AMSDOS archive.", "gray")))
            return lines
        end
        table.insert(lines, line(span("Empty directory", "gray")))
        return lines
    end

    -- Render each group, showing two entries if AMSDOS header is detected
    for _, group in ipairs(sorted_groups) do
        local raw_name = group.name
        local entries = group.entries
        
        -- Read the full file data
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
        
        -- Check if AMSDOS header is present
        local has_header = detect_amsdos_header(data)
        local first_entry = entries[1]
        local records = first_entry.nbrecords or 0
        local blocks = first_entry.blocks or {}
        local total_blocks = 0
        for _, entry in ipairs(entries) do
            total_blocks = total_blocks + (#(entry.blocks or {}))
        end
        
        if has_header then
            -- Show version with header (.amsd)
            local size_with_header = records * 128
            table.insert(lines, line(
                span(pad_left(first_entry.user or 0, 3) .. " ", "white"),
                span(pad_left(first_entry.numextension or 0, 3) .. " ", "white"),
                span(pad_right(amsdos_name(raw_name) .. ".amsd", 18), "lightcyan"),
                span(pad_left(records, 3) .. "  ", "cyan"),
                span(pad_left(total_blocks, 3) .. "  ", "cyan"),
                span(pad_left(size_with_header, 5) .. "  ", "green"),
                span(block_list(first_entry), "darkgray")
            ))
            
            -- Show user-friendly version (without header)
            local stripped = strip_amsdos_header(data)
            local size_without_header = #stripped
            table.insert(lines, line(
                span(pad_left(first_entry.user or 0, 3) .. " ", "white"),
                span(pad_left(first_entry.numextension or 0, 3) .. " ", "white"),
                span(pad_right(amsdos_name(raw_name), 18), "lightcyan", true),
                span(pad_left(math.ceil(size_without_header / 128), 3) .. "  ", "cyan"),
                span(pad_left(total_blocks, 3) .. "  ", "cyan"),
                span(pad_left(size_without_header, 5) .. "  ", "green"),
                span(block_list(first_entry), "white")
            ))
        else
            -- No AMSDOS header, show only the regular entry
            local size = records * 128
            table.insert(lines, line(
                span(pad_left(first_entry.user or 0, 3) .. " ", "white"),
                span(pad_left(first_entry.numextension or 0, 3) .. " ", "white"),
                span(pad_right(amsdos_name(raw_name), 18), "lightcyan", true),
                span(pad_left(records, 3) .. "  ", "cyan"),
                span(pad_left(total_blocks, 3) .. "  ", "cyan"),
                span(pad_left(size, 5) .. "  ", "green"),
                span(block_list(first_entry), "white")
            ))
        end
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
    read_valid_dsk(path)
    dsk.cat()

    local grouped = {}
    for _, entry in pairs(dsk.catalog or {}) do
        if type(entry) == "table" and entry.filename then
            grouped[entry.filename] = grouped[entry.filename] or {}
            table.insert(grouped[entry.filename], entry)
        end
    end

    for raw_name, entries in pairs(grouped) do
        -- Sort entries by extension number to read them in order
        table.sort(entries, function(left, right)
            return (left.numextension or 0) < (right.numextension or 0)
        end)
        
        -- Read the full file data
        local data = read_catalog_file(entries)
        
        -- Read again without stripping to get full data
        local full_data = ""
        for _, entry in ipairs(entries) do
            local part = ""
            for _, block in ipairs(entry.blocks or {}) do
                part = part .. read_block(block)
            end
            local records = entry.nbrecords or 0
            if records > 0 and records < 128 then
                part = part:sub(1, records * 128)
            end
            full_data = full_data .. part
        end
        
        local friendly_name = amsdos_name(raw_name)
        if friendly_name ~= "" then
            -- Extract the user-friendly version (without header)
            kkc.write_file(kkc.path_join(destination, friendly_name), data)
            
            -- If AMSDOS header is detected, also extract the version with header
            if detect_amsdos_header(full_data) then
                local amsd_name = friendly_name .. ".amsd"
                kkc.write_file(kkc.path_join(destination, amsd_name), full_data)
            end
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
    read_valid_dsk(path)
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
    mime_types = { "application/x-amstrad-cpc-dsk" },
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
    mime_types = { "application/x-amstrad-cpc-dsk" },
    render = render_dsk_directory,
})
