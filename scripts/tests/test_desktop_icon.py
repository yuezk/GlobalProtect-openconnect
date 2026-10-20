"""Render desktop icons through the distro's GdkPixbuf/librsvg loader.

Run with /usr/bin/python3; requires python3-gi and gir1.2-gdkpixbuf-2.0.
"""

from pathlib import Path
import unittest

import gi

gi.require_version("GdkPixbuf", "2.0")
from gi.repository import GdkPixbuf


ROOT = Path(__file__).resolve().parents[2]
ICONS = (
    ROOT / "packaging/files/usr/share/icons/hicolor/scalable/apps/gpgui.svg",
    ROOT / "apps/gpgui-helper/src-tauri/icons/icon.svg",
)


class DesktopIconTests(unittest.TestCase):
    def test_packaged_and_helper_icons_match(self):
        self.assertEqual(ICONS[0].read_bytes(), ICONS[1].read_bytes())

    def test_icons_render_visible_artwork(self):
        # Unsupported filter primitives can load successfully yet produce an
        # entirely transparent icon, so successful decoding alone is not enough.
        for icon in ICONS:
            for size in (32, 48, 128, 256, 512):
                with self.subTest(icon=icon, size=size):
                    pixbuf = GdkPixbuf.Pixbuf.new_from_file_at_scale(
                        str(icon), size, size, True
                    )
                    self.assertEqual((pixbuf.get_width(), pixbuf.get_height()), (size, size))
                    self.assertTrue(pixbuf.get_has_alpha())
                    pixels = pixbuf.get_pixels()
                    stride = pixbuf.get_rowstride()
                    channels = pixbuf.get_n_channels()
                    visible = sum(
                        pixels[y * stride + x * channels + channels - 1] > 128
                        for y in range(size)
                        for x in range(size)
                    )
                    self.assertGreater(visible, size * size * 0.4)


if __name__ == "__main__":
    unittest.main()
