use crate::model::{self, Macro, MacroEvent};
use anyhow::{Context, Result, bail, ensure};
#[cfg(target_os = "linux")]
use evdev::{AttributeSet, EventType, InputEvent, KeyCode, uinput::VirtualDevice};
use std::{
    collections::BTreeSet,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

pub enum Progress {
    Countdown(u32),
    Playing(u32, u32),
    Finished(Result<bool, String>),
}

#[derive(Default)]
pub struct Player {
    running: Arc<AtomicBool>,
    cancel: Option<mpsc::Sender<()>>,
    worker: Option<thread::JoinHandle<()>>,
}

impl Player {
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Acquire)
    }
    pub fn stop(&self) {
        if let Some(tx) = &self.cancel {
            let _ = tx.send(());
        }
    }
    pub fn shutdown(&mut self) {
        self.stop();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
    pub fn start(
        &mut self,
        mac: Macro,
        repeats: u32,
        countdown: u32,
    ) -> Result<mpsc::Receiver<Progress>> {
        ensure!(!self.is_running(), "A macro is already playing");
        model::encode_macros(std::slice::from_ref(&mac))?;
        ensure!(
            (1..=100).contains(&repeats) && (1..=10).contains(&countdown),
            "Invalid playback timing"
        );
        for event in &mac.events {
            linux_key(event.usage)?;
        }
        self.shutdown();
        let (cancel_tx, cancel_rx) = mpsc::channel();
        let (tx, rx) = mpsc::channel();
        self.cancel = Some(cancel_tx);
        self.running.store(true, Ordering::Release);
        #[cfg(target_os = "linux")]
        {
            let running = self.running.clone();
            self.worker = Some(thread::spawn(move || {
                let result = (|| -> Result<bool> {
                    let mut device = virtual_keyboard()?;
                    for seconds in (1..=countdown).rev() {
                        let _ = tx.send(Progress::Countdown(seconds));
                        if interrupted(&cancel_rx, Duration::from_secs(1)) {
                            return Ok(false);
                        }
                    }
                    run_sequence(
                        &mac,
                        repeats,
                        |code, down| {
                            device
                                .emit(&[InputEvent::new(EventType::KEY.0, code, i32::from(down))])
                                .map_err(Into::into)
                        },
                        |delay| interrupted(&cancel_rx, delay),
                        |round| {
                            let _ = tx.send(Progress::Playing(round, repeats));
                        },
                    )
                })()
                .map_err(|e| format!("{e:#}"));
                running.store(false, Ordering::Release);
                let _ = tx.send(Progress::Finished(result));
            }));
            Ok(rx)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (mac, repeats, countdown, tx, cancel_rx);
            self.running.store(false, Ordering::Release);
            bail!("Macro playback on desktop requires Linux uinput");
        }
    }
}
impl Drop for Player {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn interrupted(cancel: &mpsc::Receiver<()>, delay: Duration) -> bool {
    !matches!(
        cancel.recv_timeout(delay),
        Err(mpsc::RecvTimeoutError::Timeout)
    )
}

#[cfg(target_os = "linux")]
fn virtual_keyboard() -> Result<VirtualDevice> {
    let mut keys = AttributeSet::<KeyCode>::new();
    for usage in 4..=231 {
        if let Ok(code) = linux_key(usage) {
            keys.insert(KeyCode::new(code));
        }
    }
    VirtualDevice::builder()
        .context(
            "Macro playback needs access to /dev/uinput. Run ./install.sh --repair to configure it",
        )?
        .name("Phantom Studio Macro Playback")
        .with_keys(&keys)?
        .build()
        .context("Could not create the virtual macro keyboard")
}

fn run_sequence(
    mac: &Macro,
    repeats: u32,
    mut emit: impl FnMut(u16, bool) -> Result<()>,
    mut wait: impl FnMut(Duration) -> bool,
    mut progress: impl FnMut(u32),
) -> Result<bool> {
    let mut held = BTreeSet::new();
    let result = (|| -> Result<bool> {
        for round in 1..=repeats {
            progress(round);
            for event in &mac.events {
                if wait(Duration::from_millis(event.delay_ms.into())) {
                    return Ok(false);
                }
                let code = linux_key(event.usage)?;
                if event.down {
                    held.insert(code);
                }
                emit(code, event.down)?;
                if !event.down {
                    held.remove(&code);
                }
            }
        }
        Ok(true)
    })();
    let mut cleanup_error = None;
    for code in held {
        if let Err(error) = emit(code, false) {
            cleanup_error.get_or_insert(error);
        }
    }
    if let Some(error) = cleanup_error {
        return Err(error.context("Releasing macro keys failed"));
    }
    result
}

fn linux_key(usage: u8) -> Result<u16> {
    let key = match usage {
        4..=29 => [
            30, 48, 46, 32, 18, 33, 34, 35, 23, 36, 37, 38, 50, 49, 24, 25, 16, 19, 31, 20, 22, 47,
            17, 45, 21, 44,
        ][usage as usize - 4],
        30..=39 => usage as u16 - 28,
        40..=57 => [
            28, 1, 14, 15, 57, 12, 13, 26, 27, 43, 86, 39, 40, 41, 51, 52, 53, 58,
        ][usage as usize - 40],
        58..=67 => usage as u16 + 1,
        68..=69 => usage as u16 + 19,
        70..=82 => [
            99, 70, 119, 110, 102, 104, 111, 107, 109, 106, 105, 108, 103,
        ][usage as usize - 70],
        83..=99 => [
            69, 98, 55, 74, 78, 96, 79, 80, 81, 75, 76, 77, 71, 72, 73, 82, 83,
        ][usage as usize - 83],
        100 => 86,
        101 => 127,
        104..=115 => usage as u16 + 79,
        224..=231 => [29, 42, 56, 125, 97, 54, 100, 126][usage as usize - 224],
        _ => bail!("USB key usage {usage} is not supported for desktop playback"),
    };
    Ok(key)
}

pub fn linux_to_hid(code: u16) -> Option<u8> {
    match code {
        30 => Some(4),  // A
        48 => Some(5),  // B
        46 => Some(6),  // C
        32 => Some(7),  // D
        18 => Some(8),  // E
        33 => Some(9),  // F
        34 => Some(10), // G
        35 => Some(11), // H
        23 => Some(12), // I
        36 => Some(13), // J
        37 => Some(14), // K
        38 => Some(15), // L
        50 => Some(16), // M
        49 => Some(17), // N
        24 => Some(18), // O
        25 => Some(19), // P
        16 => Some(20), // Q
        19 => Some(21), // R
        31 => Some(22), // S
        20 => Some(23), // T
        22 => Some(24), // U
        47 => Some(25), // V
        17 => Some(26), // W
        45 => Some(27), // X
        21 => Some(28), // Y
        44 => Some(29), // Z

        2..=10 => Some(code as u8 + 28), // 1..9 -> 30..38
        11 => Some(39),                  // 0 -> 39

        28 => Some(40), // Enter
        1 => Some(41),  // Esc
        14 => Some(42), // Backspace
        15 => Some(43), // Tab
        57 => Some(44), // Space

        12 => Some(45),  // Minus
        13 => Some(46),  // Equal
        26 => Some(47),  // Left bracket
        27 => Some(48),  // Right bracket
        43 => Some(49),  // Backslash
        86 => Some(100), // 102nd
        39 => Some(51),  // Semicolon
        40 => Some(52),  // Apostrophe
        41 => Some(53),  // Grave
        51 => Some(54),  // Comma
        52 => Some(55),  // Dot
        53 => Some(56),  // Slash
        58 => Some(57),  // CapsLock

        59..=68 => Some(code as u8 - 1), // F1..F10 -> 58..67
        87 => Some(68),                  // F11
        88 => Some(69),                  // F12

        99 => Some(70),  // PrintScreen
        70 => Some(71),  // ScrollLock
        119 => Some(72), // Pause

        110 => Some(73), // Insert
        102 => Some(74), // Home
        104 => Some(75), // PageUp
        111 => Some(76), // Delete
        107 => Some(77), // End
        109 => Some(78), // PageDown
        106 => Some(79), // Right
        105 => Some(80), // Left
        108 => Some(81), // Down
        103 => Some(82), // Up

        69 => Some(83),   // NumLock
        98 => Some(84),   // KP /
        55 => Some(85),   // KP *
        74 => Some(86),   // KP -
        78 => Some(87),   // KP +
        96 => Some(88),   // KP Enter
        79 => Some(89),   // KP 1
        80 => Some(90),   // KP 2
        81 => Some(91),   // KP 3
        75 => Some(92),   // KP 4
        76 => Some(93),   // KP 5
        77 => Some(94),   // KP 6
        71 => Some(95),   // KP 7
        72 => Some(96),   // KP 8
        73 => Some(97),   // KP 9
        82 => Some(98),   // KP 0
        83 => Some(99),   // KP .
        127 => Some(101), // Compose

        183..=194 => Some((code - 79) as u8), // F13..F24

        29 => Some(224),  // Left Ctrl
        42 => Some(225),  // Left Shift
        56 => Some(226),  // Left Alt
        125 => Some(227), // Left Super/Win
        97 => Some(228),  // Right Ctrl
        54 => Some(229),  // Right Shift
        100 => Some(230), // Right Alt
        126 => Some(231), // Right Super/Win

        _ => None,
    }
}

pub struct BackgroundRecorder {
    running: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

impl BackgroundRecorder {
    #[allow(dead_code)]
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Acquire)
    }

