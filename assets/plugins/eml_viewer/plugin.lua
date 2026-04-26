local kkc = require("kkc")

local b64_chars = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
local b64_map = {}
for i = 1, #b64_chars do
    b64_map[b64_chars:sub(i, i)] = i - 1
end

local cp1252 = {
    [0x80] = "\226\130\172", [0x82] = "\226\128\154", [0x83] = "\198\146",
    [0x84] = "\226\128\158", [0x85] = "\226\128\166", [0x86] = "\226\128\160",
    [0x87] = "\226\128\161", [0x88] = "\203\134", [0x89] = "\226\128\176",
    [0x8a] = "\197\160", [0x8b] = "\226\128\185", [0x8c] = "\197\146",
    [0x8e] = "\197\189", [0x91] = "\226\128\152", [0x92] = "\226\128\153",
    [0x93] = "\226\128\156", [0x94] = "\226\128\157", [0x95] = "\226\128\162",
    [0x96] = "\226\128\147", [0x97] = "\226\128\148", [0x98] = "\203\156",
    [0x99] = "\226\132\162", [0x9a] = "\197\161", [0x9b] = "\226\128\186",
    [0x9c] = "\197\147", [0x9e] = "\197\190", [0x9f] = "\197\184",
}

local function span(text, fg, bold)
    return { text = text, fg = fg or "white", bg = "black", bold = bold or false }
end

local function line(...)
    return { ... }
end

local function is_eml_path(path)
    local ext = path:match("%.([^%.\\/]+)$")
    if not ext then return false end
    ext = ext:lower()
    return ext == "eml" or ext == "mbox"
end

local function read_all(path)
    local file, err = io.open(path, "rb")
    if not file then return nil, err end
    local data = file:read("*a")
    file:close()
    return data
end

