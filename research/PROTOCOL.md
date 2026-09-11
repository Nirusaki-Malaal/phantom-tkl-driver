# Phantom TKL reverse-engineering notes

These notes distinguish reconstructed behavior from physical verification. The Linux application is Rust/GTK 4; the original executable and DLLs are not runtime dependencies.

## Inputs and method

Source folder: `the user-supplied Windows package`.

| Input | SHA-256 |
|---|---|
| Phantom TKL Software.exe | `09454bf15149841adb45521fb55c18fdf3523b69b6c7af2247b45be0bbd7a44d` |
| HookDLL.dll | `9ab5e1ccff616db6e9a7d571b1d932953abadf85a489194827aee8326e436b12` |
| DuiLib.dll | `35be785ac7b2d05828ef89d8744e5144723d8066b2c011ee0266d45dca676dc1` |
| SDK/x86/vista/osConfLib.dll | `b3997b03c2f842386af172cb96c2c63af4e5a69dfe07693646e5c23764a52127` |
| SDK/x64/vista/osConfLib.dll | `20c58530e75179529fc7af38141730cd77815251cb94f5dee296790a0d9ea3b6` |

GhidraMCP runs at `http://127.0.0.1:8081/sse`. The supplied bridge needs a separate Ghidra backend. `tools/HeadlessBridge.java` provides a read-only backend on localhost:8080; `tools/ghidra_mcp.py` issues actual MCP calls through the SSE bridge. Local files prefixed `mcp_` under `decompiled/` record returned decompilation. Raw exports and local capture files are intentionally excluded from Git; function addresses and the protocol fixture are retained for review. Address 00494700 lies within the function beginning at 00493c80. Other extracts came from an existing full Ghidra export whose executable hash matched the supplied input. DLL hashes were inventoried; this does not mean every DLL function has been reconstructed.

Save the analysis project before starting the HTTP adapter so it can be reopened without repeating analysis.

## Device and framing

The connected device is Evision RGB Keyboard, VID `320f`, PID `505b`, vendor HID usage page `ff1c`, usage `92`. These identifiers agree with the executable's device table (004028b0). The stale VID/PID in DefaultData/Keyboard.json are not used for discovery.

Use HID output reports and input replies, **not feature reports**. Each report is 64 bytes:

| Bytes | Meaning |
|---|---|
| 0 | Report ID `04` |
| 1–2 | Little-endian sum of request bytes 3–63 |
| 3 | Command |
| 4 | Payload length, at most 56 |
| 5–6 | Little-endian byte offset |
| 7 | Request zero / response status |
| 8–63 | Payload, remaining bytes zero-padded |

Replies echo the **request checksum**, even for reads whose response payload differs. Match report ID, command, length, offset and echoed checksum; reject status `fe` or `ff`, short replies and timeouts. See captured packets in `../tests/fixtures/phantom-320f-505b.json` and the Rust packet tests.

Transactions begin with command 01 and finish with 02. The original driver waits before finishing; the Linux implementation waits 10 ms. Transfers split into 56-byte chunks with increasing offsets. Relevant transport functions include 0047a340, 0047af70, 0047b180 and 0047cd70.

| Command | Reconstructed use |
|---|---|
| 03 | Read 34-byte device information; magic aa55; byte 5 is key-slot count |
| 05 / 06 | Read/write 26-byte settings record |
| 07 | Read default physical key triples |
| 09 | Write assignment triples |
| 0a / 0b | Read/write custom RGB slot |
| 15 | Write macro storage |
| 1b | Read physical-key to LED-index map |

This keyboard reports 128 slots: key and RGB blocks are 384 bytes, LED map is 128 bytes. A custom RGB slot has a **512-byte address stride**, despite transferring only 384 bytes. Sources: 004a1110, 0047c810 and 004971b0. The original UI exposes only the first custom slot; the Linux editor offers the ten slots handled by the original code. Storage roundtrips were tested for slots 1 and 10; physical custom lighting was confirmed for the editor's selected slot, not independently for all ten slots.

## Settings

Settings record offsets recovered from 004948e0 and checked against its assembly:

| Offset | Use |
|---|---|
| 0 | Hardware settings-bank selector, separately supplied to the writer |
| 1 | Firmware effect ID |
| 2 | Brightness 0–4 |
| 3 | Inverted UI speed: 4 minus slider value |
| 4 | Direction; two choices for supported effects |
| 5 | Spectrum/multicolor flag |
| 6–8 | Red, green, blue |
| 17 | Surmount palette 0–3 (`ZZCCLedMode`) |
| 19 | WASD/arrow swap |
| 20 | Six-key rollover |
| 21 | Windows-key lock |
| 22 | Polling index: 1000, 500, 250, 125 Hz |
| 25 | Custom slot 0–9 |

