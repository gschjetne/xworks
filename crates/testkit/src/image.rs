use std::fmt;
use std::path::Path;

use crate::Error;

/// An image of straight-alpha RGBA pixels, 8 bits a channel, stored row by
/// row from the top left.
#[derive(Clone, PartialEq, Eq)]
pub struct Image {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl Image {
    /// A transparent image.
    pub fn new(width: u32, height: u32) -> Image {
        Image::filled(width, height, [0, 0, 0, 0])
    }

    /// An image filled with one color.
    pub fn filled(width: u32, height: u32, rgba: [u8; 4]) -> Image {
        let pixels = rgba.repeat(width as usize * height as usize);
        Image {
            width,
            height,
            pixels,
        }
    }

    /// An image from its pixels, which must be width × height × 4 bytes.
    pub fn from_rgba(width: u32, height: u32, pixels: Vec<u8>) -> Result<Image, Error> {
        let expected = width as usize * height as usize * 4;
        if pixels.len() != expected {
            return Err(Error::Size {
                width,
                height,
                len: pixels.len(),
                expected,
            });
        }
        Ok(Image {
            width,
            height,
            pixels,
        })
    }

    /// The width, in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// The height, in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// The pixels: red, green, blue and alpha bytes, row by row.
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// The pixel at (x, y), or `None` outside the image.
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let i = (y as usize * self.width as usize + x as usize) * 4;
        Some([
            self.pixels[i],
            self.pixels[i + 1],
            self.pixels[i + 2],
            self.pixels[i + 3],
        ])
    }

    /// Fills a rectangle with one color, replacing what was there. The part
    /// of the rectangle outside the image is ignored.
    pub fn fill_rect(&mut self, x: u32, y: u32, width: u32, height: u32, rgba: [u8; 4]) {
        let right = x.saturating_add(width).min(self.width);
        let bottom = y.saturating_add(height).min(self.height);
        for row in y..bottom {
            let start = (row as usize * self.width as usize + x as usize) * 4;
            let end = (row as usize * self.width as usize + right as usize) * 4;
            if start < end {
                let (pixels, _) = self.pixels[start..end].as_chunks_mut::<4>();
                pixels.fill(rgba);
            }
        }
    }

    /// The image encoded as a PNG file.
    pub fn encode_png(&self) -> Result<Vec<u8>, Error> {
        let mut bytes = Vec::new();
        let mut encoder = png::Encoder::new(&mut bytes, self.width, self.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(&self.pixels)?;
        writer.finish()?;
        Ok(bytes)
    }

    /// Writes the image to a PNG file, making its directory if need be.
    pub fn write_png(&self, path: &Path) -> Result<(), Error> {
        let bytes = self.encode_png()?;
        let io = |source| Error::Io {
            path: path.to_path_buf(),
            source,
        };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(io)?;
        }
        std::fs::write(path, bytes).map_err(io)
    }
}

/// Shows the size only: an image's pixels are too many to print.
impl fmt::Debug for Image {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Image({}×{})", self.width, self.height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fill_rect_is_clipped_to_the_image() {
        let mut image = Image::new(4, 3);
        image.fill_rect(2, 1, 10, 10, [9, 8, 7, 255]);
        assert_eq!(image.pixel(1, 1), Some([0, 0, 0, 0]));
        assert_eq!(image.pixel(2, 1), Some([9, 8, 7, 255]));
        assert_eq!(image.pixel(3, 2), Some([9, 8, 7, 255]));
        assert_eq!(image.pixel(2, 0), Some([0, 0, 0, 0]));
        assert_eq!(image.pixel(4, 0), None);
    }

    #[test]
    fn fill_rect_outside_the_image_changes_nothing() {
        let mut image = Image::new(4, 3);
        image.fill_rect(5, 0, 2, 2, [1, 2, 3, 4]);
        assert_eq!(image, Image::new(4, 3));
    }

    #[test]
    fn from_rgba_checks_the_length() {
        assert!(Image::from_rgba(2, 2, vec![0; 16]).is_ok());
        assert!(matches!(
            Image::from_rgba(2, 2, vec![0; 15]),
            Err(Error::Size { expected: 16, .. })
        ));
    }

    #[test]
    fn png_round_trips() {
        let mut image = Image::filled(5, 4, [255, 255, 255, 255]);
        image.fill_rect(1, 1, 2, 2, [10, 20, 30, 128]);
        let bytes = image.encode_png().unwrap();
        let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        let mut reader = decoder.read_info().unwrap();
        let mut buffer = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut buffer).unwrap();
        assert_eq!((info.width, info.height), (5, 4));
        assert_eq!(&buffer[..info.buffer_size()], image.pixels());
    }
}
