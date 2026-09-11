use crate::model::{self, Macro};
use anyhow::{Context, Result, bail, ensure};
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
}
