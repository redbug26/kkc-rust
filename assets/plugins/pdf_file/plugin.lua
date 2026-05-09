local kkc = require("kkc")

local function trim_stream_data(data)
    if data:sub(1, 2) == "\r\n" then
        return data:sub(3)
    end
    if data:sub(1, 1) == "\n" or data:sub(1, 1) == "\r" then
        return data:sub(2)
    end
    return data
end

local function pdf_unescape_literal(value)
    local out = {}
    local idx = 1
    while idx <= #value do
        local ch = value:sub(idx, idx)
        if ch == "\\" then
            local next_ch = value:sub(idx + 1, idx + 1)
            if next_ch == "n" then
                table.insert(out, "\n")
                idx = idx + 2
            elseif next_ch == "r" then
                table.insert(out, "\r")
                idx = idx + 2
            elseif next_ch == "t" then
                table.insert(out, "\t")
                idx = idx + 2
            elseif next_ch == "b" or next_ch == "f" then
                idx = idx + 2
            elseif next_ch == "(" or next_ch == ")" or next_ch == "\\" then
                table.insert(out, next_ch)
                idx = idx + 2
            else
                local oct = value:sub(idx + 1, idx + 3)
                if oct:match("^[0-7][0-7]?[0-7]?") then
                    local matched = oct:match("^[0-7][0-7]?[0-7]?")
                    table.insert(out, string.char(tonumber(matched, 8)))
                    idx = idx + 1 + #matched
                else
                    table.insert(out, next_ch)
                    idx = idx + 2
                end
            end
        else
            table.insert(out, ch)
            idx = idx + 1
        end
    end
    return table.concat(out)
end

local function decode_hex_string(value)
    value = value:gsub("%s+", "")
    if #value % 2 == 1 then
        value = value .. "0"
    end
    local out = {}
    for idx = 1, #value, 2 do
        table.insert(out, string.char(tonumber(value:sub(idx, idx + 1), 16) or 0))
    end
    return table.concat(out)
end

local function extract_pdf_strings(content)
    local out = {}
    for literal in content:gmatch("%b()") do
        table.insert(out, pdf_unescape_literal(literal:sub(2, -2)))
    end
    for hex in content:gmatch("<([0-9A-Fa-f%s]+)>") do
        if #hex:gsub("%s+", "") >= 2 then
            table.insert(out, decode_hex_string(hex))
        end
    end
    return out
end

local function stream_objects(data)
    local objects = {}
    local pos = 1
    while true do
        local obj_start, stream_word, object_no, generation =
            data:find("(%d+)%s+(%d+)%s+obj.-stream", pos)
        if not obj_start then
            break
        end

        local header = data:sub(obj_start, stream_word - #"stream")
        local stream_start = stream_word + 1
        local stream_end_start, stream_end = data:find("endstream", stream_start, true)
        if not stream_end_start then
            break
        end

        local stream = trim_stream_data(data:sub(stream_start, stream_end_start - 1))
        table.insert(objects, {
            object_no = tonumber(object_no),
            generation = tonumber(generation),
            header = header,
            stream = stream,
        })
        pos = stream_end + 1
    end
    return objects
end

local function dict_number(header, name)
    local value = header:match("/" .. name .. "%s+([%d%.%-]+)")
    return value and tonumber(value) or nil
end

local function dict_name(header, name)
    return header:match("/" .. name .. "%s*/([A-Za-z0-9]+)")
end

local function has_filter(header, filter)
    return header:find("/Filter%s*/" .. filter) ~= nil or header:find("/" .. filter) ~= nil
end

local function is_image(header)
    return header:find("/Subtype%s*/Image") ~= nil
end

local function unique_name(used, page, ext)
    local idx = 0
    while true do
        local name
        if idx == 0 then
            name = string.format("Page %d.%s", page, ext)
        else
            name = string.format("Page %d (%d).%s", page, idx, ext)
        end
        if not used[name] then
            used[name] = true
            return name
        end
        idx = idx + 1
    end
end

local function le32(value)
    return string.char(value & 255, (value >> 8) & 255, (value >> 16) & 255, (value >> 24) & 255)
end

local function le16(value)
    return string.char(value & 255, (value >> 8) & 255)
end

local function bmp_from_raw(header, data)
    local width = dict_number(header, "Width")
    local height = dict_number(header, "Height")
    local bits = dict_number(header, "BitsPerComponent") or 8
    if not width or not height or bits ~= 8 then
        return nil
    end

    local color_space = dict_name(header, "ColorSpace") or ""
    local components
    if color_space == "DeviceGray" or #data == width * height then
        components = 1
    elseif color_space == "DeviceRGB" or #data == width * height * 3 then
        components = 3
    else
        return nil
    end

    local row_size = ((width * 3 + 3) // 4) * 4
    local pixel_size = row_size * height
    local file_size = 54 + pixel_size
    local rows = {}

    for y = height - 1, 0, -1 do
        local row = {}
        for x = 0, width - 1 do
            local source = y * width * components + x * components + 1
            local r, g, b
            if components == 1 then
                local gray = data:byte(source) or 0
                r, g, b = gray, gray, gray
            else
                r = data:byte(source) or 0
                g = data:byte(source + 1) or 0
                b = data:byte(source + 2) or 0
            end
            table.insert(row, string.char(b, g, r))
        end
        table.insert(row, string.rep("\0", row_size - width * 3))
        table.insert(rows, table.concat(row))
    end

    return "BM"
        .. le32(file_size)
        .. string.rep("\0", 4)
        .. le32(54)
        .. le32(40)
        .. le32(width)
        .. le32(height)
        .. le16(1)
        .. le16(24)
        .. le32(0)
        .. le32(pixel_size)
        .. le32(2835)
        .. le32(2835)
        .. le32(0)
        .. le32(0)
        .. table.concat(rows)
end

local function extract_pdf(path, destination)
    local file = assert(io.open(path, "rb"))
    local data = file:read("*all")
    file:close()

    local text = {}
    local used = {}
    local page = 1

    for _, object in ipairs(stream_objects(data)) do
        if is_image(object.header) then
            if has_filter(object.header, "DCTDecode") then
                kkc.write_file(kkc.path_join(destination, unique_name(used, page, "jpg")), object.stream)
                page = page + 1
            elseif has_filter(object.header, "JPXDecode") then
                kkc.write_file(kkc.path_join(destination, unique_name(used, page, "jp2")), object.stream)
                page = page + 1
            elseif not object.header:find("/Filter") then
                local bmp = bmp_from_raw(object.header, object.stream)
                if bmp then
                    kkc.write_file(kkc.path_join(destination, unique_name(used, page, "bmp")), bmp)
                    page = page + 1
                end
            end
        elseif not object.header:find("/Filter") then
            for _, value in ipairs(extract_pdf_strings(object.stream)) do
                table.insert(text, value)
            end
        end
    end

    if #text > 0 then
        kkc.write_file(kkc.path_join(destination, "data.txt"), table.concat(text, "\n"))
    end

    return true
end

kkc.register_archive_plugin({
    can_handle = function(path)
        return path:lower():match("%.pdf$") ~= nil
    end,
    extract = extract_pdf,
})
