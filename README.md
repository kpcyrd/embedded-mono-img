# embedded-mono-img

This program is for use with
[`embedded_graphics`](https://docs.rs/embedded-graphics/) to convert a .png
into the data needed/used for `ImageRaw::<BinaryColor>` during embedded
firmware development with Rust.

## Usage

```
embedded-mono-img -o img.raw img.png
```

```rust
const IMAGE: ImageRaw<BinaryColor> = ImageRaw::new(include_bytes!("../img.raw"), 24);
```

The file is recommended to be in 8-bit Grayscale.

## Alternative solutions

In the past I used imagemagick like this:

```
convert frame1.png -monochrome -negate frame1.pbm
# use a hex editor, search for start of null bytes
tail -c +11 frame1.pbm > frame1.raw
```

However the exact offset for the `tail` command varied depending on the
presence of metadata (like e.g. a comment). This problem has inspired
development of `embedded-mono-img` so I don't need to worry about this anymore.

## License

`MIT OR Apache-2.0`
