import os, subprocess
from PIL import Image, ImageDraw

icons_dir = r"p:\Goraw\editors\vscode\icons"
os.makedirs(icons_dir, exist_ok=True)

# 1. Create goraw.svg with precise alignment for VS Code / Seti file tree
# Scaled by 0.88 and shifted right to center (cx=80, cy=64) so min_x aligns at screen pixel 18 with .asm / .obj
svg_content = """<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 128 128" width="128" height="128">
  <defs>
    <linearGradient id="gwGrad" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="#FF1E56" />
      <stop offset="50%" stop-color="#FF6B00" />
      <stop offset="100%" stop-color="#FFAC00" />
    </linearGradient>
    <linearGradient id="darkBg" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="#16181D" />
      <stop offset="100%" stop-color="#0B0C0E" />
    </linearGradient>
  </defs>

  <g transform="translate(80, 64) scale(0.88) translate(-64, -64)">
    <!-- Hexagonal Badge (Original Approved Geometry) -->
    <polygon points="64,6 116,34 116,94 64,122 12,94 12,34" fill="url(#darkBg)" stroke="url(#gwGrad)" stroke-width="5" stroke-linejoin="round" />
    
    <!-- Inner Accent -->
    <polygon points="64,13 109,38 109,90 64,115 19,90 19,38" fill="none" stroke="url(#gwGrad)" stroke-width="1.5" opacity="0.4" stroke-linejoin="round" />

    <!-- Stylized Geometric G Logo (Original Approved Design) -->
    <g>
      <path d="M 88,38 L 48,38 C 36,38 30,46 30,58 L 30,70 C 30,82 36,90 48,90 L 82,90 C 89,90 94,85 94,78 L 94,62 L 62,62 L 62,72 L 82,72 L 82,78 L 48,78 C 42,78 40,74 40,70 L 40,58 C 40,54 42,50 48,50 L 88,50 Z" fill="url(#gwGrad)" />
      <polygon points="64,44 72,56 64,68 56,56" fill="#FFFFFF" opacity="0.95" />
    </g>
  </g>
</svg>"""

svg_path = os.path.join(icons_dir, "goraw.svg")
with open(svg_path, "w", encoding="utf-8") as f:
    f.write(svg_content)

png_path = os.path.join(icons_dir, "goraw.png")

# Try high-fidelity rendering via headless Chrome
chrome_path = r"C:\Program Files\Google\Chrome\Application\chrome.exe"
rendered_via_chrome = False

if os.path.exists(chrome_path):
    temp_html = os.path.join(icons_dir, "_render_tmp.html")
    with open(temp_html, "w", encoding="utf-8") as f:
        f.write("""<!DOCTYPE html>
<html>
<head>
<style>
body { margin: 0; padding: 0; background: transparent; overflow: hidden; width: 128px; height: 128px; }
img { width: 128px; height: 128px; display: block; }
</style>
</head>
<body>
<img src="goraw.svg">
</body>
</html>""")
    try:
        cmd = [
            chrome_path,
            "--headless=new",
            "--no-sandbox",
            "--disable-gpu",
            "--default-background-color=00000000",
            "--window-size=128,128",
            f"--screenshot={png_path}",
            temp_html
        ]
        subprocess.run(cmd, check=True)
        if os.path.exists(png_path):
            im = Image.open(png_path)
            if im.size != (128, 128):
                im = im.crop((0, 0, 128, 128))
                im.save(png_path, "PNG")
            rendered_via_chrome = True
    except Exception as e:
        print("Chrome render fallback:", e)
    finally:
        if os.path.exists(temp_html):
            os.remove(temp_html)

if not rendered_via_chrome:
    # Fallback PIL renderer
    scale = 0.88
    cx, cy = 80, 64
    def tx(x, y):
        return (round(cx + (x - 64) * scale), round(cy + (y - 64) * scale))
    def tx_rect(r):
        p1 = tx(r[0], r[1])
        p2 = tx(r[2], r[3])
        return [p1[0], p1[1], p2[0], p2[1]]

    img = Image.new("RGBA", (128, 128), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)
    hex_orig = [(64, 6), (116, 34), (116, 94), (64, 122), (12, 94), (12, 34)]
    draw.polygon([tx(x, y) for x, y in hex_orig], fill=(20, 22, 28, 255), outline=(255, 60, 40, 255), width=4)
    inner_orig = [(64, 13), (109, 38), (109, 90), (64, 115), (19, 90), (19, 38)]
    draw.polygon([tx(x, y) for x, y in inner_orig], outline=(255, 120, 0, 100), width=1)
    draw.rectangle(tx_rect([40, 36, 88, 48]), fill=(255, 80, 20, 255))
    draw.rectangle(tx_rect([30, 44, 42, 84]), fill=(255, 40, 80, 255))
    draw.rectangle(tx_rect([40, 80, 84, 92]), fill=(255, 140, 0, 255))
    draw.rectangle(tx_rect([78, 62, 90, 84]), fill=(255, 120, 0, 255))
    draw.rectangle(tx_rect([60, 62, 84, 72]), fill=(255, 160, 0, 255))
    spark_orig = [(64, 44), (72, 56), (64, 68), (56, 56)]
    draw.polygon([tx(x, y) for x, y in spark_orig], fill=(255, 255, 255, 245))
    img.save(png_path, "PNG")

print("Generated goraw.svg and goraw.png in", icons_dir)
