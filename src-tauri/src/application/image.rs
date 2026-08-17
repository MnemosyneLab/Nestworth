use std::{io::Cursor, path::Path};

use image::{imageops::FilterType, DynamicImage, ImageFormat};

use crate::error::AppError;

pub const MAX_INPUT_BYTES: u64 = 5 * 1024 * 1024;
pub const MAX_DIMENSION: u32 = 512;
pub const OUTPUT_MIME: &str = "image/png";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageKind {
    Avatar,
    Logo,
}

pub fn process_image_file(path: &Path, kind: ImageKind) -> Result<Vec<u8>, AppError> {
    let metadata = std::fs::metadata(path).map_err(|_| unreadable())?;
    if !metadata.is_file() {
        return Err(unreadable());
    }
    if metadata.len() > MAX_INPUT_BYTES {
        return Err(too_large());
    }
    let bytes = std::fs::read(path).map_err(|_| unreadable())?;
    process_image_bytes(&bytes, kind)
}

pub fn process_image_bytes(bytes: &[u8], kind: ImageKind) -> Result<Vec<u8>, AppError> {
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_INPUT_BYTES {
        return Err(too_large());
    }
    let format = detect_format(bytes)?;
    let image = image::load_from_memory_with_format(bytes, format).map_err(|_| undecodable())?;
    let processed = match kind {
        ImageKind::Avatar => resize_avatar(image),
        ImageKind::Logo => resize_logo(image),
    };
    encode_png(&processed)
}

fn detect_format(bytes: &[u8]) -> Result<ImageFormat, AppError> {
    if bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Ok(ImageFormat::Png);
    }
    if bytes.len() >= 3 && bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF {
        return Ok(ImageFormat::Jpeg);
    }
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && bytes[8..12] == *b"WEBP" {
        return Ok(ImageFormat::WebP);
    }
    Err(AppError::media_invalid(
        "The file is not a supported image.",
    ))
}

fn resize_avatar(image: DynamicImage) -> DynamicImage {
    let square = center_crop_square(image);
    fit_within(square, MAX_DIMENSION)
}

fn resize_logo(image: DynamicImage) -> DynamicImage {
    fit_within(image, MAX_DIMENSION)
}

fn center_crop_square(image: DynamicImage) -> DynamicImage {
    let width = image.width();
    let height = image.height();
    let side = width.min(height);
    let x = (width - side) / 2;
    let y = (height - side) / 2;
    image.crop_imm(x, y, side, side)
}

fn fit_within(image: DynamicImage, max_dimension: u32) -> DynamicImage {
    if image.width() <= max_dimension && image.height() <= max_dimension {
        image
    } else {
        image.resize(max_dimension, max_dimension, FilterType::Triangle)
    }
}

fn encode_png(image: &DynamicImage) -> Result<Vec<u8>, AppError> {
    let mut encoded = Cursor::new(Vec::new());
    image
        .write_to(&mut encoded, ImageFormat::Png)
        .map_err(|_| AppError::media_invalid("The image could not be encoded."))?;
    Ok(encoded.into_inner())
}

fn unreadable() -> AppError {
    AppError::media_invalid("The image could not be read.")
}

fn too_large() -> AppError {
    AppError::media_invalid("The image is larger than 5 MB.")
}

fn undecodable() -> AppError {
    AppError::media_invalid("The image could not be decoded.")
}

#[cfg(test)]
mod tests {
    use super::{process_image_bytes, ImageKind, MAX_DIMENSION, MAX_INPUT_BYTES};
    use crate::error::{AppError, ErrorCode};
    use image::{DynamicImage, ImageBuffer, ImageFormat, Rgb, Rgba};

    fn rgba_image(width: u32, height: u32, color: [u8; 4]) -> DynamicImage {
        DynamicImage::ImageRgba8(ImageBuffer::from_pixel(width, height, Rgba(color)))
    }

    fn encode(image: &DynamicImage, format: ImageFormat) -> Vec<u8> {
        let mut encoded = std::io::Cursor::new(Vec::new());
        image
            .write_to(&mut encoded, format)
            .expect("test image should encode");
        encoded.into_inner()
    }

    fn decoded_size(png: &[u8]) -> (u32, u32) {
        let image = image::load_from_memory(png).expect("processed png should decode");
        (image.width(), image.height())
    }

    #[test]
    fn rejects_unsupported_bytes_as_media_invalid() {
        let error = process_image_bytes(b"not-an-image", ImageKind::Logo)
            .expect_err("unsupported bytes should fail");
        assert!(matches!(error, AppError::MediaInvalid { .. }));
        assert_eq!(error.into_command_error().code, ErrorCode::MediaInvalid);
    }

    #[test]
    fn rejects_oversized_input_before_decode() {
        let bytes = vec![0_u8; (MAX_INPUT_BYTES + 1) as usize];
        let error =
            process_image_bytes(&bytes, ImageKind::Logo).expect_err("oversized input should fail");
        assert!(matches!(error, AppError::MediaInvalid { message } if message.contains("5 MB")));
    }

    #[test]
    fn avatar_center_crops_to_square_without_upscaling() {
        let png = encode(&rgba_image(80, 40, [255, 0, 0, 255]), ImageFormat::Png);
        let processed = process_image_bytes(&png, ImageKind::Avatar).expect("png avatar");
        assert_eq!(decoded_size(&processed), (40, 40));
    }

    #[test]
    fn logo_keeps_aspect_ratio_and_fits_max_dimension() {
        let png = encode(&rgba_image(800, 400, [0, 0, 255, 255]), ImageFormat::Png);
        let processed = process_image_bytes(&png, ImageKind::Logo).expect("png logo");
        let (width, height) = decoded_size(&processed);
        assert!(width <= MAX_DIMENSION);
        assert!(height <= MAX_DIMENSION);
        assert_eq!(width, MAX_DIMENSION);
        assert_eq!(height, 256);
    }

    #[test]
    fn jpeg_and_webp_normalize_to_png() {
        let jpeg = encode(
            &DynamicImage::ImageRgb8(ImageBuffer::from_pixel(16, 16, Rgb([0, 255, 0]))),
            ImageFormat::Jpeg,
        );
        let webp = encode(&rgba_image(16, 16, [0, 255, 0, 255]), ImageFormat::WebP);
        let jpeg_out = process_image_bytes(&jpeg, ImageKind::Logo).expect("jpeg");
        let webp_out = process_image_bytes(&webp, ImageKind::Logo).expect("webp");
        assert!(jpeg_out.starts_with(&[0x89, 0x50, 0x4E, 0x47]));
        assert!(webp_out.starts_with(&[0x89, 0x50, 0x4E, 0x47]));
    }
}
