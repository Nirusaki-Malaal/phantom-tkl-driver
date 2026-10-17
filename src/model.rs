use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Key {
    pub id: usize,
    pub label: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub assignment: u32,
}
pub fn layout() -> Vec<Key> {
    serde_json::from_str(include_str!("../data/layout.json")).expect("bundled layout")
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Lighting {
    pub mode: u8,
    pub brightness: u8,
    pub speed: u8,
    pub direction: u8,
    pub rainbow: bool,
    pub color: [u8; 3],
    pub custom_slot: u8,
    #[serde(default)]
    pub surmount_preset: u8,
}
impl Default for Lighting {
    fn default() -> Self {
        Self {
            mode: 1,
            brightness: 3,
            speed: 2,
            direction: 0,
            rainbow: true,
            color: [161, 115, 255],
            custom_slot: 0,
            surmount_preset: 0,
        }
    }
}
pub const EFFECTS: &[(u8, &str)] = &[
    (1, "Corrugated"),
    (2, "Cloud"),
    (3, "Serpentine"),
    (4, "Spectrum"),
    (5, "Breathing"),
    (6, "Static"),
    (7, "Reactive"),
    (8, "Ripples"),
    (9, "Traverse"),
    (10, "Stars"),
    (11, "Flowers"),
    (12, "Roll"),
    (13, "Wave"),
    (14, "Cartoon"),
    (15, "Rain"),
    (16, "Scan"),
    (17, "Surmount"),
    (18, "Speed"),
    (20, "Custom"),
];

#[derive(Clone, Copy, Debug)]
pub struct EffectControls {
    pub color: bool,
    pub rainbow: bool,
    pub speed: bool,
    pub direction: bool,
}
pub fn effect_controls(mode: u8) -> EffectControls {
    let color = !matches!(mode, 4 | 11 | 12 | 17 | 18);
    EffectControls {
        color,
        rainbow: color && mode != 20,
        speed: !matches!(mode, 6 | 16 | 17 | 18 | 20),
        direction: matches!(mode, 1 | 2 | 3 | 12 | 18),
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MacroEvent {
    pub delay_ms: u16,
    pub usage: u8,
    pub down: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Macro {
    pub name: String,
    pub events: Vec<MacroEvent>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    pub lighting: Lighting,
    pub assignments: BTreeMap<usize, [u8; 3]>,
    pub colors: BTreeMap<usize, [u8; 3]>,
    pub macros: Vec<Macro>,
    pub win_lock: bool,
    pub six_key_rollover: bool,
    #[serde(default)]
    pub swap_wasd: bool,
    #[serde(default)]
    pub polling_rate: u8,
}
impl Default for Profile {
    fn default() -> Self {
        Self {
            name: "Everyday".into(),
            lighting: Lighting::default(),
            assignments: BTreeMap::new(),
            colors: BTreeMap::new(),
            macros: vec![],
            win_lock: false,
            six_key_rollover: false,
            swap_wasd: false,
            polling_rate: 0,
        }
    }
}
impl Profile {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.name.trim().is_empty() && self.name.len() <= 80,
            "Profile name must be 1–80 characters"
        );
        ensure!(
            EFFECTS.iter().any(|e| e.0 == self.lighting.mode),
            "Unknown firmware effect"
        );
        ensure!(
            self.lighting.brightness <= 4
                && self.lighting.speed <= 4
                && self.lighting.direction <= 4,
            "Lighting value outside device range"
        );
        ensure!(self.lighting.custom_slot < 10, "Custom slot must be 0–9");
        ensure!(
            self.lighting.surmount_preset < 4,
            "Surmount palette must be 0–3"
        );
        ensure!(self.macros.len() <= 250, "At most 250 macros");
        ensure!(
            self.polling_rate <= 3,
            "Polling rate must be one of the four device presets"
        );
        for id in self.assignments.keys().chain(self.colors.keys()) {
            ensure!(
                layout().iter().any(|k| k.id == *id),
                "Unknown physical key {id}"
            );
        }
        for assignment in self.assignments.values() {
            ensure!(
                matches!(assignment[0], 0x20 | 0x30 | 0x70),
                "Unsupported assignment type"
            );
            if assignment[0] == 0x70 {
                ensure!(
                    (assignment[1] as usize) < self.macros.len(),
                    "Assignment references a missing macro"
                );
            }
        }
        encode_macros(&self.macros)?;
        Ok(())
    }
}

pub fn encode_macros(macros: &[Macro]) -> Result<Vec<u8>> {
    ensure!(macros.len() <= 250, "At most 250 macros");
    let size = 16 + macros.len() * 6 + macros.iter().map(|m| m.events.len() * 4).sum::<usize>();
    ensure!(
        size <= 0x1c00,
        "Macro storage exceeds the keyboard's 7168-byte capacity"
    );
    let mut out = vec![0u8; size];
    out[..2].copy_from_slice(&0x55aau16.to_le_bytes());
    out[2..4].copy_from_slice(&(size as u16).to_le_bytes());
    out[4..6].copy_from_slice(&(macros.len() as u16).to_le_bytes());
    let mut offset = 16 + macros.len() * 2;
    for (i, mac) in macros.iter().enumerate() {
        ensure!(!mac.events.is_empty(), "Macro {} has no events", mac.name);
        let mut held = std::collections::BTreeSet::new();
        out[16 + i * 2..18 + i * 2].copy_from_slice(&(offset as u16).to_le_bytes());
        out[offset..offset + 2].copy_from_slice(&(mac.events.len() as u16).to_le_bytes());
        offset += 4;
        for event in &mac.events {
            ensure!(event.usage != 0, "Macro usage cannot be zero");
            if event.down {
                ensure!(
                    held.insert(event.usage),
                    "Duplicate key down in {}",
                    mac.name
                );
            } else {
                ensure!(
                    held.remove(&event.usage),
                    "Unmatched key up in {}",
                    mac.name
                );
            }
            out[offset..offset + 2].copy_from_slice(&event.delay_ms.to_le_bytes());
            let modifier = (0xe0..=0xe7).contains(&event.usage);
            out[offset + 2] = (if modifier { 9 } else { 10 }) | if event.down { 0x80 } else { 0 };
            out[offset + 3] = if modifier {
                1 << (event.usage - 0xe0)
            } else {
                event.usage
            };
            offset += 4;
        }
        ensure!(held.is_empty(), "Macro {} leaves a key held", mac.name);
    }
    Ok(out)
}

pub fn sanitize_events(events: &[MacroEvent]) -> Vec<MacroEvent> {
    let mut out = Vec::with_capacity(events.len() + 4);
    let mut held = std::collections::BTreeSet::new();

    for ev in events {
        if ev.usage == 0 {
            continue;
        }
        if ev.down {
            if held.insert(ev.usage) {
                out.push(ev.clone());
            }
        } else if held.remove(&ev.usage) {
            out.push(ev.clone());
        }
    }

    // Auto-release any keys that remain held at the end of the sequence
    for usage in held {
        out.push(MacroEvent {
            usage,
            down: false,
            delay_ms: 50,
        });
    }

    out
}

pub fn usage_name(usage: u8) -> &'static str {
    match usage {
        4 => "A",
        5 => "B",
        6 => "C",
        7 => "D",
        8 => "E",
        9 => "F",
        10 => "G",
        11 => "H",
        12 => "I",
        13 => "J",
        14 => "K",
        15 => "L",
        16 => "M",
        17 => "N",
        18 => "O",
        19 => "P",
        20 => "Q",
        21 => "R",
        22 => "S",
        23 => "T",
        24 => "U",
        25 => "V",
        26 => "W",
        27 => "X",
        28 => "Y",
        29 => "Z",
        30 => "1",
        31 => "2",
        32 => "3",
        33 => "4",
        34 => "5",
        35 => "6",
        36 => "7",
        37 => "8",
        38 => "9",
        39 => "0",
        40 => "Enter",
        41 => "Esc",
        42 => "Backspace",
        43 => "Tab",
        44 => "Space",
        45 => "-",
        46 => "=",
        47 => "[",
        48 => "]",
        49 => "\\",
        50 => "\\",
        51 => ";",
        52 => "'",
        53 => "`",
        54 => ",",
        55 => ".",
        56 => "/",
        57 => "CapsLock",
        58 => "F1",
        59 => "F2",
        60 => "F3",
        61 => "F4",
        62 => "F5",
        63 => "F6",
        64 => "F7",
        65 => "F8",
        66 => "F9",
        67 => "F10",
        68 => "F11",
        69 => "F12",
        70 => "PrintScreen",
        71 => "ScrollLock",
        72 => "Pause",
        73 => "Insert",
        74 => "Home",
        75 => "PageUp",
        76 => "Delete",
        77 => "End",
        78 => "PageDown",
        79 => "Right",
        80 => "Left",
        81 => "Down",
        82 => "Up",
        83 => "NumLock",
        84 => "KP /",
        85 => "KP *",
        86 => "KP -",
        87 => "KP +",
        88 => "KP Enter",
        89 => "KP 1",
        90 => "KP 2",
        91 => "KP 3",
        92 => "KP 4",
        93 => "KP 5",
        94 => "KP 6",
        95 => "KP 7",
        96 => "KP 8",
        97 => "KP 9",
        98 => "KP 0",
        99 => "KP .",
        100 => "102nd",
        101 => "Compose",
        104 => "F13",
        105 => "F14",
        106 => "F15",
        107 => "F16",
        108 => "F17",
        109 => "F18",
        110 => "F19",
        111 => "F20",
        112 => "F21",
        113 => "F22",
        114 => "F23",
        115 => "F24",
        224 => "Ctrl",
        225 => "Shift",
        226 => "Alt",
        227 => "Win",
        228 => "Right Ctrl",
        229 => "Right Shift",
        230 => "Right Alt",
        231 => "Right Win",
        _ => "Key",
    }
}

pub fn event_summary(events: &[MacroEvent]) -> String {
    if events.is_empty() {
        return "Empty macro".into();
    }
    let total_ms: u32 = events.iter().map(|e| e.delay_ms as u32).sum();
    let mut pressed = Vec::new();
    for e in events {
        if e.down {
            let name = usage_name(e.usage);
            if !pressed.contains(&name) && pressed.len() < 5 {
                pressed.push(name);
            }
        }
    }
    let keys_str = pressed.join(" + ");
    if keys_str.is_empty() {
        format!("{} actions · {}ms", events.len(), total_ms)
    } else {
        format!("{} actions · {}ms · [{}]", events.len(), total_ms, keys_str)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Library {
    pub version: u32,
    pub selected: usize,
    pub profiles: Vec<Profile>,
}
impl Default for Library {
    fn default() -> Self {
        Self {
            version: 1,
            selected: 0,
            profiles: vec![Profile::default()],
        }
    }
}
pub fn config_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("PHANTOM_CONFIG_DIR") {
        return PathBuf::from(dir);
    }
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")
        })
        .join("phantom-studio")
}
pub fn atomic_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let parent = path.parent().context("No parent directory")?;
    fs::create_dir_all(parent)?;
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    let bytes = serde_json::to_vec_pretty(value)?;
    {
        use std::io::Write;
        let mut file = fs::File::create(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
    }
    fs::rename(temporary, path)?;
    Ok(())
}

