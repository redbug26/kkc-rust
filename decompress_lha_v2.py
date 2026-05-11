import lhafile
try:
    data = lhafile.Lhafile("Xenon2_clean.lzh")
    for name in data.namelist():
        print(f"Extracting {name}")
        content = data.read(name)
        with open("decompressed_out", "wb") as f:
            f.write(content)
        break
except Exception as e:
    print(f"Error: {e}")
