//! Writes the application icon files from `mathcraft_app::icon` (the artwork is defined in code).
//!
//! ```text
//! cargo run -p mathcraft-app --example make_icons -- crates/mathcraft-app/assets/icons
//! ```
//!
//! Output: `icon-<px>.png` for every size in `icon::ICON_SIZES`, and `mathcraft.ico` (PNG
//! payloads, Windows Vista and later). The macOS `.icns` is made from the PNGs by
//! `packaging/macos/build-app.sh` (`iconutil` only runs on macOS).

use std::fs;
use std::path::PathBuf;

use mathcraft_app::icon::{rgba, ICON_SIZES};

fn main() {
    let dir = PathBuf::from(std::env::args_os().nth(1).unwrap_or_else(|| "icons".into()));
    fs::create_dir_all(&dir).expect("create the output folder");
    let mut pngs = Vec::new();
    for size in ICON_SIZES {
        let path = dir.join(format!("icon-{size}.png"));
        let image = image::RgbaImage::from_raw(size, size, rgba(size)).expect("buffer size");
        image.save(&path).expect("write the PNG");
        pngs.push((size, fs::read(&path).expect("read the PNG back")));
        println!("wrote {}", path.display());
    }
    // The ICO holds the sizes Windows uses for shell and taskbar icons.
    let ico_sizes = [16u32, 32, 48, 256];
    let entries: Vec<&(u32, Vec<u8>)> =
        pngs.iter().filter(|(s, _)| ico_sizes.contains(s)).collect();
    let mut ico = Vec::new();
    ico.extend_from_slice(&0u16.to_le_bytes()); // reserved
    ico.extend_from_slice(&1u16.to_le_bytes()); // type: icon
    ico.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * entries.len() as u32;
    for (size, data) in &entries {
        let dim = if *size >= 256 { 0u8 } else { *size as u8 }; // 0 means 256
        ico.push(dim);
        ico.push(dim);
        ico.push(0); // palette
        ico.push(0); // reserved
        ico.extend_from_slice(&1u16.to_le_bytes()); // colour planes
        ico.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
        ico.extend_from_slice(&(data.len() as u32).to_le_bytes());
        ico.extend_from_slice(&offset.to_le_bytes());
        offset += data.len() as u32;
    }
    for (_, data) in &entries {
        ico.extend_from_slice(data);
    }
    let path = dir.join("mathcraft.ico");
    fs::write(&path, ico).expect("write the ICO");
    println!("wrote {}", path.display());
}
