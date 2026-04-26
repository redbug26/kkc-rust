local kkc = require("kkc")

local methods = {
    ["-lh0-"] = true,
    ["-lh5-"] = true,
    ["-lhd-"] = true,
}

local function le16(data, pos)
    local b1, b2 = data:byte(pos, pos + 1)
    return (b1 or 0) | ((b2 or 0) << 8)
end

local function le32(data, pos)
    local b1, b2, b3, b4 = data:byte(pos, pos + 3)
    return (b1 or 0) | ((b2 or 0) << 8) | ((b3 or 0) << 16) | ((b4 or 0) << 24)
end

local function sanitize_path(path)
    path = path:gsub("\\", "/")
    local parts = {}
    for part in path:gmatch("[^/]+") do
        if part ~= "" and part ~= "." and part ~= ".." then
            local clean = part:gsub("[:%z]", "_")
            table.insert(parts, clean)
        end
    end
    if #parts == 0 then
        return "unnamed"
    end
    return table.concat(parts, "/")
end

local function read_header(data, pos)
    local header_size = data:byte(pos)
    if not header_size or header_size == 0 then
        return nil
    end
    assert(pos + header_size + 1 <= #data, "truncated LHA header")

    local method = data:sub(pos + 2, pos + 6)
    local packed_size = le32(data, pos + 7)
    local original_size = le32(data, pos + 11)
    local level = data:byte(pos + 20) or 0
    local name_len = data:byte(pos + 21) or 0
    local name_pos = pos + 22
    local name = data:sub(name_pos, name_pos + name_len - 1)
    local next_pos = pos + header_size + 2

    if level == 1 then
        local ext_size = le16(data, next_pos - 2)
        while ext_size > 0 do
            packed_size = packed_size - ext_size
            next_pos = next_pos + ext_size
            ext_size = le16(data, next_pos - 2)
        end
    elseif level > 1 then
        error("unsupported LHA header level " .. tostring(level))
    end

    return {
        method = method,
        packed_size = packed_size,
        original_size = original_size,
        name = sanitize_path(name),
        data_pos = next_pos,
        next_pos = next_pos + packed_size,
    }
end

local BitReader = {}
BitReader.__index = BitReader

function BitReader.new(data)
    return setmetatable({ data = data, pos = 1, bitbuf = 0, bitcount = 0 }, BitReader)
end

function BitReader:read_bit()
    if self.bitcount == 0 then
        self.bitbuf = self.data:byte(self.pos) or 0
        self.pos = self.pos + 1
        self.bitcount = 8
    end
    self.bitcount = self.bitcount - 1
    return (self.bitbuf >> self.bitcount) & 1
end

function BitReader:read_bits(count)
    local value = 0
    for _ = 1, count do
        value = (value << 1) | self:read_bit()
    end
    return value
end

local function build_tree(lengths)
    local max_len = 0
    local counts = {}
    local symbols = 0
    for symbol = 0, #lengths do
        local len = lengths[symbol] or 0
        if len > 0 then
            counts[len] = (counts[len] or 0) + 1
            max_len = math.max(max_len, len)
            symbols = symbols + 1
        end
    end
    if symbols == 0 then
        return { const = 0 }
    end

    local next_code = {}
    local code = 0
    for bits = 1, max_len do
        code = (code + (counts[bits - 1] or 0)) << 1
        next_code[bits] = code
    end

    local map = {}
    for symbol = 0, #lengths do
        local len = lengths[symbol] or 0
        if len > 0 then
            local sym_code = next_code[len]
            map[len .. ":" .. sym_code] = symbol
            next_code[len] = sym_code + 1
        end
    end

    return { map = map, max_len = max_len }
end

local function decode_symbol(reader, tree)
    if tree.const then
        return tree.const
    end
    local code = 0
    for len = 1, tree.max_len do
        code = (code << 1) | reader:read_bit()
        local symbol = tree.map[len .. ":" .. code]
        if symbol then
            return symbol
        end
    end
    error("invalid LHA Huffman code")
end

local function read_pt_len(reader, count, bit_count, special)
    local n = reader:read_bits(bit_count)
    if n == 0 then
        return build_tree({ [reader:read_bits(bit_count)] = 1 })
    end

    local lengths = {}
    local i = 0
    while i < n do
        local c = reader:read_bits(3)
        if c == 7 then
            while reader:read_bit() == 1 do
                c = c + 1
            end
        end
        lengths[i] = c
        i = i + 1
        if i == special then
            local zeros = reader:read_bits(2)
            for _ = 1, zeros do
                lengths[i] = 0
                i = i + 1
            end
        end
    end
    while i < count do
        lengths[i] = 0
        i = i + 1
    end
    return build_tree(lengths)
end

local function read_c_len(reader, pt_tree)
    local nc = 510
    local lengths = {}
    local n = reader:read_bits(9)
    if n == 0 then
        return build_tree({ [reader:read_bits(9)] = 1 })
    end

    local i = 0
    while i < n do
        local c = decode_symbol(reader, pt_tree)
        if c <= 2 then
            local run
            if c == 0 then
                run = 1
            elseif c == 1 then
                run = reader:read_bits(4) + 3
            else
                run = reader:read_bits(9) + 20
            end
            for _ = 1, run do
                lengths[i] = 0
                i = i + 1
            end
        else
            lengths[i] = c - 2
            i = i + 1
        end
    end
    while i < nc do
        lengths[i] = 0
        i = i + 1
    end
    return build_tree(lengths)
end

local function copy_from_output(out, length, distance)
    local start = #out - distance + 1
    assert(start > 0, "invalid LHA back-reference")
    for idx = 0, length - 1 do
        out[#out + 1] = out[start + idx]
    end
end

local function decode_lh5(packed, original_size)
    local reader = BitReader.new(packed)
    local out = {}
    local block_size = 0
    local c_tree
    local p_tree

    while #out < original_size do
        if block_size == 0 then
            block_size = reader:read_bits(16)
            local pt_tree = read_pt_len(reader, 19, 5, 3)
            c_tree = read_c_len(reader, pt_tree)
            p_tree = read_pt_len(reader, 14, 4, -1)
        end

        block_size = block_size - 1
        local c = decode_symbol(reader, c_tree)
        if c < 256 then
            out[#out + 1] = string.char(c)
        else
            local length = c - 253
            local p = decode_symbol(reader, p_tree)
            if p > 0 then
                p = (1 << (p - 1)) + reader:read_bits(p - 1)
            end
            copy_from_output(out, length, p + 1)
        end
    end

    return table.concat(out):sub(1, original_size)
end

local function extract_member(data, header)
    local packed = data:sub(header.data_pos, header.data_pos + header.packed_size - 1)
    if header.method == "-lh0-" then
        return packed:sub(1, header.original_size)
    end
    if header.method == "-lh5-" then
        return decode_lh5(packed, header.original_size)
    end
    if header.method == "-lhd-" then
        return nil
    end
    error("unsupported LHA method " .. header.method)
end

local function extract_lha_lzh(path, destination)
    local file = assert(io.open(path, "rb"))
    local data = file:read("*all")
    file:close()

    local pos = 1
    while pos <= #data do
        local header = read_header(data, pos)
        if not header then
            break
        end
        assert(methods[header.method], "unsupported LHA method " .. header.method)
        local content = extract_member(data, header)
        if content and header.name ~= "" then
            kkc.write_file(kkc.path_join(destination, header.name), content)
        end
        pos = header.next_pos
    end

    return true
end

kkc.register_archive_plugin({
    name = "lha_lzh",
    version = "1.0.0",
    description = "Pure Lua LHA/LZH archive plugin",
    mime_types = { "application/x-lzh-compressed" },
    can_handle = function(path)
        local lower = path:lower()
        return lower:match("%.lha$") ~= nil or lower:match("%.lzh$") ~= nil
    end,
    extract = extract_lha_lzh,
})
