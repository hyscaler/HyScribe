// SPDX-License-Identifier: AGPL-3.0-or-later

//! The whole upload pipeline under small limits takes any bytes: the
//! metadata, the decoders, the resampler and the encoders. Nothing panics,
//! no size is larger than the image, and every file made decodes again.

#![no_main]

use hyscribe_image::codec::{Limits, decode};
use hyscribe_image::header::image_type;
use hyscribe_image::hooks::NoHooks;
use hyscribe_image::subsizes::{Crop, SubSize, Upload, wp_generate_attachment_metadata};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let limits = Limits { bytes: 1 << 20, width: 512, height: 512, area: 65_536, memory: 4 << 20 };
    let Some(kind) = image_type(data) else {
        return;
    };
    let sizes = vec![
        SubSize { name: "thumbnail".into(), width: 16, height: 16, crop: Crop::centre() },
        SubSize { name: "medium".into(), width: 40, height: 0, crop: Crop::No },
    ];
    let upload = Upload { bytes: data, file: "2026/11/fuzz.bin", attachment: 1 };
    let Ok(made) = wp_generate_attachment_metadata(upload, kind.mime(), sizes, &limits, &NoHooks, b"") else {
        return;
    };
    for file in &made.files {
        assert!(u64::from(file.width) * u64::from(file.height) <= limits.area);
        let made = image_type(&file.bytes).map(|kind| decode(&file.bytes, kind, &Limits::default()));
        assert!(matches!(made, Some(Ok(_))), "{}", file.path);
    }
});
