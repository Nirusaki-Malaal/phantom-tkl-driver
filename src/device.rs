use crate::{
    model::{self, Profile},
    protocol,
};
use anyhow::{Context, Result, bail, ensure};
use hidapi::{HidApi, HidDevice};
use serde::{Deserialize, Serialize};
use std::{
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Snapshot {
    pub info: Vec<u8>,
    pub settings: Vec<u8>,
    pub keys: Vec<u8>,
    pub leds: Vec<u8>,
    #[serde(default)]
    pub custom_colors: Vec<u8>,
}
impl Snapshot {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.info.len() == 34 && self.info[..2] == [0xaa, 0x55],
            "Unknown device information format"
        );
        let count = self.info[5] as usize;
        ensure!((1..=170).contains(&count), "Unsupported key count");
        ensure!(
            self.settings.len() == 26 && self.keys.len() == count * 3 && self.leds.len() == count,
            "Incomplete device snapshot"
        );
        Ok(())
    }
    pub fn key_index(&self, key: &model::Key) -> Option<usize> {
        let a = key.assignment.to_be_bytes();
        self.keys
            .as_chunks::<3>()
            .0
            .iter()
            .position(|k| k == &a[1..])
    }
    pub fn led_index(&self, key: &model::Key) -> Option<usize> {
        let index = self.key_index(key)?;
        let led = *self.leds.get(index)? as usize;
        (led < self.info[5] as usize).then_some(led)
    }
    pub fn profile(&self) -> Profile {
        let mut p = Profile {
            name: "From keyboard".into(),
            ..Profile::default()
        };
        p.lighting.mode = if model::EFFECTS.iter().any(|e| e.0 == self.settings[1]) {
            self.settings[1]
        } else {
            1
        };
        p.lighting.brightness = self.settings[2].min(4);
        p.lighting.speed = 4 - self.settings[3].min(4);
        p.lighting.direction = self.settings[4].min(4);
        p.lighting.rainbow = self.settings[5] != 0;
        p.lighting.color.copy_from_slice(&self.settings[6..9]);
        p.lighting.custom_slot = self.settings[25].min(9);
        p.lighting.surmount_preset = self.settings[17].min(3);
        p.six_key_rollover = self.settings[20] != 0;
        p.win_lock = self.settings[21] != 0;
        p.swap_wasd = self.settings[19] != 0;
        p.polling_rate = self.settings[22].min(3);
        if p.lighting.mode == 20 {
            for key in model::layout() {
                if let Some(led) = self.led_index(&key)
                    && let Some(rgb) = self.custom_colors.get(led * 3..led * 3 + 3)
                {
                    p.colors.insert(key.id, [rgb[0], rgb[1], rgb[2]]);
                }
            }
        }
        p
    }
}
pub struct Keyboard {
    device: HidDevice,
    _lock: std::fs::File,
}
impl Keyboard {
    pub fn open() -> Result<Self> {
        let runtime = std::env::var_os("XDG_RUNTIME_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(model::config_dir);
        std::fs::create_dir_all(&runtime)?;
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(runtime.join("phantom-studio-hid.lock"))?;
        lock.try_lock().context(
            "Another Phantom Studio process is communicating with the keyboard; try again shortly",
        )?;
        let api = HidApi::new().context("Cannot enumerate HID devices")?;
        let devices: Vec<_> = api
            .device_list()
            .filter(|d| {
                d.vendor_id() == 0x320f
                    && d.product_id() == 0x505b
                    && d.usage_page() == 0xff1c
                    && d.usage() == 0x92
            })
            .collect();
        ensure!(
            devices.len() == 1,
            "Expected one Phantom TKL vendor interface; found {}. Connect the keyboard and install the supplied udev rule if needed.",
            devices.len()
        );
        Ok(Self{device:devices[0].open_device(&api).context("Cannot open keyboard. Install the supplied udev rule, then reconnect the keyboard.")?, _lock: lock})
    }
    fn exchange(&self, command: u8, length: usize, offset: usize, data: &[u8]) -> Result<Vec<u8>> {
        let tx = protocol::packet(command, length, offset, data)?;
        ensure!(self.device.write(&tx)? == 64, "Short HID write");
        let deadline = Instant::now() + Duration::from_millis(1500);
        while Instant::now() < deadline {
            let mut rx = [0; 64];
            let n = self.device.read_timeout(&mut rx, 100)?;
            if n == 0 {
                continue;
            }
            if rx[0] != 4 || rx[3] != command {
                continue;
            }
            protocol::validate_reply(&tx, &rx[..n])?;
            return Ok(rx[8..8 + length].to_vec());
        }
        bail!("Keyboard timed out on command {command:02x}")
    }
    fn read(&self, command: u8, length: usize, offset: usize) -> Result<Vec<u8>> {
        let mut result = Vec::with_capacity(length);
        for pos in (0..length).step_by(protocol::PAYLOAD) {
            result.extend(self.exchange(
                command,
                (length - pos).min(protocol::PAYLOAD),
                offset + pos,
                &[],
            )?);
        }
        Ok(result)
    }
    fn write(&self, command: u8, data: &[u8], offset: usize) -> Result<()> {
        for (i, chunk) in data.chunks(protocol::PAYLOAD).enumerate() {
            self.exchange(command, chunk.len(), offset + i * protocol::PAYLOAD, chunk)?;
        }
        Ok(())
    }
    fn session<T>(&self, f: impl FnOnce(&Self) -> Result<T>) -> Result<T> {
        self.exchange(1, 0, 0, &[])?;
        let result = f(self);
        thread::sleep(Duration::from_millis(10));
        let end = self.exchange(2, 0, 0, &[]);
        match (result, end) {
            (Ok(value), Ok(_)) => Ok(value),
            (Err(e), _) => Err(e),
            (_, Err(e)) => Err(e.context("Finishing keyboard transaction failed")),
        }
    }
    pub fn snapshot(&self) -> Result<Snapshot> {
        self.session(|dev| {
            let info = dev.read(3, 34, 0)?;
            ensure!(
                info[..2] == [0xaa, 0x55] && (1..=170).contains(&info[5]),
                "Unrecognized Phantom firmware"
            );
            let count = info[5] as usize;
            let settings = dev.read(5, 26, 0)?;
            let custom_colors = if settings[1] == 20 && settings[25] < 10 {
                dev.read(10, count * 3, settings[25] as usize * 0x200)?
            } else {
                vec![]
            };
            let state = Snapshot {
                info,
                settings,
                keys: dev.read(7, count * 3, 0)?,
                leds: dev.read(0x1b, count, 0)?,
                custom_colors,
            };
            state.validate()?;
            Ok(state)
        })
    }
    pub fn apply_lighting(&self, p: &Profile) -> Result<Snapshot> {
        p.validate()?;
        let before = self.snapshot()?;
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis();
        model::atomic_json(
            &model::config_dir().join(format!("backups/{stamp}.json")),
            &before,
        )?;
        let mut settings = before.settings.clone();
        // This edition uses hardware settings bank zero; profiles live on the host.
        // 004948e0 receives the bank separately from the selected effect.
        settings[0] = 0;
        settings[1] = p.lighting.mode;
        settings[2] = p.lighting.brightness;
        settings[3] = 4 - p.lighting.speed;
        settings[4] = p.lighting.direction.min(1);
        settings[5] = u8::from(p.lighting.rainbow);
        settings[6..9].copy_from_slice(&p.lighting.color);
        settings[25] = p.lighting.custom_slot;
        if p.lighting.mode == 17 {
            settings[17] = p.lighting.surmount_preset;
        }
        self.session(|dev| {
            if p.lighting.mode == 20 {
                let size = before.info[5] as usize * 3;
                let offset = p.lighting.custom_slot as usize * 0x200;
                let mut colors = dev.read(10, size, offset)?;
                model::atomic_json(
                    &model::config_dir().join(format!("backups/{stamp}-colors.json")),
                    &colors,
                )?;
                for key in model::layout() {
                    if let Some(color) = p.colors.get(&key.id) {
                        // Older profiles included PrtSc, which has no programmable
                        // entry on this device. It must not abort the whole painting.
                        let Some(led) = before.led_index(&key) else {
                            continue;
                        };
                        colors[led * 3..led * 3 + 3].copy_from_slice(color);
                    }
                }
                dev.write(11, &colors, offset)?;
                ensure!(
                    dev.read(10, size, offset)? == colors,
                    "Custom color readback differs"
                );
            }
            dev.write(6, &settings, 0)?;
            ensure!(dev.read(5, 26, 0)? == settings, "Lighting readback differs");
            Ok(())
        })?;
        self.snapshot()
    }
    pub fn apply_keys(&self, p: &Profile) -> Result<()> {
        p.validate()?;
        let state = self.snapshot()?;
        let mut keys = state.keys.clone();
        for key in model::layout() {
            if let Some(assignment) = p.assignments.get(&key.id) {
                let index = state
                    .key_index(&key)
                    .context("Physical key not in device map")?;
                keys[index * 3..index * 3 + 3].copy_from_slice(assignment);
            }
        }
        self.session(|dev| {
            if !p.macros.is_empty() {
                dev.write(0x15, &model::encode_macros(&p.macros)?, 0)?;
            }
            dev.write(9, &keys, 0)?;
            Ok(())
        })
    }
    pub fn apply_system(&self, p: &Profile) -> Result<Snapshot> {
        p.validate()?;
        let state = self.snapshot()?;
        let mut s = state.settings;
        s[0] = 0;
        s[20] = u8::from(p.six_key_rollover);
        s[21] = u8::from(p.win_lock);
        s[19] = u8::from(p.swap_wasd);
        s[22] = p.polling_rate;
        self.session(|dev| {
            dev.write(6, &s, 0)?;
            ensure!(dev.read(5, 26, 0)? == s, "Settings readback differs");
            Ok(())
        })?;
        self.snapshot()
    }
    pub fn restore_settings(&self, state: &Snapshot) -> Result<()> {
        state.validate()?;
        self.session(|dev| {
            dev.write(6, &state.settings, 0)?;
            ensure!(
                dev.read(5, 26, 0)? == state.settings,
                "Restore readback differs"
            );
            Ok(())
        })
    }
    pub fn test_lighting_roundtrip(&self) -> Result<()> {
        let before = self.snapshot()?;
        model::atomic_json(
            &model::config_dir().join("hardware-test-backup.json"),
            &before,
        )?;
        let mut s = before.settings.clone();
        s[2] = if s[2] == 0 { 1 } else { s[2] - 1 };
        let test = self.session(|dev| {
            dev.write(6, &s, 0)?;
            ensure!(dev.read(5, 26, 0)? == s, "Brightness readback failed");
            Ok(())
        });
        thread::sleep(Duration::from_millis(500));
        self.restore_settings(&before)
            .context("Restore failed; use hardware-test-backup.json")?;
        test
    }

    pub fn test_custom_roundtrip(&self) -> Result<()> {
        let state = self.snapshot()?;
        let size = state.info[5] as usize * 3;
        let before = self.session(|dev| dev.read(10, size, 0))?;
        model::atomic_json(
            &model::config_dir().join("custom-test-settings.json"),
            &state,
        )?;
        model::atomic_json(
            &model::config_dir().join("custom-test-colors.json"),
            &before,
        )?;
        let mut profile = state.profile();
        profile.lighting.mode = 20;
        profile.lighting.custom_slot = 0;
        // Include the formerly unsupported PrtSc entry to reproduce old profiles.
        for key in model::layout() {
            profile.colors.insert(key.id, [161, 115, 255]);
        }
        let test = (|| -> Result<()> {
            let after = self.apply_lighting(&profile)?;
            ensure!(
                after.settings[0] == 0 && after.settings[1] == 20,
                "Custom activation failed"
            );
            for key in model::layout() {
                if let Some(led) = after.led_index(&key) {
                    ensure!(
                        after.custom_colors[led * 3..led * 3 + 3] == [161, 115, 255],
                        "Custom key readback failed: {}",
                        key.label
                    );
                }
            }
            thread::sleep(Duration::from_millis(700));
            Ok(())
        })();
        let restore_colors = self.session(|dev| {
            dev.write(11, &before, 0)?;
            ensure!(
                dev.read(10, size, 0)? == before,
                "Custom restore readback differs"
            );
            Ok(())
        });
        let restore_settings = self.restore_settings(&state);
        restore_colors.context("Could not restore custom colors")?;
        restore_settings.context("Could not restore settings")?;
        test
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn physical_map_and_custom_readback() {
        let mut state: Snapshot =
            serde_json::from_str(include_str!("../tests/fixtures/phantom-320f-505b.json")).unwrap();
        state.settings[1] = 20;
        state.custom_colors = vec![0; state.info[5] as usize * 3];
        let layout = model::layout();
        let missing = layout.iter().find(|k| k.id == 13).unwrap();
        assert!(state.led_index(missing).is_none());
        let a = layout.iter().find(|k| k.label == "A").unwrap();
        let led = state.led_index(a).unwrap();
        state.custom_colors[led * 3..led * 3 + 3].copy_from_slice(&[12, 34, 56]);
        let profile = state.profile();
        assert_eq!(profile.colors.get(&a.id), Some(&[12, 34, 56]));
        assert!(!profile.colors.contains_key(&13));
    }
}
