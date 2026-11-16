// SPDX-License-Identifier: AGPL-3.0-or-later

//! Decoded pixels and what the editor does to them, as ImageMagick does it
//! for WordPress's Imagick editor: point sampling (`sampleImage()`), the
//! triangle filter of `resizeImage()`, `cropImage()`, quarter turns and
//! mirrors. Eight bits a channel, interleaved;
//! the filters work on floats in the image's own colour space, colour
//! weighted by alpha.

/// An image: its size, its channels (1 grey, 2 grey and alpha, 3 RGB,
/// 4 RGBA) and its samples, row by row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pixels {
    /// The width.
    pub width: u32,
    /// The height.
    pub height: u32,
    /// The channels of a pixel.
    pub channels: usize,
    /// The samples.
    pub data: Vec<u8>,
}

/// One output sample's source samples and their weights.
struct Contribution {
    start: usize,
    weights: Vec<f32>,
}

/// ImageMagick's `MagickEpsilon`.
const EPSILON: f64 = 1.0e-12;

/// The triangle filter, `max( 0, 1 - |x| )`.
fn triangle(x: f64) -> f64 {
    (1.0 - x.abs()).max(0.0)
}

/// Each output position's contributions along a side of `from` samples
/// resized to `to`.
fn contributions(from: usize, to: usize) -> Vec<Contribution> {
    let factor = to as f64 / from as f64;
    // filter:support 2.0 widens the window, not the triangle: the weights
    // past its own support, the scale, are 0.
    let scale = (1.0 / factor).max(1.0);
    (0..to)
        .map(|x| {
            let bisect = (x as f64 + 0.5) / factor + EPSILON;
            let start = (bisect - scale + 0.5).max(0.0) as usize;
            let stop = ((bisect + scale + 0.5) as usize).min(from);
            let weights: Vec<f64> = (start..stop)
                .map(|n| triangle((n as f64 - bisect + 0.5) / scale))
                .collect();
            let density: f64 = weights.iter().sum();
            let weights = weights.iter().map(|w| (w / density) as f32).collect();
            Contribution { start, weights }
        })
        .collect()
}

impl Pixels {
    /// Whether the last channel is alpha.
    pub fn has_alpha(&self) -> bool {
        self.channels.is_multiple_of(2)
    }

    fn row_len(&self) -> usize {
        self.width as usize * self.channels
    }

    /// `sampleImage( $width, $height )`: each output pixel the source pixel
    /// under its centre.
    pub fn sample(&self, width: u32, height: u32) -> Pixels {
        let pick = |x: u32, from: u32, to: u32| {
            ((f64::from(x) + 0.5) * f64::from(from) / f64::from(to)) as usize
        };
        let columns: Vec<usize> = (0..width).map(|x| pick(x, self.width, width)).collect();
        let mut data = Vec::with_capacity(width as usize * height as usize * self.channels);
        for y in 0..height {
            let row = self
                .data
                .chunks(self.row_len())
                .nth(pick(y, self.height, height))
                .unwrap_or_default();
            for &x in &columns {
                data.extend(row.chunks(self.channels).nth(x).unwrap_or_default());
            }
        }
        Pixels {
            width,
            height,
            channels: self.channels,
            data,
        }
    }

    /// `resizeImage( $width, $height, Imagick::FILTER_TRIANGLE, 1 )` with
    /// `filter:support` 2.0: the two passes in ImageMagick's order, the
    /// horizontal first unless the image shrinks to under 0.265 of its area.
    pub fn resize(&self, width: u32, height: u32) -> Pixels {
        let area =
            f64::from(width) / f64::from(self.width) * (f64::from(height) / f64::from(self.height));
        let floats = if area > 0.265 {
            vertical(&horizontal(self, width), height)
        } else {
            horizontal(&vertical(self, height), width)
        };
        floats.into_pixels()
    }

    /// `cropImage( $width, $height, $x, $y )`, within the image.
    pub fn crop(&self, x: u32, y: u32, width: u32, height: u32) -> Pixels {
        let x = x.min(self.width);
        let y = y.min(self.height);
        let width = width.min(self.width - x);
        let height = height.min(self.height - y);
        let mut data = Vec::with_capacity(width as usize * height as usize * self.channels);
        for row in self
            .data
            .chunks(self.row_len())
            .skip(y as usize)
            .take(height as usize)
        {
            let from = x as usize * self.channels;
            data.extend(
                row.get(from..from + width as usize * self.channels)
                    .unwrap_or_default(),
            );
        }
        Pixels {
            width,
            height,
            channels: self.channels,
            data,
        }
    }