Unknown bytes are preserved. The generic original reader (00492cb0) reads the bank selector then a record at `bank * 26`. The normal lighting change handler (0045ca90, virtual call at offset 294 in the keyboard class) supplies bank zero. Linux profiles are host-side, and application writes now select bank zero and address offset zero. An earlier build incorrectly put the effect's menu index in byte zero. The correction matches the original call path and passes hardware readback. Runtime settings writes are restricted to record zero; the app does not follow selectors created by the earlier bug.

## Effect controls

The visible firmware IDs from `DefaultData/default_light.json` are 1–18 and 20. They are distinct from the menu index, which skips hidden entries. Control capabilities come from **004181f0**, dispatched for keyboard type 1 by 00418110:

| Effects | Controls omitted/disabled |
|---|---|
| Spectrum (4), Flowers (11), Roll (12), Surmount (17), Speed (18) | RGB picker and spectrum toggle; colors are built into the effect |
| Static (6), Scan (16), Surmount (17), Speed (18), Custom (20) | Animation speed |
| All except Corrugated (1), Cloud (2), Serpentine (3), Roll (12), Speed (18) | Direction |
| Custom (20) | Spectrum toggle; RGB picker remains available as a painting brush |

Direction labels follow the original UI: horizontal for 1/2, clockwise/counterclockwise for 3, vertical for 12, inward/outward for 18. Surmount exposes four preset palettes instead of arbitrary RGB.

## Physical mapping and custom-layout correction

The application draws an 87-key TKL layout. Default-assignment triples identify physical slots; the separate LED map translates those slots to RGB indices. Do not use the LED index to write a key assignment. Original mapping code: 00494af0. The connected map contains no programmable PrtSc entry. That key is dimmed/unselectable, and old profiles containing its color no longer abort an otherwise valid paint operation. Fn can be lit but is reserved for remapping.

Custom painting overlays the selected colors onto the existing slot, preserving other stored RGB values. “Paint all keys” covers 86 programmable entries and excludes PrtSc online and offline. The custom preview uses stored colors where available; unknown unstaged keys appear dark. Reading lighting from the device performs a fresh hardware read, including current custom RGB data.

## Assignments and macros

Keyboard assignment triples are `[20, modifier-mask, HID-usage]`; consumer/media triples use type 30. Disabling a key uses `[20, 00, 00]`. Assignment writes rebuild the default map and overlay profile entries. There is no verified current-remapped-map read command in this edition, so applying a profile replaces previous remaps not represented in that profile. Source: 00493690 / 0047b890.

Macro source: 00493c80, event serializer 0044a8a0 and keyboard event handling 004271f0. Blob format:

- 16-byte header: aa55 magic, total length LE16, count LE16, remaining bytes zero.
- Count × 2-byte absolute offsets, then each macro: event count LE16, two zero bytes, events.
- Event: delay LE16, type byte with bit 80 for down, usage/mask byte.
- Regular keys: type 0a (down 8a). Modifiers: type 09 (down 89), with a modifier mask rather than a keyboard usage.
- Total storage limit: 7168 bytes. Host validation rejects unbalanced presses/releases and oversized data.

The GUI supports app-local recording, numeric event editing, and desktop playback through Rust evdev/uinput. Desktop playback has countdown, repeat count, cancellation, and held-key cleanup. Macro assignment repeat semantics and physical playback remain unverified. Windows program launching and Windows macro-file import are not implemented.

## Validation and remaining coverage

On 2026-09-11:

- User confirmed: **“Custom lighting works now.”** after applying a painted layout in the GTK app.
- Hardware regression activated custom mode, wrote every mapped RGB entry (including an old-profile PrtSc entry to exercise the fix), read back the settings and colors, then restored original settings and colors and verified restoration.
- Brightness change/readback/restore passed.
- Rust tests cover packet fixtures, malformed frames, macro encoding/bounds, physical RGB mapping and unavailable-key handling.
- GTK regression exercises every effect's palette availability, online/offline “Paint all keys,” and the profile dispatched by Apply. It performs no device writes.
- Formatting, tests and Clippy with warnings denied pass. The release binary is built for this machine's Linux/GTK environment.

Physical remapping, macro playback, every direction/preset, all ten custom slots, and polling-rate timing have not all received end-to-end physical verification. Firmware flashing, factory reset, vendor cloud services, hidden audio/screen effects and Windows process hooks are not implemented. These gaps prevent claiming full behavioral parity with every part of the Windows package.

## Native blur

The GTK window uses alpha only on background surfaces, retaining opaque text and controls. `packaging/niri-blur.kdl` requests compositor-native blur for the exact app ID. The installer validates the candidate niri configuration, saves a timestamped backup, and updates only its managed app rule. The rule opens the main window floating at a practical size. It was installed and validated on the user's niri 26.04 desktop. The Appearance switch restores an opaque window when disabled.

Other compositors require their own blur support/configuration. This is not simulated CSS blur or a blurred screenshot. See [niri's native window-effects documentation](https://niri-wm.github.io/niri/Window-Effects.html).
