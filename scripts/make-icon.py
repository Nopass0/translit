"""Create the simple vector-style application icon as Windows resources."""
from pathlib import Path
from PIL import Image, ImageDraw
directory = Path(__file__).resolve().parents[1] / "src-tauri/icons"
directory.mkdir(exist_ok=True)
image = Image.new("RGBA", (256, 256), (0, 0, 0, 0))
draw = ImageDraw.Draw(image)
draw.rounded_rectangle((4, 4, 252, 252), 58, fill="#93e7cf")
draw.rounded_rectangle((56, 60, 122, 192), 8, outline="#183b2d", width=10)
draw.rounded_rectangle((132, 60, 198, 192), 8, outline="#183b2d", width=10)
draw.line((128, 68, 128, 192), fill="#183b2d", width=10)
for y in (100, 129, 158):
    draw.line((76, y, 104, y), fill="#183b2d", width=7)
    draw.line((150, y, 178, y), fill="#183b2d", width=7)
image.save(directory / "icon.png")
image.save(directory / "icon.ico", sizes=[(16,16), (32,32), (48,48), (64,64), (128,128), (256,256)])
