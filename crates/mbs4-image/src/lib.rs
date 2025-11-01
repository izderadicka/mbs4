use std::{io::BufWriter, path::Path};

use fast_image_resize::{images::Image, IntoImageView as _, Resizer};
use image::{codecs::png::PngEncoder, GenericImageView, ImageEncoder as _, ImageReader};

type Result<T> = anyhow::Result<T>;

pub const ICON_SIZE: u32 = 128;

fn calculate_dimensions(required: (u32, u32), actual: (u32, u32)) -> (u32, u32) {
    let scale = f32::min(
        required.0 as f32 / actual.0 as f32,
        required.1 as f32 / actual.1 as f32,
    );
    let nw = (actual.0 as f32 * scale).round() as u32;
    let nh = (actual.1 as f32 * scale).round() as u32;
    return (nw.max(1), nh.max(1));
}

pub fn scale_icon(path: impl AsRef<Path> + std::fmt::Debug) -> Result<Vec<u8>> {
    scale_image(path, ICON_SIZE)
}

pub fn scale_image(path: impl AsRef<Path> + std::fmt::Debug, sz: u32) -> Result<Vec<u8>> {
    let img = ImageReader::open(&path)?.decode()?;
    let actual_dim = img.dimensions();
    let (width, height) = calculate_dimensions((sz, sz), actual_dim);
    let mut dst_image = Image::new(
        width,
        height,
        img.pixel_type()
            .ok_or_else(|| anyhow::anyhow!("Cannot get pixel type"))?,
    );
    let mut resizer = Resizer::new();
    resizer.resize(&img, &mut dst_image, None)?;

    let data = Vec::with_capacity(1024);
    let mut writer = BufWriter::new(data);
    PngEncoder::new(&mut writer).write_image(
        dst_image.buffer(),
        width,
        height,
        img.color().into(),
    )?;
    Ok(writer.into_inner()?)
}

#[cfg(test)]
mod tests {
    use image::ImageFormat;

    use super::*;

    #[test]
    fn test_calculate_dimensions() {
        let required = (100, 100);
        let actual = (1000, 1000);
        assert_eq!(calculate_dimensions(required, actual), (100, 100));

        let required = (1000, 1000);
        let actual = (100, 100);
        assert_eq!(calculate_dimensions(required, actual), (1000, 1000));

        let required = (100, 100);
        let actual = (500, 1000);
        assert_eq!(calculate_dimensions(required, actual), (50, 100));
    }

    #[test]
    fn test_scale_icon() {
        let cover_path = Path::new("../../test-data/samples/cover.jpg");
        assert!(cover_path.exists());
        let icon_data = scale_icon(cover_path).unwrap();
        assert!(icon_data.len() > 1024);
        let image = ImageReader::with_format(std::io::Cursor::new(icon_data), ImageFormat::Png)
            .decode()
            .unwrap();
        assert_eq!(image.dimensions().1, 128);
    }
}
