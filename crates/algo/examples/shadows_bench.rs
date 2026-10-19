//! Shadows / Highlights preview cost on a proxy-sized image: `cargo run -p photocraft-algo --example shadows_bench -- [width height]`.
use photocraft_algo::camera_raw::{CameraRaw, develop};
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (w, h) = (args.get(1).and_then(|s| s.parse().ok()).unwrap_or(900usize), args.get(2).and_then(|s| s.parse().ok()).unwrap_or(600usize));
    // A wide-range picture: a broad luminance ramp, a dark region and fine texture, so the filter sees about 9 stops.
    let mut base = vec![[0.0f32; 4]; w * h];
    let mut seed = 12345u32;
    for y in 0..h {
        for x in 0..w {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let noise = (seed >> 16) as f32 / 65535.0 - 0.5;
            let ramp = (x as f32 / w as f32).powf(2.2);
            let dark = if (x / 90 + y / 90) % 3 == 0 { 0.03 } else { 1.0 };
            let v = (ramp * dark * 0.9 + 0.004 + noise * 0.02 * ramp).clamp(0.0, 1.0);
            base[y * w + x] = [v, v * 0.95, v * 0.9, 1.0];
        }
    }
    if args.iter().any(|a| a == "--hash") {
        // Bit-exactness check: an FNV-1a hash of every output sample's bits, over several settings and sizes.
        for (cw, ch) in [(w, h), (1013, 677), (2400, 1600), (97, 53)] {
            let mut px0 = vec![[0.0f32; 4]; cw * ch];
            let mut seed = 777u32;
            for y in 0..ch {
                for x in 0..cw {
                    seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                    let noise = (seed >> 16) as f32 / 65535.0 - 0.5;
                    let v = ((x as f32 / cw as f32).powf(1.7) * (0.2 + 0.8 * ((y as f32 * 0.05).sin() * 0.5 + 0.5)) + 0.002 + noise * 0.03).clamp(0.0, 1.0);
                    px0[y * cw + x] = [v, v * 0.9, v * 0.8, 1.0];
                }
            }
            for (shadows, highlights) in [(60.0, 0.0), (-40.0, 0.0), (0.0, -60.0), (0.0, 50.0), (35.0, -25.0)] {
                let params = CameraRaw { shadows, highlights, ..Default::default() };
                let mut px = px0.clone();
                develop(&mut px, cw, ch, &params, false);
                let mut hash = 0xcbf29ce484222325u64;
                for q in &px {
                    for c in q {
                        hash = (hash ^ u64::from(c.to_bits())).wrapping_mul(0x100000001b3);
                    }
                }
                println!("hash {cw}x{ch} shadows {shadows:>5} highlights {highlights:>5}: {hash:016x}");
            }
        }
        return;
    }
    for (name, params) in [
        ("neutral", CameraRaw::default()),
        ("shadows +60", CameraRaw { shadows: 60.0, ..Default::default() }),
        ("highlights -60", CameraRaw { highlights: -60.0, ..Default::default() }),
        ("exposure", CameraRaw { exposure: 0.5, ..Default::default() }),
    ] {
        let mut best = f64::MAX;
        let mut all = Vec::new();
        for _ in 0..6 {
            let mut px = base.clone();
            let t = Instant::now();
            develop(&mut px, w, h, &params, false);
            let ms = t.elapsed().as_secs_f64() * 1000.0;
            all.push(ms);
            best = best.min(ms);
        }
        all.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!("{name:<16} best {best:7.1} ms   median {:7.1} ms   ({w}x{h})", all[all.len() / 2]);
    }
}
