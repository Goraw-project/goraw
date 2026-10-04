import os

icons_dir = r"p:\Goraw\editors\vscode\icons"
os.makedirs(icons_dir, exist_ok=True)

# 1. folder.svg
folder_svg = """<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="24" height="24" fill="none">
  <path d="M2 6C2 4.89543 2.89543 4 4 4H9.17157C9.70201 4 10.2107 4.21071 10.5858 4.58579L12.4142 6.41421C12.7893 6.78929 13.298 7 13.8284 7H20C21.1046 7 22 7.89543 22 9V18C22 19.1046 21.1046 20 20 20H4C2.89543 20 2 19.1046 2 18V6Z" fill="#6272A4" opacity="0.9" />
</svg>"""

# 2. folder-open.svg
folder_open_svg = """<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="24" height="24" fill="none">
  <path d="M2 6C2 4.89543 2.89543 4 4 4H9.17157C9.70201 4 10.2107 4.21071 10.5858 4.58579L12.4142 6.41421C12.7893 6.78929 13.298 7 13.8284 7H20C21.1046 7 22 7.89543 22 9V11H4C2.89543 11 2 11.8954 2 13V6Z" fill="#6272A4" opacity="0.7" />
  <path d="M2 13C2 12.4477 2.44772 12 3 12H21C21.6441 12 22.1332 12.5857 21.9961 13.2144L20.6885 19.2144C20.5866 19.6817 20.1754 20 19.6924 20H4.30761C3.82464 20 3.41341 19.6817 3.31149 19.2144L2.00388 13.2144C2.00126 13.2024 2 13.087 2 13Z" fill="#798BBF" />
</svg>"""

# 3. file.svg
file_svg = """<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="24" height="24" fill="none">
  <path d="M6 2C4.89543 2 4 2.89543 4 4V20C4 21.1046 4.89543 22 6 22H18C19.1046 22 20 21.1046 20 20V8L14 2H6Z" fill="#3B4252" />
  <path d="M14 2V8H20" fill="#4C566A" />
</svg>"""

# 4. file-rust.svg
rust_svg = """<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="24" height="24" fill="none">
  <path d="M6 2C4.89543 2 4 2.89543 4 4V20C4 21.1046 4.89543 22 6 22H18C19.1046 22 20 21.1046 20 20V8L14 2H6Z" fill="#2E3440" />
  <path d="M14 2V8H20" fill="#4C566A" />
  <circle cx="12" cy="14" r="4.5" stroke="#DEA584" stroke-width="1.5" fill="none" />
  <text x="12" y="16.5" font-size="6.5" font-weight="bold" fill="#DEA584" text-anchor="middle" font-family="sans-serif">R</text>
</svg>"""

# 5. file-cpp.svg
cpp_svg = """<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="24" height="24" fill="none">
  <path d="M6 2C4.89543 2 4 2.89543 4 4V20C4 21.1046 4.89543 22 6 22H18C19.1046 22 20 21.1046 20 20V8L14 2H6Z" fill="#2E3440" />
  <path d="M14 2V8H20" fill="#4C566A" />
  <text x="12" y="16.5" font-size="6.5" font-weight="bold" fill="#519ABA" text-anchor="middle" font-family="sans-serif">C++</text>
</svg>"""

# 6. file-llvm.svg
llvm_svg = """<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="24" height="24" fill="none">
  <path d="M6 2C4.89543 2 4 2.89543 4 4V20C4 21.1046 4.89543 22 6 22H18C19.1046 22 20 21.1046 20 20V8L14 2H6Z" fill="#202634" />
  <path d="M14 2V8H20" fill="#354259" />
  <path d="M9 13C10 11 14 11 15 13C16 15 13 18 12 18C11 18 8 15 9 13Z" fill="#18B7A4" />
</svg>"""

# 7. file-asm.svg
asm_svg = """<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="24" height="24" fill="none">
  <path d="M6 2C4.89543 2 4 2.89543 4 4V20C4 21.1046 4.89543 22 6 22H18C19.1046 22 20 21.1046 20 20V8L14 2H6Z" fill="#2E3440" />
  <path d="M14 2V8H20" fill="#4C566A" />
  <text x="12" y="16.5" font-size="5.5" font-weight="bold" fill="#E5C07B" text-anchor="middle" font-family="sans-serif">ASM</text>
</svg>"""

# 8. file-json.svg
json_svg = """<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="24" height="24" fill="none">
  <path d="M6 2C4.89543 2 4 2.89543 4 4V20C4 21.1046 4.89543 22 6 22H18C19.1046 22 20 21.1046 20 20V8L14 2H6Z" fill="#2E3440" />
  <path d="M14 2V8H20" fill="#4C566A" />
  <text x="12" y="16.5" font-size="7.5" font-weight="bold" fill="#CBCB41" text-anchor="middle" font-family="sans-serif">{}</text>
</svg>"""

# 9. file-toml.svg
toml_svg = """<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="24" height="24" fill="none">
  <path d="M6 2C4.89543 2 4 2.89543 4 4V20C4 21.1046 4.89543 22 6 22H18C19.1046 22 20 21.1046 20 20V8L14 2H6Z" fill="#2E3440" />
  <path d="M14 2V8H20" fill="#4C566A" />
  <text x="12" y="16.5" font-size="5" font-weight="bold" fill="#9C6B4E" text-anchor="middle" font-family="sans-serif">TOML</text>
</svg>"""

# 10. file-markdown.svg
md_svg = """<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="24" height="24" fill="none">
  <path d="M6 2C4.89543 2 4 2.89543 4 4V20C4 21.1046 4.89543 22 6 22H18C19.1046 22 20 21.1046 20 20V8L14 2H6Z" fill="#2E3440" />
  <path d="M14 2V8H20" fill="#4C566A" />
  <text x="12" y="16.5" font-size="6" font-weight="bold" fill="#4B89DC" text-anchor="middle" font-family="sans-serif">MD</text>
</svg>"""

files = {
    "folder.svg": folder_svg,
    "folder-open.svg": folder_open_svg,
    "file.svg": file_svg,
    "file-rust.svg": rust_svg,
    "file-cpp.svg": cpp_svg,
    "file-llvm.svg": llvm_svg,
    "file-asm.svg": asm_svg,
    "file-json.svg": json_svg,
    "file-toml.svg": toml_svg,
    "file-markdown.svg": md_svg,
}

for name, content in files.items():
    with open(os.path.join(icons_dir, name), "w", encoding="utf-8") as f:
        f.write(content)

print(f"Successfully wrote {len(files)} icons to {icons_dir}")
