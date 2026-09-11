#![forbid(unsafe_code)]
mod device;
mod model;
mod playback;
mod protocol;
mod ui;

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        let result = (|| -> anyhow::Result<()> {
            match args[1].as_str() {
                "--probe" => {
                    let state = device::Keyboard::open()?.snapshot()?;
                    println!("{}", serde_json::to_string_pretty(&state)?);
                }
                "--test-lighting" => {
                    device::Keyboard::open()?.test_lighting_roundtrip()?;
                    println!("PASS: brightness changed, read back, restored, and verified.");
                }
                "--test-custom" => {
                    device::Keyboard::open()?.test_custom_roundtrip()?;
                    println!(
                        "PASS: custom layout activated, all programmable keys read back, original colors and settings restored."
                    );
                }
                "--restore-settings" => {
                    let path = args
                        .get(2)
                        .ok_or_else(|| anyhow::anyhow!("Supply a snapshot JSON file"))?;
                    let state = serde_json::from_slice(&std::fs::read(path)?)?;
                    device::Keyboard::open()?.restore_settings(&state)?;
                    println!("Settings restored and verified.");
                }
                "--help" => println!(
                    "Phantom Studio\n  --probe                 Read device information and settings\n  --test-lighting         Change brightness briefly, verify, restore\n  --test-custom           Test custom activation and all keys, then restore\n  --restore-settings FILE Restore a saved settings snapshot\n  No arguments            Open the GTK app"
                ),
                other => anyhow::bail!("Unknown option {other}. Use --help."),
            }
            Ok(())
        })();
        return match result {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("{e:#}");
                std::process::ExitCode::FAILURE
            }
        };
    }
    let code = ui::run();
    std::process::ExitCode::from(u8::from(code))
}