    pub fn stop(&mut self) {
        self.running.store(false, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }

    pub fn start() -> Result<(mpsc::Receiver<MacroEvent>, Self)> {
        #[cfg(target_os = "linux")]
        {
            let mut devices = Vec::new();
            for (path, dev) in evdev::enumerate() {
                let name = dev.name().unwrap_or("").to_string();
                if name.contains("Phantom Studio") {
                    continue;
                }
                let is_kb = dev
                    .supported_keys()
                    .map(|k| k.contains(KeyCode::KEY_A) && k.contains(KeyCode::KEY_SPACE))
                    .unwrap_or(false);
                if !is_kb {
                    continue;
                }
                let lower = name.to_ascii_lowercase();
                if lower.contains("mouse") || lower.contains("touchpad") || lower.contains("pen") {
                    continue;
                }
                if dev.set_nonblocking(true).is_ok() {
                    devices.push((path, dev));
                }
            }

            ensure!(
                !devices.is_empty(),
                "No accessible keyboard input devices found in /dev/input"
            );

            let (tx, rx) = mpsc::channel();
            let running = Arc::new(AtomicBool::new(true));
            let running_clone = running.clone();

            let worker = thread::spawn(move || {
                let mut last_event_time = std::time::Instant::now();
                let mut held_usages = BTreeSet::new();

                while running_clone.load(Ordering::Acquire) {
                    let mut had_events = false;
                    let now = std::time::Instant::now();

                    for (_path, dev) in &mut devices {
                        match dev.fetch_events() {
                            Ok(events) => {
                                for ev in events {
                                    if ev.event_type() == EventType::KEY {
                                        let val = ev.value();
                                        if val == 0 || val == 1 {
                                            let down = val == 1;
                                            if let Some(usage) = linux_to_hid(ev.code()) {
                                                had_events = true;
                                                if down {
                                                    if held_usages.insert(usage) {
                                                        let delay = now
                                                            .duration_since(last_event_time)
                                                            .as_millis()
                                                            .min(65535)
                                                            as u16;
                                                        last_event_time = now;
                                                        let _ = tx.send(MacroEvent {
                                                            delay_ms: delay,
                                                            usage,
                                                            down: true,
                                                        });
                                                    }
                                                } else if held_usages.remove(&usage) {
                                                    let delay = now
                                                        .duration_since(last_event_time)
                                                        .as_millis()
                                                        .min(65535)
                                                        as u16;
                                                    last_event_time = now;
                                                    let _ = tx.send(MacroEvent {
                                                        delay_ms: delay,
                                                        usage,
                                                        down: false,
                                                    });
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                            Err(_) => {}
                        }
                    }

                    if !had_events {
                        thread::sleep(Duration::from_millis(5));
                    }
                }

                for usage in held_usages {
                    let _ = tx.send(MacroEvent {
                        delay_ms: 50,
                        usage,
                        down: false,
                    });
                }
            });

            Ok((
                rx,
                Self {
                    running,
                    worker: Some(worker),
                },
            ))
        }
        #[cfg(not(target_os = "linux"))]
        {
            bail!("Background recording on desktop requires Linux evdev");
        }
    }
}

impl Drop for BackgroundRecorder {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::MacroEvent;
    fn chord() -> Macro {
        Macro {
            name: "Ctrl+A".into(),
            events: [(224, true), (4, true), (4, false), (224, false)]
                .into_iter()
                .map(|(usage, down)| MacroEvent {
                    usage,
                    down,
                    delay_ms: 10,
                })
                .collect(),
        }
    }
    #[test]
    fn translates_and_repeats_chords() {
        let mut output = vec![];
        assert!(
            run_sequence(
                &chord(),
                2,
                |k, d| {
                    output.push((k, d));
                    Ok(())
                },
                |_| false,
                |_| {}
            )
            .unwrap()
        );
        assert_eq!(
            output,
            [(29, true), (30, true), (30, false), (29, false)].repeat(2)
        );
        assert_eq!(linux_key(79).unwrap(), 106);
        assert!(linux_key(0).is_err());
    }
    #[test]
    fn cancellation_releases_held_keys() {
        let mut output = vec![];
        let mut waits = 0;
        assert!(
            !run_sequence(
                &chord(),
                1,
                |k, d| {
                    output.push((k, d));
                    Ok(())
                },
                |_| {
                    waits += 1;
                    waits == 3
                },
                |_| {}
            )
            .unwrap()
        );
        assert_eq!(output, [(29, true), (30, true), (29, false), (30, false)]);
    }
    #[test]
    fn emission_failure_still_releases_keys() {
        let mut output = vec![];
        let result = run_sequence(
            &chord(),
            1,
            |k, d| {
                output.push((k, d));
                if k == 30 && d {
                    bail!("injected failure");
                }
                Ok(())
            },
            |_| false,
            |_| {},
        );
        assert!(result.is_err());
        assert!(output.contains(&(29, false)) && output.contains(&(30, false)));
    }
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "Requires uinput and access to the newly created event node; grabs only the test device"]
    fn virtual_keyboard_roundtrip() {
        let mac = chord();
        let mut device = virtual_keyboard().unwrap();
        let path = device
            .enumerate_dev_nodes_blocking()
            .unwrap()
            .next()
            .unwrap()
            .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        let mut reader = loop {
            match evdev::Device::open(&path) {
                Ok(reader) => break reader,
                Err(_) if std::time::Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(20))
                }
                Err(error) => panic!("Opening the test device: {error}"),
            }
        };
        reader.grab().unwrap();
        reader.set_nonblocking(true).unwrap();
        run_sequence(
            &mac,
            1,
            |k, d| {
                device
                    .emit(&[InputEvent::new(EventType::KEY.0, k, i32::from(d))])
                    .map_err(Into::into)
            },
            |_| false,
            |_| {},
        )
        .unwrap();
        let mut output = vec![];
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while output.len() < 4 && std::time::Instant::now() < deadline {
            match reader.fetch_events() {
                Ok(events) => output.extend(
                    events
                        .filter(|e| e.event_type() == EventType::KEY)
                        .map(|e| (e.code(), e.value())),
                ),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10))
                }
                Err(e) => panic!("{e}"),
            }
        }
        assert_eq!(output, [(29, 1), (30, 1), (30, 0), (29, 0)]);
    }

    #[test]
    fn linux_to_hid_translates_common_keys() {
        assert_eq!(linux_to_hid(30), Some(4)); // A
        assert_eq!(linux_to_hid(17), Some(26)); // W
        assert_eq!(linux_to_hid(57), Some(44)); // Space
        assert_eq!(linux_to_hid(28), Some(40)); // Enter
        assert_eq!(linux_to_hid(1), Some(41)); // Esc
        assert_eq!(linux_to_hid(29), Some(224)); // Left Ctrl
        assert_eq!(linux_to_hid(42), Some(225)); // Left Shift
    }

    #[test]
    fn sanitize_events_fixes_held_and_duplicates() {
        use crate::model::sanitize_events;
        // Case 1: unclosed key down -> auto releases
        let raw = vec![MacroEvent {
            delay_ms: 0,
            usage: 4,
            down: true,
        }];
        let fixed = sanitize_events(&raw);
        assert_eq!(fixed.len(), 2);
        assert!(fixed[0].down && fixed[0].usage == 4);
        assert!(!fixed[1].down && fixed[1].usage == 4);

        // Case 2: duplicate down + orphan up -> cleaned
        let dirty = vec![
            MacroEvent {
                delay_ms: 0,
                usage: 5,
                down: false,
            }, // orphan up
            MacroEvent {
                delay_ms: 10,
                usage: 5,
                down: true,
            },
            MacroEvent {
                delay_ms: 20,
                usage: 5,
                down: true,
            }, // duplicate down
            MacroEvent {
                delay_ms: 30,
                usage: 5,
                down: false,
            },
        ];
        let cleaned = sanitize_events(&dirty);
        assert_eq!(cleaned.len(), 2);
        assert!(cleaned[0].down && cleaned[0].usage == 5);
        assert!(!cleaned[1].down && cleaned[1].usage == 5);

        // Validate that encode_macros accepts the sanitized output
        let mac = Macro {
            name: "Test".into(),
            events: cleaned,
        };
        assert!(crate::model::encode_macros(&[mac]).is_ok());
    }
}