local function decode_latin1(bytes)
    local out = {}
    for i = 1, #bytes do
        local b = bytes:byte(i)
        if b < 0x80 then
            out[#out + 1] = string.char(b)
        elseif b < 0xC0 then
            out[#out + 1] = string.char(0xC0 + math.floor(b / 0x40), 0x80 + (b % 0x40))
        else
            out[#out + 1] = string.char(0xE0 + math.floor(b / 0x1000), 0x80 + (math.floor(b / 0x40) % 0x40), 0x80 + (b % 0x40))
        end
    end
    return table.concat(out)
end

local function decode_cp1252(bytes)
    local out = {}
    for i = 1, #bytes do
        local b = bytes:byte(i)
        out[#out + 1] = cp1252[b] or decode_latin1(string.char(b))
    end
    return table.concat(out)
end

local function decode_charset(bytes, charset)
    charset = (charset or "utf-8"):lower()
    if charset == "iso-8859-1" or charset == "iso8859-1" or charset == "latin-1" or charset == "latin1" then
        return decode_latin1(bytes)
    end
    if charset == "windows-1252" or charset == "cp1252" or charset == "cp-1252" then
        return decode_cp1252(bytes)
    end
    return bytes
end

local function decode_base64(input)
    local clean = input:gsub("%s+", "")
    local out = {}
    local i = 1
    while i <= #clean do
        local c1 = clean:sub(i, i)
        local c2 = clean:sub(i + 1, i + 1)
        local c3 = clean:sub(i + 2, i + 2)
        local c4 = clean:sub(i + 3, i + 3)
        if c1 == "" or c2 == "" then break end
        local n1 = b64_map[c1] or 0
        local n2 = b64_map[c2] or 0
        local n3 = c3 == "=" and 0 or (b64_map[c3] or 0)
        local n4 = c4 == "=" and 0 or (b64_map[c4] or 0)
        out[#out + 1] = string.char((n1 * 4) + math.floor(n2 / 16))
        if c3 ~= "=" and c3 ~= "" then
            out[#out + 1] = string.char(((n2 % 16) * 16) + math.floor(n3 / 4))
        end
        if c4 ~= "=" and c4 ~= "" then
            out[#out + 1] = string.char(((n3 % 4) * 64) + n4)
        end
        i = i + 4
    end
    return table.concat(out)
end

local function decode_qp(input, underscore_as_space)
    local out = {}
    local i = 1
    while i <= #input do
        local ch = input:sub(i, i)
        if underscore_as_space and ch == "_" then
            out[#out + 1] = " "
            i = i + 1
        elseif ch == "=" and input:sub(i + 1, i + 1) == "\n" then
            i = i + 2
        elseif ch == "=" and input:sub(i + 1, i + 2) == "\r\n" then
            i = i + 3
        elseif ch == "=" and input:sub(i + 1, i + 2):match("^%x%x$") then
            out[#out + 1] = string.char(tonumber(input:sub(i + 1, i + 2), 16))
            i = i + 3
        else
            out[#out + 1] = ch
            i = i + 1
        end
    end
    return table.concat(out)
end

local function decode_rfc2047(value)
    return (value:gsub("=%?([^?]+)%?([bBqQ])%?([^?]*)%?=", function(charset, encoding, encoded)
        local bytes
        if encoding:lower() == "b" then
            bytes = decode_base64(encoded)
        else
            bytes = decode_qp(encoded, true)
        end
        return decode_charset(bytes, charset)
    end))
end

local function split_headers_body(text)
    local start, finish = text:find("\n\n", 1, true)
    if start then
        return text:sub(1, start - 1), text:sub(finish + 1)
    end
    return text, ""
end

local function parse_headers(headers)
    local map = {}
    local current_name = nil
    local current_value = ""
    for raw in (headers .. "\n"):gmatch("(.-)\n") do
        local line_text = raw:gsub("\r$", "")
        if (line_text:sub(1, 1) == " " or line_text:sub(1, 1) == "\t") and current_name then
            current_value = current_value .. " " .. line_text:match("^%s*(.-)%s*$")
        else
            if current_name then
                map[current_name] = current_value:match("^%s*(.-)%s*$")
            end
            local name, value = line_text:match("^([^:]+):(.*)$")
            current_name = name and name:lower() or nil
            current_value = value or ""
        end
    end
    return map
end

local function header_param(value, name)
    if not value then return nil end
    name = name:lower()
    for part in value:gmatch(";([^;]+)") do
        local key, val = part:match("^%s*([^=]+)%s*=%s*(.-)%s*$")
        if key and key:lower() == name then
            return (val:gsub("^['\"]", ""):gsub("['\"]$", ""))
        end
    end
    return nil
end

local function html_to_text(input)
    input = input:gsub("<!%-%-.-%-%->", "")
    input = input:gsub("<[sS][cC][rR][iI][pP][tT][^>]*>.-</[sS][cC][rR][iI][pP][tT]>", "")
    input = input:gsub("<[sS][tT][yY][lL][eE][^>]*>.-</[sS][tT][yY][lL][eE]>", "")
    input = input:gsub("<[bB][rR][^>]*>", "\n")
    input = input:gsub("</[pP]>", "\n")
    input = input:gsub("</[dD][iI][vV]>", "\n")
    input = input:gsub("<[^>]+>", "")
    input = input:gsub("&nbsp;", " "):gsub("&lt;", "<"):gsub("&gt;", ">"):gsub("&amp;", "&"):gsub("&quot;", '"')
    return input
end

local function decode_body(body, encoding, content_type, charset)
    encoding = (encoding or ""):lower()
    local decoded
    if encoding == "base64" then
        decoded = decode_charset(decode_base64(body), charset)
    elseif encoding == "quoted-printable" then
        decoded = decode_charset(decode_qp(body, false), charset)
    else
        decoded = decode_charset(body, charset)
    end
    decoded = decoded:gsub("\r\n", "\n"):gsub("\r", "\n")
    if (content_type or ""):lower():match("^text/html") then
        decoded = html_to_text(decoded)
    end
    local lines = {}
    for l in (decoded .. "\n"):gmatch("(.-)\n") do
        lines[#lines + 1] = l
    end
    if #lines == 0 then lines[1] = "" end
    return lines
end

local function multipart_parts(body, boundary)
    local parts = {}
    local marker = "--" .. boundary
    for part in body:gmatch(marker .. "\r?\n(.-)\r?\n" .. marker) do
        parts[#parts + 1] = part
    end
    if #parts == 0 then
        for part in body:gmatch(marker .. "(.-)" .. marker) do
            local cleaned = part:gsub("^%s+", ""):gsub("%s+$", "")
            if cleaned ~= "" and cleaned ~= "--" then
                parts[#parts + 1] = cleaned
            end
        end
    end
    return parts
end

local function best_body(headers, body)
    local content_type = (headers["content-type"] or "text/plain"):lower()
    local charset = header_param(headers["content-type"], "charset") or "utf-8"
    local encoding = headers["content-transfer-encoding"] or ""

    if content_type:match("^multipart/") then
        local boundary = header_param(headers["content-type"], "boundary")
        if boundary then
            local html = nil
            for _, part in ipairs(multipart_parts(body, boundary)) do
                local ph, pb = split_headers_body(part:gsub("^\r?\n", ""))
                local h = parse_headers(ph)
                local ct = (h["content-type"] or "text/plain"):lower()
                local disp = (h["content-disposition"] or ""):lower()
                if not disp:match("^attachment") then
                    local lines = best_body(h, pb)
                    if ct:match("^text/plain") then return lines end
                    if ct:match("^text/html") and not html then html = lines end
                end
            end
            if html then return html end
        end
        return {}
    end

    return decode_body(body, encoding, content_type, charset)
end

local function collect_attachments(headers, body, names)
    names = names or {}
    local content_type = (headers["content-type"] or ""):lower()
    local boundary = header_param(headers["content-type"], "boundary")
    if not content_type:match("^multipart/") or not boundary then return names end
    for _, part in ipairs(multipart_parts(body, boundary)) do
        local ph, pb = split_headers_body(part:gsub("^\r?\n", ""))
        local h = parse_headers(ph)
        local ct = (h["content-type"] or ""):lower()
        local disp = (h["content-disposition"] or ""):lower()
        if ct:match("^multipart/") then
            collect_attachments(h, pb, names)
        elseif disp:match("^attachment") or (ct ~= "" and not ct:match("^text/")) then
            local name = header_param(h["content-disposition"], "filename")
                or header_param(h["content-type"], "name")
                or ct:gsub(";.*$", "")
                or "file"
            names[#names + 1] = decode_rfc2047(name)
        end
    end
    return names
end

local function render_eml(path, mode, state, width)
    if mode ~= "text" or not is_eml_path(path) then return nil end
    local input, err = read_all(path)
    if not input then
        return { line(span("Error opening EML file: " .. tostring(err), "red", true)) }
    end
    input = input:gsub("\r\n", "\n"):gsub("\r", "\n")
    local headers_raw, body = split_headers_body(input)
    local headers = parse_headers(headers_raw)

    local out = {
        line(span("─ Message ", "yellow", true), span(string.rep("─", 50), "yellow", true)),
        line(span("")),
    }
    for _, key in ipairs({ "from", "to", "cc", "reply-to", "date", "subject" }) do
        if headers[key] then
            local label = key:gsub("^%l", string.upper)
            table.insert(out, line(span(label .. ": ", "lightcyan", true), span(decode_rfc2047(headers[key]), "white")))
        end
    end
    if headers["content-type"] then
        table.insert(out, line(span("Content-Type: ", "lightcyan", true), span(decode_rfc2047(headers["content-type"]), "white")))
    end
    if headers["content-transfer-encoding"] then
        table.insert(out, line(span("Encoding: ", "lightcyan", true), span(headers["content-transfer-encoding"], "white")))
    end

    local attachments = collect_attachments(headers, body)
    if #attachments > 0 then
        table.insert(out, line(span("")))
        table.insert(out, line(span("─ Attachments ", "yellow", true), span(string.rep("─", 45), "yellow", true)))
        table.insert(out, line(span("")))
        for _, name in ipairs(attachments) do
            table.insert(out, line(span("  [+] " .. name, "lightyellow", true)))
        end
    end

    table.insert(out, line(span("")))
    table.insert(out, line(span("─ Body ", "yellow", true), span(string.rep("─", 53), "yellow", true)))
    table.insert(out, line(span("")))
    local body_lines = best_body(headers, body)
    if #body_lines == 0 then
        table.insert(out, line(span("(empty body)", "darkgray")))
    else
        for _, text in ipairs(body_lines) do
            table.insert(out, line(span(text, "white")))
        end
    end
    return out
end

kkc.register_viewer_plugin({
    name = "eml_viewer",
    version = "1.0.0",
    description = "Rendered EML/MIME message viewer",
    modes = { "text" },
    mime_types = { "message/rfc822", "application/mbox" },
    render = render_eml,
})
