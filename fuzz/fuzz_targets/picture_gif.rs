//! GIF pictures, as a document places them: any bytes decode as a picture
//! within the limits, its first frame, or give an error, never a panic.

#![no_main]

use app::picture::Format;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| xworks_fuzz::picture(Format::Gif, data));
