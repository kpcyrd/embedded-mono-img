mod errors;

use crate::errors::*;
use clap::{ArgAction, Parser};
use env_logger::Env;
use image::{ImageBuffer, ImageFormat, ImageReader, Luma};
use std::fs::File;
use std::io::{self, BufReader, Read, Write};
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(version)]
pub struct Args {
    /// Increase logging output (can be used multiple times)
    #[arg(short, long, global = true, action(ArgAction::Count))]
    verbose: u8,
    /// Threshold to decide if pixel should be on/off
    #[arg(short, long, default_value = "100")]
    threshold: u8,
    /// Don't flush partial bytes after completing a pixel row
    #[arg(short = 'N', long)]
    no_flush_after_pixel_row: bool,
    /// The path to write the output to (- for stdout)
    #[arg(short, long)]
    output: PathBuf,
    /// The path to read the image from (- for stdin)
    input: PathBuf,
}

pub struct Settings {
    threshold: u8,
    no_flush_after_pixel_row: bool,
}

impl From<&Args> for Settings {
    fn from(args: &Args) -> Self {
        Self {
            threshold: args.threshold,
            no_flush_after_pixel_row: args.no_flush_after_pixel_row,
        }
    }
}

pub type Image = ImageBuffer<Luma<u8>, Vec<u8>>;

struct Pack<W> {
    writer: W,
    bits: [u8; 8],
    ctr: usize,
}

impl<W: io::Write> Pack<W> {
    pub fn new(writer: W) -> Self {
        Pack {
            writer,
            bits: Default::default(),
            ctr: 0,
        }
    }

    fn clear(&mut self) {
        self.bits = Default::default();
        self.ctr = 0;
    }

    fn to_byte(&self) -> u8 {
        let mut byte = 0;
        for (ctr, bit) in self.bits.iter().enumerate() {
            if ctr > 0 {
                byte <<= 1;
            }
            byte |= bit;
        }
        byte
    }

    fn write(&mut self) -> Result<()> {
        let byte = self.to_byte();
        debug!("Writing byte to file: 0x{byte:02X}");
        self.writer.write_all(&[byte])?;
        self.clear();
        Ok(())
    }

    pub fn add(&mut self, bit: u8) -> Result<()> {
        self.bits[self.ctr] = bit;
        self.ctr += 1;
        if self.ctr >= self.bits.len() {
            self.write()?;
        }
        Ok(())
    }

    pub fn flush(&mut self) -> Result<()> {
        if self.ctr == 0 {
            return Ok(());
        }
        debug!("Padding incomplete byte with false-y bits");
        self.write()
    }

    pub fn into_inner(self) -> W {
        self.writer
    }
}

pub fn load_image<R: io::BufRead + io::Seek>(reader: R) -> Result<Image> {
    let reader = ImageReader::with_format(reader, ImageFormat::Png);
    let image = reader.decode().context("Failed to decode png image")?;
    let gray_image = image.into_luma8();
    Ok(gray_image)
}

pub fn process_image<W: io::Write>(
    gray_image: &Image,
    output: &mut W,
    settings: &Settings,
) -> Result<()> {
    let mut pack = Pack::new(output);

    // Pack 8 pixels into 1 byte
    for row in gray_image.rows() {
        for px in row {
            trace!("pixel = {px:?}");
            let bit = if px.0[0] > settings.threshold { 1 } else { 0 };
            pack.add(bit).context("Failed to write to output file")?;
        }

        if !settings.no_flush_after_pixel_row {
            pack.flush().context("Failed to write to output file")?;
        }
    }

    // Flush remaining pixels
    pack.flush().context("Failed to write to output file")?;
    pack.into_inner()
        .flush()
        .context("Failed to flush output file")?;

    Ok(())
}