    /// The image with each pixel moved: the pixel at output `(x, y)` is the
    /// source's at `source(x, y)`, on an output `width` by `height`.
    fn moved(&self, width: u32, height: u32, source: impl Fn(u32, u32) -> (u32, u32)) -> Pixels {
        let mut data = Vec::with_capacity(self.data.len());
        for y in 0..height {
            for x in 0..width {
                let (sx, sy) = source(x, y);
                let at = (sy as usize * self.width as usize + sx as usize) * self.channels;
                data.extend(self.data.get(at..at + self.channels).unwrap_or_default());
            }
        }
        Pixels {
            width,
            height,
            channels: self.channels,
            data,
        }
    }

    /// `rotateImage( 'none', $degrees )` clockwise for a quarter, half or
    /// three quarter turn.
    pub fn rotate(&self, quarters: u8) -> Pixels {
        let (w, h) = (self.width, self.height);
        match quarters % 4 {
            1 => self.moved(h, w, |x, y| (y, h - 1 - x)),
            2 => self.moved(w, h, |x, y| (w - 1 - x, h - 1 - y)),
            3 => self.moved(h, w, |x, y| (w - 1 - y, x)),
            _ => self.clone(),
        }
    }

    /// `flipImage()`: upside down.
    pub fn flip(&self) -> Pixels {
        self.moved(self.width, self.height, |x, y| (x, self.height - 1 - y))
    }

    /// `flopImage()`: left to right.
    pub fn flop(&self) -> Pixels {
        self.moved(self.width, self.height, |x, y| (self.width - 1 - x, y))
    }
}

/// Samples as floats, colour premultiplied by alpha.
struct Floats {
    width: usize,
    height: usize,
    channels: usize,
    data: Vec<f32>,
}

/// Samples the filter passes read, colour premultiplied by alpha.
trait Plane {
    /// Width, height and channels.
    fn shape(&self) -> (usize, usize, usize);
    /// The sample at `i`.
    fn at(&self, i: usize) -> f32;
}

impl Plane for Pixels {
    fn shape(&self) -> (usize, usize, usize) {
        (self.width as usize, self.height as usize, self.channels)
    }

    fn at(&self, i: usize) -> f32 {
        let value = f32::from(self.data.get(i).copied().unwrap_or(0));
        let channel = i % self.channels;
        if !self.has_alpha() || channel + 1 == self.channels {
            return value;
        }
        let alpha = self
            .data
            .get(i - channel + self.channels - 1)
            .copied()
            .unwrap_or(0);
        value * f32::from(alpha) / 255.0
    }
}

impl Plane for Floats {
    fn shape(&self) -> (usize, usize, usize) {
        (self.width, self.height, self.channels)
    }

    fn at(&self, i: usize) -> f32 {
        self.data.get(i).copied().unwrap_or(0.0)
    }
}

/// The horizontal pass to `width`.
fn horizontal(source: &impl Plane, width: u32) -> Floats {
    let (from, height, channels) = source.shape();
    let plan = contributions(from, width as usize);
    let mut data = Vec::with_capacity(width as usize * height * channels);
    for y in 0..height {
        for c in &plan {
            for channel in 0..channels {
                let at = |n: usize| source.at((y * from + c.start + n) * channels + channel);
                data.push(c.weights.iter().enumerate().map(|(n, w)| w * at(n)).sum());
            }
        }
    }
    Floats {
        width: width as usize,
        height,
        channels,
        data,
    }
}

/// The vertical pass to `height`.
fn vertical(source: &impl Plane, height: u32) -> Floats {
    let (width, from, channels) = source.shape();
    let plan = contributions(from, height as usize);
    let line = width * channels;
    let mut data = Vec::with_capacity(height as usize * line);
    for c in &plan {
        for i in 0..line {
            let at = |n: usize| source.at((c.start + n) * line + i);
            data.push(c.weights.iter().enumerate().map(|(n, w)| w * at(n)).sum());
        }
    }
    Floats {
        width,
        height: height as usize,
        channels,
        data,
    }
}

impl Floats {
    fn into_pixels(self) -> Pixels {
        let alpha = self.channels.is_multiple_of(2);
        let mut data = Vec::with_capacity(self.data.len());
        for pixel in self.data.chunks(self.channels) {
            let a = if alpha {
                pixel.last().copied().unwrap_or(0.0)
            } else {
                255.0
            };
            for (i, &v) in pixel.iter().enumerate() {
                let v = if alpha && i + 1 < self.channels && a > 0.0 {
                    v * 255.0 / a
                } else {
                    v
                };
                data.push(v.round().clamp(0.0, 255.0) as u8);
            }
        }
        Pixels {
            width: self.width as u32,
            height: self.height as u32,
            channels: self.channels,
            data,
        }
    }
}

#[cfg(test)]
mod tests;
