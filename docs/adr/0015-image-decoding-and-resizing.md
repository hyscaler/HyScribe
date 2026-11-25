# 0015: Image decoding and resizing for uploads

- Status: Accepted (2026-11-16)
- Date: 2026-11-16

## Context

When an image is uploaded, WordPress 7.1.3 hands it to `WP_Image_Editor` (Imagick first, then GD) through `wp_create_image_subsizes()`:

- It reads the size and type with `wp_getimagesize()`, and EXIF and IPTC with `wp_read_image_metadata()` (JPEG and TIFF only) into `image_meta`
- Above `big_image_size_threshold` (2560) it saves a `-scaled` copy, which becomes the attached file; below it, a JPEG with an EXIF orientation other than 1 gets a `-rotated` copy (`maybe_exif_rotate()`)
- It makes the registered sizes (thumbnail, medium, medium_large, large, `1536x1536`, `2048x2048` and the theme's) and writes `_wp_attachment_metadata` (width, height, file, filesize, sizes, image_meta)
- It saves at quality 82 (`wp_editor_set_quality`; 86 for WebP), keeps the source's format (HEIC becomes JPEG), strips metadata but keeps colour profiles, and resizes only the first frame of an animation
- On the oracle (`wordpress:7.1.3-php8.4-apache`) the editor is Imagick 3.8.1 over ImageMagick 7.1.1-43 Q16, with JPEG, PNG, GIF, WebP, AVIF and HEIC, a policy (measured with `Imagick::getResourceLimit()`) of at most 32,000 pixels wide, 32,000 high, an area of 256,000,000 pixels and 1 GiB (1,073,741,824 bytes) of pixel cache memory, and a resize of `sampleImage()` to 5 times the target for big reductions, then `resizeImage()` with a triangle filter of support 2.0, then for JPEG `unsharpMaskImage( 0.25, 0.25, 8, 0.065 )`
- When no editor supports a type, the REST upload refuses it (`rest_upload_image_type_not_supported`, 400) and Plupload is told (`webp_upload_error`, `avif_upload_error`); when an editor fails on a file it supports, the attachment is kept with width and height but no sizes
- Since 7.1, on HTTPS the block editor can make the sizes in the browser and upload with `generate_sub_sizes` false

HyScribe must do this in Rust: memory safe, no `unsafe` of its own (ADR 0006), every parser fuzzed, limits on dimensions and memory, no shelling out, inside one static musl binary (ADR 0005), with GPL compatible licences (ADR 0004) and a cargo-vet audit for every new crate (`supply-chain/`; exemptions only shrink).

## Candidates

Audits are those in the six imported sets at `safe-to-deploy` (Google's `ub-risk-N` criteria alone do not imply it). Downloads are crates.io totals on 2026-11-16.

| Crate | Role | Licence | `unsafe` | Imported audits | Fuzzing | Limits it offers |
| --- | --- | --- | --- | --- | --- | --- |
| `image` 0.25.10 | Facade over the codecs | MIT OR Apache-2.0 | A few blocks; always pulls `moxcms` and `pxfm` (colour management, about 500 lines of SIMD `unsafe`, default features on) | Google: 0.23.14 and 0.24.8 `safe-to-run`, 0.24.6 `ub-risk-2` | OSS-Fuzz (`image-rs`), every format | `Limits`: width, height, `max_alloc` (512 MiB, not strict for every decoder) |
| `zune-jpeg` 0.5.15 | JPEG decoder | MIT OR Apache-2.0 OR Zlib | SIMD behind `x86` and `neon` (default); `forbid(unsafe_code)` without them | Google 0.4.19 `ub-risk-3` only | Own cargo-fuzz targets and CI job; through OSS-Fuzz `image-rs` | `set_max_width`, `set_max_height` |
| `png` 0.18.1 | PNG decoder and encoder | MIT OR Apache-2.0 | None (`forbid`) | Google up to 0.18.0-rc, Embark 0.17.9 | OSS-Fuzz (`image-png`) | `Limits { bytes }` (64 MiB default) |
| `gif` 0.14.2 | GIF decoder and encoder | MIT OR Apache-2.0 | None (`forbid`) | Google 0.12.1 `ub-risk-1` only | cargo-fuzz targets; OSS-Fuzz `image-rs` | `MemoryLimit` |
| `image-webp` 0.2.4 | WebP decoder, lossless encoder | MIT OR Apache-2.0 | None (`forbid`) | None | cargo-fuzz targets; OSS-Fuzz `image-rs` | `set_memory_limit`; 16,384 a side by format |
| `jpeg-encoder` 0.7.1 | JPEG encoder | (MIT OR Apache-2.0) AND IJG | None unless the `simd` feature (`forbid` otherwise) | None | cargo-fuzz targets | Not applicable |
| `mozjpeg` 0.10.13 | JPEG encoder | IJG, over C libjpeg | FFI to C | None | Upstream C fuzzing | Not applicable |
| `kamadak-exif` 0.6.2 | EXIF reader | BSD-2-Clause | One cast; `mutate_once` | Google 0.6.1 `ub-risk-1` only | None found upstream | Reads one segment the container bounds |
| `fast_image_resize` 6.1.0 | Resizer | MIT OR Apache-2.0 | About 700 lines of SIMD intrinsics | None | None found | Not applicable |
| `pic-scale-safe` 0.1.12 | Resizer | BSD-3-Clause OR Apache-2.0 | None (`forbid`) | None | None found | Not applicable |
| `rav1d` 1.1.0 | AV1 decoder (for AVIF) | BSD-2-Clause | c2rust port: about 500 lines of `unsafe` plus optional assembly | None | Fuzzed upstream | Frame size options |
| `dav1d`, `avif-rs`, `libavif` | AVIF | MIT, Apache-2.0 | FFI to C | None | Upstream C | Not applicable |
| `rav1d-safe`, `zenavif`, `zenwebp` | AV1, AVIF, lossy WebP | AGPL-3.0-only or commercial | Claims none | None | Young (2026) | Not applicable |
| `gamut-webp` 0.3.1, `gamut-avif` 2.0.0 | Lossy WebP; AVIF container only | MIT OR Apache-2.0 | `forbid` | None | Not established | Not applicable |

Notes:

- `image` is the most used (253 million downloads) but forces `moxcms` with its SIMD defaults on, and `zune-jpeg` with its SIMD defaults on; a downstream crate cannot turn either off, so it would bring about 800 lines of SIMD `unsafe` that no imported audit covers. Its codec crates can be used directly with SIMD off.
- `mozjpeg`, `dav1d` and `libavif` are C: refused (`unsafe` FFI, a C toolchain in the musl build, C parsers on uploaded bytes).
- IJG is GPL compatible by the FSF's list but is not yet in `deny.toml`'s allowlist.
- No pure Rust AVIF decoder is both safe and established: `rav1d` keeps `unsafe` and assembly, `rav1d-safe` is AGPL-3.0-only from one vendor and months old. No mature pure Rust lossy WebP encoder exists under a permissive licence either.
- Transitive crates `crc32fast` and `simd-adler32` (through `png` and `miniz_oxide`) carry audited SIMD `unsafe`; `fdeflate`, `flate2`, `miniz_oxide`, `adler2` and `color_quant` have `safe-to-deploy` audits.

## Decision

Accepted on 2026-11-16 with amendments: an AVIF upload is kept without sizes rather than refused, the limits follow the oracle's ImageMagick policy, resizing runs in a bounded worker pool, and an editor interface and WordPress's hook points sit in front of the resampler.

### The stack

- **A crate, `hyscribe-image`,** with `#![forbid(unsafe_code)]`, pure and synchronous: bytes and the size list go in, the files' bytes and the metadata come out. It holds the ports of `wp_getimagesize()`, `wp_read_image_metadata()` (IPTC included), `maybe_exif_rotate()` and Imagick's resize steps, and has fuzz targets for the header reader, the metadata reader and the whole pipeline under small limits.
- **Codec crates directly, not `image`:** `zune-jpeg` (default features off), `png`, `gif`, `image-webp`, `jpeg-encoder` (default features, no `simd`), `kamadak-exif` for EXIF; IPTC and the image header reader are HyScribe's own small parsers.
- **The IJG licence is allowed for `jpeg-encoder`:** `deny.toml`'s licence allowlist gains `IJG`, with a note that it is there for `jpeg-encoder` only (FSF listed as GPL compatible); the `deny.toml` change lands with the code that adds the crate.
- **HyScribe's own resampler,** ported from the steps Imagick runs (point sample to 5 times the target, triangle filter of support 2.0, the unsharp mask for JPEG), in safe Rust under the coverage and mutation gates; `fast_image_resize` is refused for its `unsafe`, `pic-scale-safe` is the fallback if the port proves too costly.
- **An editor interface shaped like `WP_Image_Editor`** sits in front of the resampler: load, `resize()`, `crop()`, `rotate()` and `save()`, so `wp_create_image_subsizes()`'s port and later the image edit screen call the same operations PHP calls on its editor.
- **WordPress's hook points exist under their names,** with WordPress's arguments, as plain internal calls until the extension runtime (docs/EXTENSIONS.md): `big_image_size_threshold`, `intermediate_image_sizes_advanced`, `wp_editor_set_quality`, `jpeg_quality`, `wp_generate_attachment_metadata` and `image_make_intermediate_size`.
- **Encoding:** JPEG at the WordPress quality with 4:2:0 subsampling, baseline, the ICC profile kept, no EXIF (`jpeg-encoder` uses the IJG quality tables libjpeg uses); PNG with `png`, indexed again when the source was, through `color_quant`; GIF with `gif` and `color_quant`; WebP lossless with `image-webp`, since no safe lossy encoder is ready.
- **Pixels are 8 bits a channel** after decoding; colour profiles are carried, not applied, as Imagick does without a colour space option.

### Limits

All are checked from the header before any pixel buffer is allocated, and are `.env` settings. The defaults of width, height, area and memory are the oracle's ImageMagick policy, measured on `wordpress:7.1.3-php8.4-apache`:

- **Input bytes:** the upload limit; the image layer also refuses to decode above 64 MiB
- **Width:** at most 32,000 pixels
- **Height:** at most 32,000 pixels
- **Area:** at most 256,000,000 pixels (width times height)
- **Memory:** at most 1 GiB (1,073,741,824 bytes) for one image job's buffers at the source's bit depth, checked before decoding and passed to each decoder's own limit (`png::Limits`, `gif::MemoryLimit`, `image-webp`'s memory limit, `zune-jpeg`'s width and height). ImageMagick spills a larger pixel cache to a memory map and disk; HyScribe does not, so above it the image gets no sizes
- **Concurrency:** resizing runs in a bounded worker pool sized to the CPU count, never on a request's thread; jobs beyond the pool wait in a bounded queue, so the total stays the pool's size times one job's budget
- **Animation:** one frame is decoded, the first, as WordPress's Imagick editor does; the original file is kept as uploaded
- **Metadata:** EXIF and IPTC only from their JPEG segments (at most 64 KiB each by format); IPTC keywords and EXIF strings as PHP limits them, bounded by the segment

A file over a limit, or one its decoder rejects, is kept as WordPress keeps an image its editor cannot open: the attachment with width and height (when the header gives them) and no sizes, and a line in the log.

### Formats without a safe codec

- **AVIF:** the upload is kept without intermediate sizes, as WordPress keeps a file its editor cannot handle: the attachment with width and height from the header and no sizes. The proposal to refuse it as PHP refuses it without AVIF support (`rest_upload_image_type_not_supported`, `avif_upload_error`) was not accepted. A safe AVIF decoder is a follow-up.
- **HEIC and HEIF:** accepted unprocessed, as PHP 7.1 lets still HEIC through for the browser to process.
- **WebP:** supported, its sizes written lossless for now; files are larger than PHP's lossy ones. A safe lossy encoder is a follow-up.
- AVIF sizes and lossy WebP are reviewed when a pure Rust codec without `unsafe`, permissively licensed and audited, is established.

### How output differs from PHP, and how the goldens bound it

The encoders and the resampler are not ImageMagick's, so file bytes differ. The goldens (6.3) record PHP's files and metadata over a corpus (the default install, the Theme Test Data images, and edge cases: EXIF orientations 1 to 8, CMYK JPEG, indexed and 16 bit PNG, animated GIF and WebP, images over 2560 and over the limits) and compare:

- **Exact:** file names (`-scaled`, `-rotated`, `-WxH`), the uploads paths, every size's width, height and MIME type, which sizes exist, `image_meta` (EXIF and IPTC fields, orientation reset to 1 after a rotation), the post rows and meta
- **Bounded:** the pixels of each output, decoded by HyScribe's decoders and compared with PHP's at PSNR of at least 35 dB and SSIM of at least 0.97 (starting values, tuned once in 6.3 and then only tightened)
- **Banded:** `filesize`, within half and double PHP's for JPEG, PNG and GIF; not compared for WebP while it is lossless
- **Own regression:** HyScribe's output is deterministic (no SIMD, and each image is resized by one worker alone), so its files are also pinned by hash

### Static musl binary

- Every crate is pure Rust with no build script that compiles C or assembly: no C toolchain, no `nasm`, nothing linked at runtime, so the musl target builds as today
- No runtime CPU dispatch in HyScribe's own code; the decoders run their scalar paths, slower than SIMD but bounded by the limits

## Consequences

- New crates need audits before 6.3: `zune-jpeg`, `zune-core`, `gif` (0.14), `image-webp`, `byteorder-lite`, `quick-error` (2.x), `weezl`, `jpeg-encoder`, `kamadak-exif`, `mutate_once`, and deltas for `png` (from 0.18.0-rc), `flate2`, `miniz_oxide`, `crc32fast` and `simd-adler32` where the imported audits stop short of the versions resolved
- `deny.toml` gains `IJG` in its allowlist, noted as for `jpeg-encoder`, with the code that adds it
- Uploads and sizes match PHP's names, dimensions and metadata exactly, and its pixels within the bounds above; a site whose owner relies on AVIF sizes or small lossy WebP sizes sees a difference: AVIF uploads have no sizes, WebP sizes are larger
- Images over the limits keep working as plain attachments, as they do in PHP when its editor fails

## Decisions (2026-11-16)

1. The stack is accepted: codec crates directly with SIMD off, not `image`; HyScribe's own resampler
2. The IJG licence is allowed for `jpeg-encoder`
3. AVIF: kept without intermediate sizes (not refused); a safe AVIF decoder is a follow-up
4. WebP sizes: lossless for now; a safe lossy encoder is a follow-up
5. Limits: the defaults are the oracle's ImageMagick policy (32,000 wide, 32,000 high, 256,000,000 pixels, 1 GiB), configurable in `.env` and checked from the header before any buffer is allocated
6. The goldens' bounds are accepted as proposed: PSNR 35 dB and SSIM 0.97 to start, `filesize` within half and double
7. Resizing runs in a bounded worker pool sized to the CPUs, never on the request thread
8. An interface shaped like `WP_Image_Editor` sits in front of the resampler, and WordPress's image hook points exist under their names
