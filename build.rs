extern crate core;

use std::{
    env,
    fs::{read_dir, File},
    io::Write,
    path::Path,
};

use anyhow::Result;
use image::{io::Reader, GenericImageView, Pixel};

fn generate_icons() -> Result<()> {
    let out_dir = env::var_os("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("icons.rs");
    let mut target_file = File::create(dest_path)?;
    writeln!(
        &mut target_file,
        "use embedded_graphics::{{image::ImageRaw,pixelcolor::BinaryColor}};"
    )?;
    for dir_entry in read_dir("./icons")? {
        let dir_entry = dir_entry?;
        let string = dir_entry.file_name();
        let filename = string.to_str().expect("Error on filename encoding");
        if !filename.ends_with(".png") {
            continue;
        }
        let icon_name = &filename[0..filename.len() - 4];
        let img = Reader::open(dir_entry.path())?.decode()?;
        let width = img.width();
        let data_width = (width + 7) / 8 * 8;
        let pixel_count = data_width * img.height();
        let mut pixels = vec![false; pixel_count as usize];
        for (x, y, color) in img.pixels() {
            if color.to_luma().0[0] < 128 {
                if let Some(p) = pixels.get_mut((y * data_width + x) as usize) {
                    *p = true;
                }
            }
        }
        let array_entries = (0..pixels.len() / 8)
            .map(|idx| {
                let offset = idx * 8;
                format!(
                    "0b{}",
                    pixels[offset..offset + 8]
                        .iter()
                        .map(|b| if *b { "1" } else { "0" })
                        .collect::<String>()
                )
            })
            .collect::<Vec<_>>()
            .join(",");

        writeln!(
            &mut target_file,
            "const {}_DATA: &[u8]=&[{}];",
            icon_name.to_ascii_uppercase(),
            array_entries
        )?;
        writeln!(
            &mut target_file,
            "pub const {}: ImageRaw<'_, BinaryColor> = ImageRaw::<BinaryColor>::new({}_DATA, {width});",
            icon_name.to_ascii_uppercase(),
            icon_name.to_ascii_uppercase()
        )?;
    }
    Ok(())
}
fn main() -> Result<()> {
    generate_icons()?;
    Ok(())
}
