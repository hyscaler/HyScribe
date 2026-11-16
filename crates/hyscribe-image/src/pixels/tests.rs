// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;

/// A `width` by `height` image of `channels` whose samples count up.
fn counting(width: u32, height: u32, channels: usize) -> Pixels {
    let data = (0..width as usize * height as usize * channels)
        .map(|i| (i % 251) as u8)
        .collect();
    Pixels {
        width,
        height,
        channels,
        data,
    }
}

fn grey(width: u32, height: u32, data: &[u8]) -> Pixels {
    Pixels {
        width,
        height,
        channels: 1,
        data: data.to_vec(),
    }
}

#[test]
fn turns_and_mirrors_pixels() {
    // 1 2 3
    // 4 5 6
    let image = grey(3, 2, &[1, 2, 3, 4, 5, 6]);
    assert_eq!(image.rotate(1), grey(2, 3, &[4, 1, 5, 2, 6, 3]));
    assert_eq!(image.rotate(2), grey(3, 2, &[6, 5, 4, 3, 2, 1]));
    assert_eq!(image.rotate(3), grey(2, 3, &[3, 6, 2, 5, 1, 4]));
    assert_eq!(image.rotate(4), image);
    assert_eq!(image.flip(), grey(3, 2, &[4, 5, 6, 1, 2, 3]));
    assert_eq!(image.flop(), grey(3, 2, &[3, 2, 1, 6, 5, 4]));
    let rgb = counting(3, 2, 3);
    assert_eq!(rgb.rotate(1).rotate(3), rgb);
    assert_eq!(rgb.flip().flip(), rgb);
}

#[test]
fn crops_within_the_image() {
    let image = grey(3, 3, &[1, 2, 3, 4, 5, 6, 7, 8, 9]);
    assert_eq!(image.crop(1, 1, 2, 2), grey(2, 2, &[5, 6, 8, 9]));
    assert_eq!(image.crop(2, 0, 5, 1), grey(1, 1, &[3]));
    assert_eq!(image.crop(4, 4, 1, 1), grey(0, 0, &[]));
}

#[test]
fn samples_the_pixel_under_each_centre() {
    let image = grey(4, 1, &[10, 20, 30, 40]);
    assert_eq!(image.sample(2, 1), grey(2, 1, &[20, 40]));
    assert_eq!(
        image.sample(8, 2),
        grey(
            8,
            2,
            &[
                10, 10, 20, 20, 30, 30, 40, 40, 10, 10, 20, 20, 30, 30, 40, 40
            ]
        )
    );
}

#[test]
fn resizes_with_a_triangle_filter() {
    // A flat image stays flat; a ramp halves to the tent's means.
    let flat = grey(10, 6, &[77; 60]);
    assert_eq!(flat.resize(4, 3), grey(4, 3, &[77; 12]));
    let ramp = grey(4, 1, &[0, 100, 200, 250]);
    assert_eq!(ramp.resize(2, 1), grey(2, 1, &[71, 207]));
    // Growing interpolates between neighbours.
    assert_eq!(
        grey(2, 1, &[0, 200]).resize(4, 1),
        grey(4, 1, &[0, 50, 150, 200])
    );
    // Heavy reductions take the vertical pass first, with the same result
    // for a separable picture.
    let wide = counting(40, 40, 1);
    let small = wide.resize(4, 4);
    assert_eq!((small.width, small.height, small.data.len()), (4, 4, 16));
    let tall = wide.resize(39, 39);
    assert_eq!((tall.width, tall.height), (39, 39));
}

#[test]
fn weighs_colour_by_alpha() {
    // A transparent red pixel beside an opaque blue one: the colour comes
    // from the blue alone.
    let image = Pixels {
        width: 2,
        height: 1,
        channels: 4,
        data: vec![255, 0, 0, 0, 0, 0, 255, 255],
    };
    assert_eq!(image.resize(1, 1).data, vec![0, 0, 255, 128]);
    let clear = Pixels {
        width: 2,
        height: 1,
        channels: 2,
        data: vec![9, 0, 9, 0],
    };
    assert_eq!(clear.resize(1, 1).data, vec![0, 0]);
    assert!(clear.has_alpha());
    assert!(!counting(1, 1, 3).has_alpha());
}
