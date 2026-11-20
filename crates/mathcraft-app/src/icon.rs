//! The application icon: the Mathcraft logo (a rounded tile with a silver sigma and a squared
//! exponent over a faint grid and a rising curve). The artwork is `assets/icons/logo-master.png`,
//! a 1024 px RGBA image with transparent corners; every size, the window icon and the files in
//! `assets/icons/` (made by `examples/make_icons.rs`) are scaled from it.

use std::sync::OnceLock;

use eframe::egui;
use image::imageops::FilterType;

/// Sizes of the icon files: 16 to 1024 px, covering the macOS iconset, the Windows `.ico` and the
/// Linux hicolor theme.
pub const ICON_SIZES: [u32; 8] = [16, 32, 48, 64, 128, 256, 512, 1024];

/// The master artwork (PNG, 1024 x 1024, straight alpha).
const MASTER_PNG: &[u8] = include_bytes!("../assets/icons/logo-master.png");

/// The decoded master, once.
fn master() -> &'static image::RgbaImage {
    static MASTER: OnceLock<image::RgbaImage> = OnceLock::new();
    MASTER.get_or_init(|| {
        image::load_from_memory(MASTER_PNG)
            .expect("the embedded logo is a valid PNG")
            .to_rgba8()
    })
}

/// RGBA pixels (straight alpha, row by row from the top) of the icon at `size` x `size`.
pub fn rgba(size: u32) -> Vec<u8> {
    let n = size.max(1);
    let master = master();
    if n == master.width() {
        return master.as_raw().clone();
    }
    image::imageops::resize(master, n, n, FilterType::Lanczos3).into_raw()
}

/// The icon as the window icon data of egui.
pub fn icon_data(size: u32) -> egui::IconData {
    egui::IconData {
        rgba: rgba(size),
        width: size,
        height: size,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corners_are_transparent_and_the_tile_is_opaque() {
        let size = 64;
        let px = rgba(size);
        assert_eq!(px.len(), (size * size * 4) as usize);
        let alpha = |x: u32, y: u32| px[((y * size + x) * 4 + 3) as usize];
        assert_eq!(alpha(0, 0), 0);
        assert_eq!(alpha(size - 1, size - 1), 0);
        assert_eq!(alpha(size / 2, size / 2), 255);
    }

    #[test]
    fn the_sigma_is_light_on_a_dark_tile() {
        let size = 256;
        let px = rgba(size);
        let lum = |x: f32, y: f32| {
            let i = ((((y * size as f32) as u32) * size + (x * size as f32) as u32) * 4) as usize;
            (u32::from(px[i]) + u32::from(px[i + 1]) + u32::from(px[i + 2])) / 3
        };
        // The top bar of the sigma is silver; the tile near the left edge is dark.
        assert!(lum(0.38, 0.30) > 170, "{}", lum(0.38, 0.30));
        assert!(lum(0.12, 0.50) < 60, "{}", lum(0.12, 0.50));
    }

    #[test]
    fn every_icon_size_has_the_right_buffer_length() {
        for size in ICON_SIZES {
            assert_eq!(rgba(size).len(), (size * size * 4) as usize, "{size}");
        }
    }
}