pub fn import_profile(path: &Path) -> Result<Profile> {
    use std::io::Read;
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 4 * 1024 * 1024, "Profile exceeds 4 MB");
    let value: serde_json::Value = serde_json::from_slice(&bytes)?;
    if value.get("Device").is_none() {
        let p: Profile = serde_json::from_value(value)?;
        p.validate()?;
        return Ok(p);
    }
    let devices = value["Device"]
        .as_array()
        .context("Windows profile has no devices")?;
    let device = devices
        .iter()
        .find(|d| d["DeviceBasicInfo"]["DeviceType"] == "keyboard")
        .context("Windows profile has no keyboard")?;
    let light = &device["LightInfo"];
    let integer = |name: &str, default: u8| -> Result<u8> {
        match light.get(name) {
            None | Some(serde_json::Value::Null) => Ok(default),
            Some(v) => {
                let n = v.as_u64().context(format!("Invalid {name}"))?;
                Ok(u8::try_from(n)?)
            }
        }
    };
    let name = path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .chars()
        .take(80)
        .collect();
    let mut p = Profile {
        name,
        ..Profile::default()
    };
    let selected = integer("SelectItem", 0)? as usize;
    p.lighting.mode = EFFECTS
        .get(selected)
        .context("Unsupported Windows effect selection")?
        .0;
    p.lighting.brightness = integer("Light", 4)?;
    p.lighting.speed = integer("Speed", 2)?;
    p.lighting.direction = integer("Fx", 0)?;
    p.lighting.rainbow = light["MultiColor"].as_bool().unwrap_or(true);
    p.lighting.color = [
        integer("Red", 255)?,
        integer("Green", 255)?,
        integer("Blue", 255)?,
    ];
    p.lighting.custom_slot = integer("CustomGroup", 0)?;
    p.lighting.surmount_preset = integer("ZZCCLedMode", 0)?;
    p.win_lock = device["SystemStages"]["WinFlag"].as_u64().unwrap_or(0) != 0;
    p.six_key_rollover = device["SystemStages"]["Key6Flag"].as_u64().unwrap_or(0) != 0;
    p.swap_wasd = device["SystemStages"]["WFlag"].as_u64().unwrap_or(0) != 0;
    p.polling_rate = u8::try_from(device["SystemStages"]["PollingRate"].as_u64().unwrap_or(0))?;
    for key in layout() {
        if key.id == 110 {
            continue;
        }
        if let Some(a) = device["KeyList"][key.id]["Assignment"].as_u64() {
            let a = u32::try_from(a)?;
            if a != key.assignment {
                let b = a.to_be_bytes();
                ensure!(
                    matches!(b[1], 0x20 | 0x30),
                    "Windows key {} uses an unsupported macro or launch action; import its macro separately",
                    key.label
                );
                p.assignments.insert(key.id, [b[1], b[2], b[3]]);
            }
        }
        if let Some(c) = device["CustomLightMode"]["LightColorInfo"]
            [p.lighting.custom_slot as usize][key.id]
            .as_object()
        {
            let component = |name: &str| -> Result<u8> {
                Ok(u8::try_from(
                    c.get(name)
                        .and_then(|v| v.as_u64())
                        .context("Invalid custom color")?,
                )?)
            };
            p.colors.insert(
                key.id,
                [component("Red")?, component("Green")?, component("Blue")?],
            );
        }
    }
    p.validate()?;
    Ok(p)
}
impl Library {
    pub fn load() -> Result<Self> {
        let path = config_dir().join("profiles.json");
        if !path.exists() {
            return Ok(Self::default());
        }
        let lib: Self = serde_json::from_slice(&fs::read(path)?)?;
        ensure!(
            lib.version == 1 && !lib.profiles.is_empty() && lib.selected < lib.profiles.len(),
            "Invalid profile library"
        );
        for p in &lib.profiles {
            p.validate()?;
        }
        Ok(lib)
    }
    pub fn save(&self) -> Result<()> {
        for p in &self.profiles {
            p.validate()?;
        }
        atomic_json(&config_dir().join("profiles.json"), self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn macro_offsets_and_bounds() {
        let m = Macro {
            name: "A".into(),
            events: vec![
                MacroEvent {
                    delay_ms: 0,
                    usage: 4,
                    down: true,
                },
                MacroEvent {
                    delay_ms: 100,
                    usage: 4,
                    down: false,
                },
            ],
        };
        let b = encode_macros(std::slice::from_ref(&m)).unwrap();
        assert_eq!(&b[..6], &[0xaa, 0x55, 30, 0, 1, 0]);
        assert_eq!(&b[16..], &[18, 0, 2, 0, 0, 0, 0, 0, 0x8a, 4, 100, 0, 10, 4]);
        let mut bad = m.clone();
        bad.events.pop();
        assert!(encode_macros(&[bad]).is_err());
        assert!(encode_macros(&vec![m; 251]).is_err());
    }
}
