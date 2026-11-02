use image::ExtendedColorType;
use image::codecs::ico::{IcoEncoder, IcoFrame};
use resvg::{tiny_skia, usvg};
use std::{env, fs, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let svg_path = "assets/images/Inviscid.svg";
    println!("cargo:rerun-if-changed={svg_path}");

    if env::var("CARGO_CFG_TARGET_OS")? != "windows" {
        return Ok(());
    }

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").ok_or("Cargo did not set OUT_DIR")?);
    let tree = usvg::Tree::from_data(&fs::read(svg_path)?, &usvg::Options::default())?;
    let mut frames = Vec::new();

    for size in [16, 20, 24, 32, 40, 48, 64, 128, 256] {
        let mut pixmap = tiny_skia::Pixmap::new(size, size).ok_or("Cannot allocate icon pixmap")?;
        let scale = size as f32 / tree.size().width().max(tree.size().height());
        let transform = tiny_skia::Transform::from_row(
            scale,
            0.0,
            0.0,
            scale,
            (size as f32 - tree.size().width() * scale) / 2.0,
            (size as f32 - tree.size().height() * scale) / 2.0,
        );
        resvg::render(&tree, transform, &mut pixmap.as_mut());
        frames.push(IcoFrame::with_encoded(
            pixmap.encode_png()?,
            size,
            size,
            ExtendedColorType::Rgba8,
        )?);
    }

    IcoEncoder::new(fs::File::create(out_dir.join("inviscid.ico"))?).encode_images(&frames)?;
    let resource_path = out_dir.join("inviscid.rc");
    // GPUI loads resource 1 as the native window and taskbar icon.
    fs::write(&resource_path, "1 ICON \"inviscid.ico\"\n")?;

    embed_resource::compile_for(resource_path, ["inviscid"], embed_resource::NONE)
        .manifest_required()?;
    Ok(())
}