fn main() -> Result<()> {
    let args = Args::parse();
    let log_level = match args.verbose {
        0 => "info",
        1 => "debug",
        _ => "trace",
    };
    env_logger::init_from_env(Env::default().default_filter_or(log_level));

    // Read input file
    let gray_image = if args.input == Path::new("-") {
        let mut buf = vec![];
        io::stdin()
            .read_to_end(&mut buf)
            .context("Failed to read from stdin")?;
        load_image(io::Cursor::new(buf))?
    } else {
        let file = File::open(&args.input)
            .with_context(|| anyhow!("Failed to open input file: {:?}", args.input))?;
        load_image(BufReader::new(file))?
    };

    // Open output file
    let mut output: Box<dyn Write> = if args.output == Path::new("-") {
        Box::new(io::stdout())
    } else {
        let file = File::create(&args.output)
            .with_context(|| anyhow!("Failed to open output file: {:?}", args.output))?;
        Box::new(file)
    };

    // Process image
    process_image(&gray_image, &mut output, &Settings::from(&args))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    impl Default for Settings {
        fn default() -> Settings {
            Self {
                threshold: 100,
                no_flush_after_pixel_row: false,
            }
        }
    }

    #[test]
    fn test_all_true() {
        let mut p = Pack::new(Vec::new());
        for _ in 0..16 {
            p.add(1).unwrap();
        }
        p.flush().unwrap();
        assert_eq!(p.into_inner(), &[0xFF, 0xFF]);
    }

    #[test]
    fn test_all_false() {
        let mut p = Pack::new(Vec::new());
        for _ in 0..16 {
            p.add(0).unwrap();
        }
        p.flush().unwrap();
        assert_eq!(p.into_inner(), &[0x00, 0x00]);
    }

    #[test]
    fn test_some_true() {
        let mut p = Pack::new(Vec::new());
        for _ in 0..16 {
            p.add(1).unwrap();
            p.add(0).unwrap();
        }
        p.flush().unwrap();
        assert_eq!(p.into_inner(), &[0xAA, 0xAA, 0xAA, 0xAA]);
    }

    #[test]
    fn test_unaligned_pixels() {
        let mut p = Pack::new(Vec::new());
        for _ in 0..30 {
            p.add(1).unwrap();
            p.add(0).unwrap();
        }
        p.flush().unwrap();
        assert_eq!(
            p.into_inner(),
            &[0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xA0]
        );
    }

    #[test]
    fn test_convert_bike_png() {
        let png = b"\
iVBORw0KGgoAAAANSUhEUgAAABgAAAAOCAQAAACf8RT1AAABI2lDQ1BJQ0MgcHJvZmlsZQAAKJGd\
kLFKw1AUhr+mRUUUBMVBHDI4CR3t5GBVCEKFWCsYndKkxWJuDElK8Q18E32YDoLgO7gqOPvf6OBg\
Fi8c/o/DOf9/7wXHTSJTtA7ApGXu9bvBZXDlLr7h0GKNXZphVGRd3+9Rez5faVh9aVuv+rk/z0I8\
KiLpXJVGWV5CY1/cmZWZZRUbt4P+kfhB7MYmjcVP4p3YxJbtbt8k0+jH095mZZRenNu+ahuPE07x\
cRkyZUJCSVuaqnNMhz2pR07IPQWRNGGk3kwzJTeiQk4eh6KBSLepyduq8nylDOUxkZdNuMPI0+Zh\
//d77eOs2mxszrMwD6tWU+WMx/D+CKsBrD/D8nVN1tLvt9XMdKqZf77xC9hLUFyVMfiXAAAAAmJL\
R0QAAKqNIzIAAAAJcEhZcwAACxMAAAsTAQCanBgAAACtSURBVCjPrZLBCsIwEETfplJQQcSL//93\
eqkXEbFpx0PSZGmrIJhDSHYzszNDDPHT2nxuJSabVQP8YcId2CO0wrcyoScQANFgRdq0bLorq1U+\
DTQ82GWILQFTOdI4vlg021KSIeSejxgnRMd1OUGZ48bR8T8Z2BZOqwAVcy/aWQTVQ9pDtWRAC4zZ\
UQR6fD/pUM1APgOn2fdDDerCATgDscAMMHqX4sz0t3+V+m/WOjn9Gzyk1gAAAABJRU5ErkJggg==";
        let png = data_encoding::BASE64.decode(png).unwrap();
        let mut output = Vec::new();
        let image = load_image(io::Cursor::new(png)).unwrap();
        process_image(&image, &mut output, &Settings::default()).unwrap();
        assert_eq!(
            output,
            &[
                0x00, 0x00, 0x00, 0x00, 0x07, 0x00, 0x00, 0x07, 0x00, 0x00, 0x3f, 0x00, 0x00, 0x7c,
                0x40, 0x08, 0xff, 0x20, 0x04, 0xff, 0xf0, 0x03, 0xfc, 0xe0, 0x19, 0xff, 0xf8, 0x27,
                0xff, 0x24, 0x43, 0xff, 0x42, 0x42, 0xff, 0x42, 0x24, 0x7e, 0x24, 0x18, 0x00, 0x18
            ]
        );
    }

    #[test]
    fn test_convert_not_multiple_of_8() {
        let png = b"\
iVBORw0KGgoAAAANSUhEUgAAAB4AAAAeCAQAAACROWYpAAAAAXNSR0IB2cksfwAAAARnQU1BAACx\
jwv8YQUAAAAgY0hSTQAAeiYAAICEAAD6AAAAgOgAAHUwAADqYAAAOpgAABdwnLpRPAAAAAJiS0dE\
AACqjSMyAAAACXBIWXMAAC4jAAAuIwF4pT92AAABOElEQVQ4y2Nk+M9ANmBiYBg2mpUYvjH8R4K5\
pGguYeBE4QfhtPs/Jrz2HxV8/M/7H5s6LJo9///5jw66idW85j8muECs5qdwLU/grF//rbFoxgiw\
HAYpOHsSww8oi5Uhg5gA2we37dV/hv9n4bx7hG3mZTCGs88xMDDsh/MUGGIJ2dyFFEiZ/xn+y/z/\
CudvIxRg5+FKn0NFTsJF3uB3tgWDFpx9Fkrvg4sIM9Tic/YiJEcnQcXE/n+Gi53A5+x7cGVPkUSP\
wUW//VfDpTnm/7//hMAMXH6OYmAkmIPtcfn59X/C4M9/X2w21zKIEFF2MDMkYLP5OJLpThgx+gIu\
+wwzwNT+f4NLX8eSircgOT0P3dmFSAXPESzOXY2jUPrP8J/h/3WknGuKNeM/hqv49F8MKsY4AmsM\
AAy2Yn26+qMSAAAAAElFTkSuQmCC";
        let png = data_encoding::BASE64.decode(png).unwrap();
        let mut output = Vec::new();
        let image = load_image(io::Cursor::new(png)).unwrap();
        process_image(&image, &mut output, &Settings::default()).unwrap();
        assert_eq!(
            output,
            [
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x1f, 0xe0, 0x00, 0x00, 0x3f, 0xe0, 0x00, 0x00, 0x3f, 0xe0, 0x00,
                0x00, 0x3f, 0xf0, 0x00, 0x00, 0x7f, 0xf0, 0x00, 0x00, 0x7f, 0xf0, 0x00, 0x00, 0xff,
                0xf8, 0x00, 0x00, 0xfd, 0xf8, 0x00, 0x00, 0xfd, 0xf8, 0x00, 0x01, 0xfc, 0xfc, 0x00,
                0x01, 0xf8, 0xfc, 0x00, 0x01, 0xf8, 0xfe, 0x00, 0x03, 0xf0, 0x7e, 0x00, 0x03, 0xf0,
                0x7e, 0x00, 0x03, 0xff, 0xff, 0x00, 0x07, 0xff, 0xff, 0x00, 0x07, 0xff, 0xff, 0x00,
                0x0f, 0xff, 0xff, 0x80, 0x0f, 0xc0, 0x1f, 0x80, 0x0f, 0xc0, 0x1f, 0xc0, 0x1f, 0xc0,
                0x0f, 0xc0, 0x1f, 0x80, 0x0f, 0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00
            ]
        );
    }
}
