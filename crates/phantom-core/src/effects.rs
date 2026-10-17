use crate::model;

pub fn hsv_to_rgb(h: f64, s: f64, v: f64) -> (f64, f64, f64) {
    if s <= 0.0 {
        return (v, v, v);
    }
    let h = (h.rem_euclid(1.0)) * 6.0;
    let i = h.floor() as i32;
    let f = h - i as f64;
    let p = v * (1.0 - s);
    let q = v * (1.0 - s * f);
    let t = v * (1.0 - s * (1.0 - f));
    match i {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    }
}

pub fn calculate_key_color(key: &model::Key, p: &model::Profile, elapsed: f64) -> (f64, f64, f64) {
    if p.lighting.mode == 20 {
        let color = p.colors.get(&key.id).copied().unwrap_or([0, 0, 0]);
        let b = p.lighting.brightness as f64 / 4.0;
        return (
            color[0] as f64 / 255.0 * b,
            color[1] as f64 / 255.0 * b,
            color[2] as f64 / 255.0 * b,
        );
    }

    let speed_mult = match p.lighting.speed {
        0 => 0.4,
        1 => 0.7,
        2 => 1.0,
        3 => 1.5,
        _ => 2.2,
    };
    let t = elapsed * speed_mult;
    let dir = if p.lighting.direction == 0 { 1.0 } else { -1.0 };
    let brightness = (p.lighting.brightness as f64 / 4.0).clamp(0.0, 1.0);
    let base_rgb = (
        p.lighting.color[0] as f64 / 255.0,
        p.lighting.color[1] as f64 / 255.0,
        p.lighting.color[2] as f64 / 255.0,
    );

    match p.lighting.mode {
        1 => {
            let phase = key.x * 0.008 * dir - t * 2.5;
            let wave = (phase.sin() * 0.5 + 0.5).powf(1.8);
            if p.lighting.rainbow {
                let hue = (key.x * 0.0012 * dir - t * 0.4).rem_euclid(1.0);
                let (r, g, b) = hsv_to_rgb(hue, 0.9, 1.0);
                (
                    r * wave * brightness,
                    g * wave * brightness,
                    b * wave * brightness,
                )
            } else {
                (
                    base_rgb.0 * wave * brightness,
                    base_rgb.1 * wave * brightness,
                    base_rgb.2 * wave * brightness,
                )
            }
        }
        2 => {
            let w1 = (key.x * 0.006 * dir + key.y * 0.008 + t * 1.2).sin();
            let w2 = (key.x * 0.004 - key.y * 0.012 - t * 0.8).cos();
            let wave = ((w1 + w2) * 0.35 + 0.65).clamp(0.1, 1.0);
            if p.lighting.rainbow {
                let hue = (key.x * 0.0008 + key.y * 0.001 + t * 0.25 * dir).rem_euclid(1.0);
                let (r, g, b) = hsv_to_rgb(hue, 0.85, 1.0);
                (
                    r * wave * brightness,
                    g * wave * brightness,
                    b * wave * brightness,
                )
            } else {
                (
                    base_rgb.0 * wave * brightness,
                    base_rgb.1 * wave * brightness,
                    base_rgb.2 * wave * brightness,
                )
            }
        }
        3 => {
            let row = ((key.y - 10.0) / 45.0).clamp(0.0, 5.0) as i32;
            let row_norm = if (row % 2 == 0) ^ (dir < 0.0) {
                key.x / 888.0
            } else {
                1.0 - key.x / 888.0
            };
            let path_pos = row as f64 + row_norm;
            let head = (t * 3.0).rem_euclid(6.5);
            let dist = (path_pos - head).rem_euclid(6.5);
            let tail_len = 1.2;
            let intensity = if dist < tail_len {
                (1.0 - dist / tail_len).powf(1.5)
            } else {
                0.05
            };
            if p.lighting.rainbow {
                let hue = (path_pos * 0.15 + t * 0.2).rem_euclid(1.0);
                let (r, g, b) = hsv_to_rgb(hue, 0.95, 1.0);
                (
                    r * intensity * brightness,
                    g * intensity * brightness,
                    b * intensity * brightness,
                )
            } else {
                (
                    base_rgb.0 * intensity * brightness,
                    base_rgb.1 * intensity * brightness,
                    base_rgb.2 * intensity * brightness,
                )
            }
        }
        4 => {
            let hue = (t * 0.25).rem_euclid(1.0);
            let (r, g, b) = hsv_to_rgb(hue, 1.0, 1.0);
            (r * brightness, g * brightness, b * brightness)
        }
        5 => {
            let cycle = t * 1.8;
            let wave = (cycle.sin() * 0.5 + 0.5).powf(2.0);
            let color = if p.lighting.rainbow {
                let step = (cycle / std::f64::consts::TAU).floor();
                let hue = (step * 0.16 + 0.1).rem_euclid(1.0);
                hsv_to_rgb(hue, 1.0, 1.0)
            } else {
                base_rgb
            };
            (
                color.0 * wave * brightness,
                color.1 * wave * brightness,
                color.2 * wave * brightness,
            )
        }
        6 => (
            base_rgb.0 * brightness,
            base_rgb.1 * brightness,
            base_rgb.2 * brightness,
        ),
        7 => {
            let tap_interval = 0.8;
            let cycle_idx = (t / tap_interval).floor() as u64;
            let time_in_cycle = t % tap_interval;
            let pseudo_key_id = ((cycle_idx.wrapping_mul(6364136223846793005) >> 32) % 87) as usize;
            let prev_id =
                ((cycle_idx.wrapping_sub(1).wrapping_mul(6364136223846793005) >> 32) % 87) as usize;
            let is_hit = key.id == pseudo_key_id;
            let is_prev = key.id == prev_id;
            let fade = if is_hit {
                (1.0 - time_in_cycle / tap_interval).max(0.0)
            } else if is_prev {
                (1.0 - (time_in_cycle + tap_interval) / (tap_interval * 2.0)).max(0.0)
            } else {
                0.0
            };
            let intensity = (0.08 + fade * 0.92).clamp(0.0, 1.0);
            if p.lighting.rainbow {
                let hue = (key.id as f64 * 0.08 + t * 0.2).rem_euclid(1.0);
                let (r, g, b) = hsv_to_rgb(hue, 0.9, 1.0);
                (
                    r * intensity * brightness,
                    g * intensity * brightness,
                    b * intensity * brightness,
                )
            } else {
                (
                    base_rgb.0 * intensity * brightness,
                    base_rgb.1 * intensity * brightness,
                    base_rgb.2 * intensity * brightness,
                )
            }
        }
        8 => {
            let dx = key.x + key.width / 2.0 - 444.0;
            let dy = key.y + key.height / 2.0 - 150.0;
            let dist = (dx * dx + dy * dy).sqrt();
            let wave = ((dist * 0.02 - t * 4.0).sin() * 0.5 + 0.5).powf(3.0);
            let intensity = wave.clamp(0.08, 1.0);
            if p.lighting.rainbow {
                let hue = (dist * 0.002 - t * 0.5).rem_euclid(1.0);
                let (r, g, b) = hsv_to_rgb(hue, 1.0, 1.0);
                (
                    r * intensity * brightness,
                    g * intensity * brightness,
                    b * intensity * brightness,
                )
            } else {
                (
                    base_rgb.0 * intensity * brightness,
                    base_rgb.1 * intensity * brightness,
                    base_rgb.2 * intensity * brightness,
                )
            }
        }
        9 => {
            let sweep_x = (t * 400.0).rem_euclid(988.0) - 50.0;
            let dist = (key.x + key.width / 2.0 - sweep_x).abs();
            let beam = (-dist * 0.03).exp();
            let intensity = (beam + 0.06).clamp(0.0, 1.0);
            if p.lighting.rainbow {
                let hue = (key.x * 0.001 + t * 0.2).rem_euclid(1.0);
                let (r, g, b) = hsv_to_rgb(hue, 0.9, 1.0);
                (
                    r * intensity * brightness,
                    g * intensity * brightness,
                    b * intensity * brightness,
                )
            } else {
                (
                    base_rgb.0 * intensity * brightness,
                    base_rgb.1 * intensity * brightness,
                    base_rgb.2 * intensity * brightness,
                )
            }
        }
        10 => {
            let phase = ((key.id as f64 * 12.9898 + key.x * 78.233).sin() * 43758.5453).fract();
            let speed_mult = 0.8 + 1.2 * phase;
            let twinkle = ((t * 2.0 * speed_mult + phase * std::f64::consts::TAU).sin() * 0.5
                + 0.5)
                .powf(3.0);
            let intensity = twinkle.clamp(0.05, 1.0);
            if p.lighting.rainbow {
                let (r, g, b) = hsv_to_rgb(phase, 0.9, 1.0);
                (
                    r * intensity * brightness,
                    g * intensity * brightness,
                    b * intensity * brightness,
                )
            } else {
                (
                    base_rgb.0 * intensity * brightness,
                    base_rgb.1 * intensity * brightness,
                    base_rgb.2 * intensity * brightness,
                )
            }
        }
        11 => {
            let dx = key.x + key.width / 2.0 - 444.0;
            let dy = key.y + key.height / 2.0 - 150.0;
            let angle = dy.atan2(dx);
            let dist = (dx * dx + dy * dy).sqrt();
            let petals = (angle * 4.0 + dist * 0.015 - t * 2.5).sin() * 0.5 + 0.5;
            let hue = (angle / std::f64::consts::TAU + dist * 0.001 - t * 0.2).rem_euclid(1.0);
            let (r, g, b) = hsv_to_rgb(hue, 1.0, 1.0);
            let intensity = petals * 0.7 + 0.3;
            (
                r * intensity * brightness,
                g * intensity * brightness,
                b * intensity * brightness,
            )
        }
        12 => {
            let hue = (key.x / 450.0 * dir - t * 0.7).rem_euclid(1.0);
            let (r, g, b) = hsv_to_rgb(hue, 1.0, 1.0);
            (r * brightness, g * brightness, b * brightness)
        }
        13 => {
            let hue = ((key.x + key.y * 1.6) / 520.0 - t * 0.75).rem_euclid(1.0);
            let (r, g, b) = hsv_to_rgb(hue, 1.0, 1.0);
            (r * brightness, g * brightness, b * brightness)
        }
        14 => {
            let block = ((key.x / 140.0).floor() - (t * 2.5).floor()).rem_euclid(5.0);
            let intensity = if block < 2.0 { 1.0 } else { 0.1 };
            if p.lighting.rainbow {
                let hue =
                    ((key.x / 140.0).floor() * 0.2 + (t * 0.5).floor() * 0.15).rem_euclid(1.0);
                let (r, g, b) = hsv_to_rgb(hue, 1.0, 1.0);
                (
                    r * intensity * brightness,
                    g * intensity * brightness,
                    b * intensity * brightness,
                )
            } else {
                (
                    base_rgb.0 * intensity * brightness,
                    base_rgb.1 * intensity * brightness,
                    base_rgb.2 * intensity * brightness,
                )
            }
        }
        15 => {
            let col = (key.x / 55.0).floor();
            let seed = ((col * 93.17).sin() * 43758.5).fract().abs();
            let drop_y = (t * 220.0 * (0.8 + 0.4 * seed) + seed * 300.0).rem_euclid(380.0);
            let dist_y = key.y - (drop_y - 40.0);
            let intensity = if (0.0..120.0).contains(&dist_y) {
                (1.0 - dist_y / 120.0).powf(1.4)
            } else {
                0.04
            };
            if p.lighting.rainbow {
                let (r, g, b) = hsv_to_rgb(seed, 0.9, 1.0);
                (
                    r * intensity * brightness,
                    g * intensity * brightness,
                    b * intensity * brightness,
                )
            } else {
                (
                    base_rgb.0 * intensity * brightness,
                    base_rgb.1 * intensity * brightness,
                    base_rgb.2 * intensity * brightness,
                )
            }
        }
        16 => {
            let dx = key.x + key.width / 2.0 - 444.0;
            let dy = key.y + key.height / 2.0 - 150.0;
            let angle = dy.atan2(dx).rem_euclid(std::f64::consts::TAU);
            let sweep = (t * 3.5).rem_euclid(std::f64::consts::TAU);
            let diff = (angle - sweep).rem_euclid(std::f64::consts::TAU);
            let intensity = if diff < 1.2 {
                (1.0 - diff / 1.2).powf(1.5)
            } else {
                0.06
            };
            if p.lighting.rainbow {
                let hue = (angle / std::f64::consts::TAU).rem_euclid(1.0);
                let (r, g, b) = hsv_to_rgb(hue, 1.0, 1.0);
                (
                    r * intensity * brightness,
                    g * intensity * brightness,
                    b * intensity * brightness,
                )
            } else {
                (
                    base_rgb.0 * intensity * brightness,
                    base_rgb.1 * intensity * brightness,
                    base_rgb.2 * intensity * brightness,
                )
            }
        }
        17 => {
            let preset = p.lighting.surmount_preset;
            let is_highlighted = match preset {
                0 => matches!(
                    key.label.as_str(),
                    "W" | "A" | "S" | "D" | "↑" | "↓" | "←" | "→" | "Esc"
                ),
                1 => matches!(
                    key.label.as_str(),
                    "W" | "A" | "S" | "D" | "1" | "2" | "3" | "4" | "Shift" | "Ctrl" | "Space"
                ),
                2 => matches!(
                    key.label.as_str(),
                    "Q" | "W"
                        | "E"
                        | "R"
                        | "D"
                        | "F"
                        | "1"
                        | "2"
                        | "3"
                        | "4"
                        | "5"
                        | "6"
                        | "Space"
                        | "B"
                ),
                _ => matches!(
                    key.label.as_str(),
                    "W" | "A" | "S" | "D" | "Q" | "E" | "R" | "F" | "Shift" | "Space"
                ),
            };
            if is_highlighted {
                (1.0 * brightness, 0.2 * brightness, 0.3 * brightness)
            } else {
                (0.12 * brightness, 0.12 * brightness, 0.15 * brightness)
            }
        }
        18 => {
            let pulse = ((t * 4.0 * dir).sin() * 0.5 + 0.5) * 888.0;
            let intensity =
                if (key.x <= pulse && dir > 0.0) || (key.x >= 888.0 - pulse && dir < 0.0) {
                    1.0
                } else {
                    0.08
                };
            let hue = (key.x / 888.0 * 0.35).rem_euclid(1.0);
            let (r, g, b) = hsv_to_rgb(hue, 1.0, 1.0);
            (
                r * intensity * brightness,
                g * intensity * brightness,
                b * intensity * brightness,
            )
        }
        _ => (
            base_rgb.0 * brightness,
            base_rgb.1 * brightness,
            base_rgb.2 * brightness,
        ),
    }
}

pub fn calculate_key_argb(key: &model::Key, p: &model::Profile, elapsed: f64) -> u32 {
    let (r, g, b) = calculate_key_color(key, p, elapsed);
    let r_byte = (r.clamp(0.0, 1.0) * 255.0).round() as u32;
    let g_byte = (g.clamp(0.0, 1.0) * 255.0).round() as u32;
    let b_byte = (b.clamp(0.0, 1.0) * 255.0).round() as u32;
    0xFF000000 | (r_byte << 16) | (g_byte << 8) | b_byte
}
