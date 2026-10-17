use crate::{
    device::{Keyboard, Snapshot},
    model::{self, Library, Macro, MacroEvent, Profile},
};
use anyhow::Context;
use gtk::{Orientation, cairo, gdk, glib, prelude::*};
use std::{cell::RefCell, rc::Rc, sync::mpsc, time::Duration};

struct State {
    library: Library,
    selected: Option<usize>,
    snapshot: Option<Snapshot>,
    busy: bool,
    revision: u64,
}
type Shared = Rc<RefCell<State>>;
enum Job {
    Probe,
    ReadLighting,
    Lighting(Profile),
    Keys(Profile),
    System(Profile),
}
struct Completion {
    result: Result<Option<Snapshot>, String>,
    message: &'static str,
    read_into_profile: bool,
}
fn label(text: &str, class: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.set_xalign(0.0);
    if !class.is_empty() {
        l.add_css_class(class);
    }
    l
}
fn column(spacing: i32) -> gtk::Box {
    gtk::Box::new(Orientation::Vertical, spacing)
}
fn row(spacing: i32) -> gtk::Box {
    gtk::Box::new(Orientation::Horizontal, spacing)
}
fn card(title: &str, sub: &str) -> gtk::Box {
    let b = column(15);
    b.add_css_class("card");
    b.append(&label(title, "section-title"));
    if !sub.is_empty() {
        let l = label(sub, "muted");
        l.set_wrap(true);
        b.append(&l);
    }
    b
}
fn button(text: &str, primary: bool) -> gtk::Button {
    let b = gtk::Button::with_label(text);
    if primary {
        b.add_css_class("primary");
    }
    b
}

fn current(s: &Shared) -> Profile {
    let s = s.borrow();
    s.library.profiles[s.library.selected].clone()
}
fn edit(s: &Shared, f: impl FnOnce(&mut Profile)) {
    let mut s = s.borrow_mut();
    let i = s.library.selected;
    f(&mut s.library.profiles[i]);
}

fn on_profile_change(s: &Shared, widget: &impl IsA<gtk::Widget>, f: impl Fn(&Profile) + 'static) {
    let s = s.clone();
    let weak = widget.downgrade();
    let mut revision = s.borrow().revision;
    glib::timeout_add_local(Duration::from_millis(120), move || {
        if weak.upgrade().is_none() {
            return glib::ControlFlow::Break;
        }
        let next = s.borrow().revision;
        if next != revision {
            revision = next;
            let profile = current(&s);
            f(&profile);
        }
        glib::ControlFlow::Continue
    });
}
fn status_set(status: &gtk::Label, result: anyhow::Result<()>, ok: &str) {
    match result {
        Ok(()) => status.set_text(ok),
        Err(e) => status.set_text(&format!("{e:#}")),
    }
}

fn start_job(state: &Shared, tx: &mpsc::Sender<Job>, status: &gtk::Label, job: Job) {
    if state.borrow().busy {
        status.set_text("A keyboard operation is already in progress.");
        return;
    }
    state.borrow_mut().busy = true;
    status.set_text("Communicating with your keyboard…");
    if tx.send(job).is_err() {
        state.borrow_mut().busy = false;
        status.set_text("Device worker stopped. Restart the app.");
    }
}

pub fn run() -> glib::ExitCode {
    let app = gtk::Application::builder()
        .application_id("io.github.phantom.Studio")
        .build();
    app.connect_activate(build);
    app.run_with_args::<&str>(&[])
}

fn build(app: &gtk::Application) {
    app.set_accels_for_action("window.show-menu", &[]);
    let css = gtk::CssProvider::new();
    css.load_from_string(include_str!("style.css"));
    gtk::style_context_add_provider_for_display(
        &gdk::Display::default().expect("GTK display"),
        &css,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    gtk::Settings::default()
        .unwrap()
        .set_gtk_application_prefer_dark_theme(true);
    let (library, _load_error) = match Library::load() {
        Ok(l) => (l, None),
        Err(e) => (
            Library::default(),
            Some(format!(
                "Could not load saved profiles: {e}. The original file has been kept."
            )),
        ),
    };
    let state = Rc::new(RefCell::new(State {
        library,
        selected: None,
        snapshot: None,
        busy: false,
        revision: 0,
    }));
    let (job_tx, job_rx) = mpsc::channel::<Job>();
    let (done_tx, done_rx) = mpsc::channel();
    std::thread::spawn(move || {
        while let Ok(job) = job_rx.recv() {
            let read_into_profile = matches!(job, Job::ReadLighting);
            let message = match &job {
                Job::Probe => "Keyboard connected. Device information refreshed.",
                Job::ReadLighting => "Current lighting and colors read from the keyboard.",
                Job::Lighting(_) => "Lighting applied and verified by reading it back.",
                Job::Keys(_) => "Keymap sent. Test the assigned keys to verify behavior.",
                Job::System(_) => "Device settings applied and verified.",
            };
            let result = (|| -> anyhow::Result<Option<Snapshot>> {
                let k = Keyboard::open()?;
                match job {
                    Job::Probe | Job::ReadLighting => Ok(Some(k.snapshot()?)),
                    Job::Lighting(p) => Ok(Some(k.apply_lighting(&p)?)),
                    Job::Keys(p) => {
                        k.apply_keys(&p)?;
                        Ok(None)
                    }
                    Job::System(p) => Ok(Some(k.apply_system(&p)?)),
                }
            })()
            .map_err(|e| format!("{e:#}"));
            if done_tx
                .send(Completion {
                    result,
                    message,
                    read_into_profile,
                })
                .is_err()
            {
                break;
            }
        }
    });
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("Phantom Studio")
        .default_width(1024)
        .default_height(640)
        .build();
    let frosted = std::fs::read(model::config_dir().join("appearance.json"))
        .ok()
        .and_then(|data| serde_json::from_slice::<bool>(&data).ok())
        .unwrap_or_else(|| {
            std::env::var("XDG_CURRENT_DESKTOP")
                .unwrap_or_default()
                .to_lowercase()
                .contains("niri")
        });
    if frosted {
        window.add_css_class("frosted");
    }
    let header = gtk::HeaderBar::new();
    header.set_show_title_buttons(true);

    let toggle_sidebar = gtk::ToggleButton::new();
    toggle_sidebar.add_css_class("flat");
    toggle_sidebar.set_icon_name("sidebar-show-symbolic");
    toggle_sidebar.set_tooltip_text(Some("Toggle sidebar (Ctrl+B)"));
    toggle_sidebar.set_active(true);
    header.pack_start(&toggle_sidebar);

    let title = label("PHANTOM STUDIO", "eyebrow");
    title.set_valign(gtk::Align::Center);
    header.set_title_widget(Some(&title));
    window.set_titlebar(Some(&header));

    let status = label("", "");

    let root = column(0);
    let body = row(0);
    body.set_vexpand(true);

    let sidebar = column(10);
    sidebar.add_css_class("sidebar");
    sidebar.set_width_request(184);

    let brand_row = row(6);
    let brand_box = column(2);
    brand_box.append(&label("PHANTOM", "brand"));
    brand_box.append(&label("KEYBOARD STUDIO", "eyebrow"));
    brand_box.set_hexpand(true);
    brand_row.append(&brand_box);

    let collapse_btn = gtk::Button::builder()
        .icon_name("pan-start-symbolic")
        .tooltip_text("Collapse sidebar")
        .build();
    collapse_btn.add_css_class("flat");
    {
        let toggle = toggle_sidebar.clone();
        collapse_btn.connect_clicked(move |_| {
            toggle.set_active(false);
        });
    }
    brand_row.append(&collapse_btn);
    sidebar.append(&brand_row);

    let gap = column(0);
    gap.set_height_request(20);
    sidebar.append(&gap);

    let stack = gtk::Stack::new();
    stack.set_transition_type(gtk::StackTransitionType::Crossfade);
    stack.set_transition_duration(160);
    stack.set_hexpand(true);
    stack.set_vexpand(true);

    let badge = label("DISCONNECTED", "pill");
    badge.set_halign(gtk::Align::Start);
    let dev_name = label("No keyboard", "section-title");
    let dev_keys = label("0 keys", "muted");
    let keyboard = keyboard_view(&state, false);
    let mut first: Option<gtk::ToggleButton> = None;
    for (name, title, icon) in [
        ("lighting", "Lighting", "weather-clear-night-symbolic"),
        ("keymap", "Key assignments", "input-keyboard-symbolic"),
        ("macros", "Macros", "media-record-symbolic"),
        ("profiles", "Profiles", "folder-symbolic"),
        ("device", "Device", "preferences-system-symbolic"),
    ] {
        let nav = gtk::ToggleButton::new();
        nav.add_css_class("nav");
        let contents = row(13);
        contents.append(&gtk::Image::from_icon_name(icon));
        contents.append(&label(title, ""));
        nav.set_child(Some(&contents));
        if let Some(first) = &first {
            nav.set_group(Some(first));
        } else {
            nav.set_active(true);
            first = Some(nav.clone());
        }
        let stack = stack.clone();
        nav.connect_toggled(move |b| {
            if b.is_active() {
                stack.set_visible_child_name(name);
            }
        });
        sidebar.append(&nav);
    }
    let spacer = column(0);
    spacer.set_vexpand(true);
    sidebar.append(&spacer);

    sidebar.append(&label("DETECTED KEYBOARD", "eyebrow"));
    sidebar.append(&dev_name);
    sidebar.append(&dev_keys);
    sidebar.append(&badge);

    {
        let sidebar = sidebar.clone();
        toggle_sidebar.connect_toggled(move |btn| {
            sidebar.set_visible(btn.is_active());
        });
    }

    let key_ctrl = gtk::EventControllerKey::new();
    {
        let toggle = toggle_sidebar.clone();
        key_ctrl.connect_key_pressed(move |_, keyval, _, state| {
            if (state.contains(gdk::ModifierType::CONTROL_MASK) && keyval == gdk::Key::b)
                || keyval == gdk::Key::F9
            {
                toggle.set_active(!toggle.is_active());
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
    }
    window.add_controller(key_ctrl);

    let lighting = lighting_page(&state, &job_tx, &status, &keyboard);
    let keymap = keymap_page(&state, &job_tx, &status);
    let macros = macros_page(&state, &status, &window);
    let profiles = profiles_page(&state, &status, &window);
    let device = device_page(&state, &job_tx, &status, &window);
    for (name, page) in [
        ("lighting", lighting),
        ("keymap", keymap),
        ("macros", macros),
        ("profiles", profiles),
        ("device", device),
    ] {
        page.set_margin_top(16);
        page.set_margin_bottom(16);
        page.set_margin_start(20);
        page.set_margin_end(20);
        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&page)
            .build();
        stack.add_named(&scroll, Some(name));
    }
    body.append(&sidebar);
    body.append(&stack);
    root.append(&body);
    window.set_child(Some(&root));
    {
        let (s, st, badge, dev_name, dev_keys, k) = (
            state.clone(),
            status.clone(),
            badge.clone(),
            dev_name.clone(),
            dev_keys.clone(),
            keyboard.clone(),
        );
        glib::timeout_add_local(Duration::from_millis(80), move || {
            while let Ok(done) = done_rx.try_recv() {
                s.borrow_mut().busy = false;
                match done.result {
                    Ok(snapshot) => {
                        if let Some(snapshot) = snapshot {
                            let p = snapshot.profile();
                            if done.read_into_profile {
                                edit(&s, |old| {
                                    old.lighting = p.lighting;
                                    old.colors = p.colors;
                                    old.win_lock = p.win_lock;
                                    old.six_key_rollover = p.six_key_rollover;
                                    old.swap_wasd = p.swap_wasd;
                                    old.polling_rate = p.polling_rate;
                                });
                                s.borrow_mut().revision += 1;
                            } else {
                                edit(&s, |old| {
                                    old.win_lock = p.win_lock;
                                    old.six_key_rollover = p.six_key_rollover;
                                    old.swap_wasd = p.swap_wasd;
                                    old.polling_rate = p.polling_rate;
                                });
                                s.borrow_mut().revision += 1;
                            }
                            s.borrow_mut().snapshot = Some(snapshot);
                        }
                        dev_name.set_text("Phantom TKL");
                        dev_keys.set_text(&format!("{} keys", model::layout().len()));
                        badge.set_text("●  CONNECTED");
                        badge.add_css_class("connected");
                        st.set_text(done.message);
                    }
                    Err(e) => {
                        st.set_text(&e);
                        dev_name.set_text("No keyboard");
                        dev_keys.set_text("0 keys");
                        badge.set_text("DISCONNECTED");
                        badge.remove_css_class("connected");
                    }
                }
            }
            k.queue_draw();
            glib::ControlFlow::Continue
        });
    }
    start_job(&state, &job_tx, &status, Job::Probe);
    if let Some(nav) = first {
        nav.grab_focus();
    }
    {
        let stack = stack.clone();
        window.add_tick_callback(move |_, _| {
            if let Some(scroll) = stack.visible_child().and_downcast::<gtk::ScrolledWindow>() {
                scroll.vadjustment().set_value(0.);
            }
            glib::ControlFlow::Break
        });
    }
    window.present();
    if let Ok(path) = std::env::var("PHANTOM_SCREENSHOT") {
        let w = window.clone();
        glib::timeout_add_local_once(Duration::from_secs(3), move || {
            let paintable = gtk::WidgetPaintable::new(Some(&w));
            let snap = gtk::Snapshot::new();
            paintable.snapshot(&snap, w.width() as f64, w.height() as f64);
            if let (Some(node), Some(renderer)) = (snap.to_node(), w.renderer()) {
                let texture = renderer.render_texture(&node, None);
                if let Err(e) = texture.save_to_png(path) {
                    eprintln!("Screenshot: {e}");
                }
            }
        });
    }
}

fn hsv_to_rgb(h: f64, s: f64, v: f64) -> (f64, f64, f64) {
    let h = (h.rem_euclid(1.0)) * 6.0;
    let i = h.floor() as i32;
    let f = h - i as f64;
    let p = v * (1.0 - s);
    let q = v * (1.0 - s * f);
    let t = v * (1.0 - s * (1.0 - f));
    match i % 6 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    }
}

fn calculate_key_color(
    key: &model::Key,
    p: &model::Profile,
    elapsed: f64,
    snapshot: Option<&crate::device::Snapshot>,
) -> (f64, f64, f64) {
    if p.lighting.mode == 20 {
        let color = p.colors.get(&key.id).copied().unwrap_or_else(|| {
            snapshot
                .filter(|snap| snap.settings[25] == p.lighting.custom_slot)
                .and_then(|snap| {
                    snap.led_index(key)
                        .and_then(|led| snap.custom_colors.get(led * 3..led * 3 + 3))
                })
                .map(|rgb| [rgb[0], rgb[1], rgb[2]])
                .unwrap_or([0, 0, 0])
        });
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

fn keyboard_view(state: &Shared, select: bool) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_content_width(888);
    area.set_content_height(300);
    area.set_hexpand(true);

    let start_time = std::time::Instant::now();
    let frame_time_micros = std::rc::Rc::new(std::cell::Cell::new(0i64));
    let base_time = std::rc::Rc::new(std::cell::Cell::new(None));
    {
        let frame_time_micros = frame_time_micros.clone();
        let base_time = base_time.clone();
        area.add_tick_callback(move |area, clock| {
            let t = clock.frame_time();
            if t > 0 {
                if base_time.get().is_none() {
                    base_time.set(Some(t));
                }
                frame_time_micros.set(t - base_time.get().unwrap_or(0));
            }
            area.queue_draw();
            glib::ControlFlow::Continue
        });
    }

    {
        let state = state.clone();
        let frame_time_micros = frame_time_micros.clone();
        area.set_draw_func(move |_, cr, w, h| {
            let elapsed = if frame_time_micros.get() > 0 {
                frame_time_micros.get() as f64 / 1_000_000.0
            } else {
                start_time.elapsed().as_secs_f64()
            };
            let s = state.borrow();
            let p = &s.library.profiles[s.library.selected];
            let keys = model::layout();
            let scale = (w as f64 / 902.0).min(h as f64 / 320.0);
            let ox = (w as f64 - 888.0 * scale) / 2.0;
            let oy = (h as f64 - 300.0 * scale) / 2.0;
            cr.translate(ox, oy);
            cr.scale(scale, scale);
            for key in keys {
                let selected = s.selected == Some(key.id);
                let available = key.id != 13
                    && s.snapshot
                        .as_ref()
                        .is_none_or(|snap| snap.key_index(&key).is_some());
                let (r, g, b) = calculate_key_color(&key, p, elapsed, s.snapshot.as_ref());
                rounded(cr, key.x, key.y, key.width, key.height, 6.0);
                cr.set_source_rgb(0.05, 0.05, 0.05);
                let _ = cr.fill();
                rounded(cr, key.x, key.y, key.width, key.height - 3., 6.);
                cr.set_source_rgb(0.12 + r * 0.07, 0.12 + g * 0.07, 0.12 + b * 0.07);
                let _ = cr.fill_preserve();
                cr.set_line_width(if selected { 2.5 } else { 1. });
                cr.set_source_rgba(r, g, b, if selected { 1.0 } else { 0.4 });
                let _ = cr.stroke();
                cr.set_source_rgba(r, g, b, 0.6 * p.lighting.brightness as f64 / 4.);
                cr.rectangle(key.x + 8., key.y + key.height - 3., key.width - 16., 1.5);
                let _ = cr.fill();
                cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
                cr.set_font_size(if key.label.len() > 5 { 10. } else { 12. });
                cr.set_source_rgba(0.92, 0.92, 0.92, if available { 1.0 } else { 0.25 });
                if let Some(angle) = match key.label.as_str() {
                    "↑" => Some(-90.),
                    "↓" => Some(90.),
                    "←" => Some(180.),
                    "→" => Some(0.),
                    _ => None,
                } {
                    let _ = cr.save();
                    cr.translate(key.x + key.width / 2., key.y + 20.);
                    cr.rotate(angle * std::f64::consts::PI / 180.);
                    cr.set_line_width(1.5);
                    cr.move_to(-5., 0.);
                    cr.line_to(5., 0.);
                    cr.move_to(1., -4.);
                    cr.line_to(5., 0.);
                    cr.line_to(1., 4.);
                    let _ = cr.stroke();
                    let _ = cr.restore();
                } else if let Ok(e) = cr.text_extents(&key.label) {
                    cr.move_to(
                        key.x + (key.width - e.width()) / 2. - e.x_bearing(),
                        key.y + 23.,
                    );
                    let _ = cr.show_text(&key.label);
                }
                if p.assignments.contains_key(&key.id) {
                    cr.set_source_rgb(1.0, 1.0, 1.0);
                    cr.arc(
                        key.x + key.width - 7.,
                        key.y + 7.,
                        2.,
                        0.,
                        std::f64::consts::TAU,
                    );
                    let _ = cr.fill();
                }
            }
        });
    }
    if select {
        let click = gtk::GestureClick::new();
        let (s, a) = (state.clone(), area.clone());
        click.connect_pressed(move |_, _, x, y| {
            let scale = (a.width() as f64 / 902.).min(a.height() as f64 / 320.);
            let ox = (a.width() as f64 - 888. * scale) / 2.;
            let oy = (a.height() as f64 - 300. * scale) / 2.;
            let (x, y) = ((x - ox) / scale, (y - oy) / scale);
            let selected = model::layout()
                .iter()
                .find(|k| x >= k.x && x <= k.x + k.width && y >= k.y && y <= k.y + k.height)
                .map(|k| k.id);
            let available = selected.is_none_or(|id| state_key_available(&s, id));
            if available {
                s.borrow_mut().selected = selected;
            }
            a.queue_draw();
        });
        area.add_controller(click);
    }
    area
}
fn state_key_available(s: &Shared, id: usize) -> bool {
    if id == 13 {
        return false;
    }
    let state = s.borrow();
    model::layout()
        .iter()
        .find(|k| k.id == id)
        .is_some_and(|key| {
            state
                .snapshot
                .as_ref()
                .is_none_or(|snapshot| snapshot.key_index(key).is_some())
        })
}
fn rounded(cr: &cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    cr.new_sub_path();
    for (cx, cy, a) in [
        (x + w - r, y + r, -90.),
        (x + w - r, y + h - r, 0.),
        (x + r, y + h - r, 90.),
        (x + r, y + r, 180.),
    ] {
        cr.arc(
            cx,
            cy,
            r,
            a * std::f64::consts::PI / 180.,
            (a + 90.) * std::f64::consts::PI / 180.,
        );
    }
    cr.close_path();
}

fn lighting_page(
    s: &Shared,
    tx: &mpsc::Sender<Job>,
    status: &gtk::Label,
    keyboard: &gtk::DrawingArea,
) -> gtk::Box {
    let page = column(23);
    let top = row(10);
    let spacer = column(0);
    spacer.set_hexpand(true);
    top.append(&spacer);
    let apply = button("Apply lighting  ↗", true);
    apply.set_valign(gtk::Align::Center);
    {
        let (s, tx, st) = (s.clone(), tx.clone(), status.clone());
        apply.connect_clicked(move |_| start_job(&s, &tx, &st, Job::Lighting(current(&s))));
    }
    top.append(&apply);
    page.append(&top);
    let hero = column(20);
    hero.add_css_class("keyboard-card");
    let hr = row(10);
    let text = label("PHANTOM TKL", "eyebrow");
    text.set_hexpand(true);
    hr.append(&text);
    hr.append(&label("LIGHTING PREVIEW", "pill"));
    hero.append(&hr);
    hero.append(keyboard);
    page.append(&hero);
    let controls = row(18);
    controls.set_homogeneous(true);
    let effect = card(
        "01  /  The effect",
        "Choose how your keyboard comes to life.",
    );
    let names: Vec<&str> = model::EFFECTS.iter().map(|e| e.1).collect();
    let modes = gtk::DropDown::from_strings(&names);
    let p = current(s);
    modes.set_selected(
        model::EFFECTS
            .iter()
            .position(|e| e.0 == p.lighting.mode)
            .unwrap_or(0) as u32,
    );
    {
        let (s, k) = (s.clone(), keyboard.clone());
        modes.connect_selected_notify(move |d| {
            if let Some(e) = model::EFFECTS.get(d.selected() as usize) {
                edit(&s, |p| p.lighting.mode = e.0);
                k.queue_draw();
            }
        });
    }
    effect.append(&modes);
    {
        let w = modes.clone();
        on_profile_change(s, &modes, move |p| {
            w.set_selected(
                model::EFFECTS
                    .iter()
                    .position(|e| e.0 == p.lighting.mode)
                    .unwrap_or(0) as u32,
            )
        });
    }
    let mut animation_scale = None;
    for (title, value, is_speed) in [
        ("Brightness", p.lighting.brightness, false),
        ("Animation speed", p.lighting.speed, true),
    ] {
        let r = row(0);
        let title = label(title, "muted");
        title.set_hexpand(true);
        r.append(&title);
        let val = label(&format!("{}%", value * 25), "");
        r.append(&val);
        effect.append(&r);
        let scale = gtk::Scale::with_range(Orientation::Horizontal, 0., 4., 1.);
        scale.set_value(value as f64);
        scale.set_draw_value(false);
        if is_speed {
            animation_scale = Some(scale.clone());
        }
        {
            let (s, k) = (s.clone(), keyboard.clone());
            scale.connect_value_changed(move |v| {
                let n = v.value() as u8;
                val.set_text(&format!("{}%", n * 25));
                edit(&s, |p| {
                    if is_speed {
                        p.lighting.speed = n
                    } else {
                        p.lighting.brightness = n
                    }
                });
                k.queue_draw();
            });
        }
        effect.append(&scale);
        {
            let w = scale.clone();
            on_profile_change(s, &scale, move |p| {
                w.set_value(if is_speed {
                    p.lighting.speed
                } else {
                    p.lighting.brightness
                } as f64)
            });
        }
    }
    let dir = gtk::DropDown::from_strings(&[
        "Direction  ·  Default",
        "Direction  ·  Left",
        "Direction  ·  Right",
        "Direction  ·  Up",
        "Direction  ·  Down",
    ]);
    dir.set_selected(p.lighting.direction as u32);
    {
        let s = s.clone();
        dir.connect_selected_notify(move |d| {
            if d.selected() < 2 {
                edit(&s, |p| p.lighting.direction = d.selected() as u8);
            }
        });
    }
    effect.append(&dir);
    {
        let w = dir.clone();
        on_profile_change(s, &dir, move |p| {
            w.set_selected(p.lighting.direction as u32)
        });
    }
    let preset = gtk::DropDown::from_strings(&[
        "Surmount · Palette 1",
        "Surmount · Palette 2",
        "Surmount · Palette 3",
        "Surmount · Palette 4",
    ]);
    preset.set_selected(p.lighting.surmount_preset as u32);
    preset.set_visible(p.lighting.mode == 17);
    {
        let s = s.clone();
        preset.connect_selected_notify(move |w| {
            if w.selected() < 4 {
                edit(&s, |p| p.lighting.surmount_preset = w.selected() as u8);
            }
        });
    }
    {
        let w = preset.clone();
        modes.connect_selected_notify(move |d| {
            w.set_visible(
                model::EFFECTS
                    .get(d.selected() as usize)
                    .is_some_and(|e| e.0 == 17),
            );
        });
    }
    {
        let w = preset.clone();
        on_profile_change(s, &preset, move |p| {
            w.set_selected(p.lighting.surmount_preset as u32)
        });
    }
    effect.append(&preset);
    controls.append(&effect);
    let color = card("02  /  The palette", "Find your signature color.");
    let rainbow = row(10);
    let t = label("Spectrum colors", "muted");
    t.set_hexpand(true);
    rainbow.append(&t);
    let sw = gtk::Switch::new();
    sw.set_active(p.lighting.rainbow);
    rainbow.append(&sw);
    {
        let (s, k) = (s.clone(), keyboard.clone());
        sw.connect_active_notify(move |w| {
            edit(&s, |p| p.lighting.rainbow = w.is_active());
            k.queue_draw();
        });
    }
    color.append(&rainbow);
    {
        let w = sw.clone();
        on_profile_change(s, &sw, move |p| w.set_active(p.lighting.rainbow));
    }
    let color_inputs = column(12);
    let swatches = row(5);
    let hex = gtk::Entry::new();
    hex.set_text(&format!(
        "#{:02X}{:02X}{:02X}",
        p.lighting.color[0], p.lighting.color[1], p.lighting.color[2]
    ));
    for c in [
        [188, 162, 255],
        [113, 196, 255],
        [113, 230, 180],
        [255, 197, 107],
        [255, 121, 154],
        [239, 239, 250],
    ] {
        let b = gtk::Button::new();
        b.add_css_class("swatch");
        let provider = gtk::CssProvider::new();
        provider.load_from_string(&format!(
            ".swatch-{}-{}-{} {{background:rgb({},{},{});}}",
            c[0], c[1], c[2], c[0], c[1], c[2]
        ));
        b.add_css_class(&format!("swatch-{}-{}-{}", c[0], c[1], c[2]));
        gtk::style_context_add_provider_for_display(
            &gdk::Display::default().unwrap(),
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
        );
        let (s, k, hex) = (s.clone(), keyboard.clone(), hex.clone());
        b.connect_clicked(move |_| {
            edit(&s, |p| {
                p.lighting.color = c;
                p.lighting.rainbow = false;
            });
            s.borrow_mut().revision += 1;
            hex.set_text(&format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2]));
            k.queue_draw();
        });
        swatches.append(&b);
    }
    color_inputs.append(&swatches);
    color_inputs.append(&label("HEX COLOR", "eyebrow"));
    color_inputs.append(&hex);
    {
        let w = hex.clone();
        on_profile_change(s, &hex, move |p| {
            w.set_text(&format!(
                "#{:02X}{:02X}{:02X}",
                p.lighting.color[0], p.lighting.color[1], p.lighting.color[2]
            ))
        });
    }
    {
        let (s, k, st) = (s.clone(), keyboard.clone(), status.clone());
        hex.connect_changed(move |e| {
            let text = e.text();
            let t = text.trim_start_matches('#');
            if t.len() == 6
                && let Ok(c) = u32::from_str_radix(t, 16)
            {
                edit(&s, |p| {
                    p.lighting.color = [(c >> 16) as u8, (c >> 8) as u8, c as u8]
                });
                k.queue_draw();
                st.set_text("Color updated. Apply lighting to send it to the keyboard.");
            }
        });
    }
    let choose = button("Open color picker", false);
    {
        let (s, hex) = (s.clone(), hex.clone());
        choose.connect_clicked(move |button| {
            let dialog = gtk::ColorDialog::builder()
                .title("Choose your color")
                .with_alpha(false)
                .build();
            let c = current(&s).lighting.color;
            let rgba = gdk::RGBA::new(
                c[0] as f32 / 255.,
                c[1] as f32 / 255.,
                c[2] as f32 / 255.,
                1.,
            );
            let hex = hex.clone();
            dialog.choose_rgba(
                button.root().and_downcast::<gtk::Window>().as_ref(),
                Some(&rgba),
                None::<&gtk::gio::Cancellable>,
                move |result| {
                    if let Ok(c) = result {
                        hex.set_text(&format!(
                            "#{:02X}{:02X}{:02X}",
                            (c.red() * 255.) as u8,
                            (c.green() * 255.) as u8,
                            (c.blue() * 255.) as u8
                        ));
                    }
                },
            );
        });
    }
    color_inputs.append(&choose);
    color.append(&color_inputs);
    let palette_note = label("", "subtle");
    palette_note.set_wrap(true);
    color.append(&palette_note);
    {
        let (s, dir, rainbow, color_inputs, note, scale) = (
            s.clone(),
            dir.clone(),
            rainbow.clone(),
            color_inputs.clone(),
            palette_note.clone(),
            animation_scale.unwrap(),
        );
        let initial_mode = current(&s).lighting.mode;
        let update = move |mode: u8| {
            let caps = model::effect_controls(mode);
            color_inputs.set_sensitive(caps.color);
            rainbow.set_sensitive(caps.rainbow);
            scale.set_sensitive(caps.speed);
            dir.set_sensitive(caps.direction);
            let directions: &[&str] = match mode {
                3 => &["Clockwise", "Counterclockwise"],
                12 => &["Bottom to top", "Top to bottom"],
                18 => &["Outside to inside", "Inside to outside"],
                _ => &["Right to left", "Left to right"],
            };
            let direction = current(&s).lighting.direction.min(1) as u32;
            dir.set_model(Some(&gtk::StringList::new(directions)));
            dir.set_selected(direction);
            note.set_text(if !caps.color {
                "This effect uses its own colors."
            } else if mode == 20 {
                "Choose a brush color, then paint keys below."
            } else {
                "Turn off Spectrum colors to use your chosen color."
            });
        };
        update(initial_mode);
        modes.connect_selected_notify(move |d| {
            if let Some(e) = model::EFFECTS.get(d.selected() as usize) {
                update(e.0);
            }
        });
    }
    let save = button("Save to profile", false);
    {
        let (s, st) = (s.clone(), status.clone());
        save.connect_clicked(move |_| status_set(&st, s.borrow().library.save(), "Profile saved."));
    }
    color.append(&save);
    controls.append(&color);
    page.append(&controls);
    let custom = card(
        "Paint individual keys",
        "Select a key and paint it with the current palette color. Apply lighting when you're ready.",
    );
    let custom_mode = button("Edit custom lighting", false);
    {
        let s = s.clone();
        custom_mode.connect_clicked(move |_| {
            edit(&s, |p| p.lighting.mode = 20);
            s.borrow_mut().revision += 1;
        });
    }
    custom.append(&custom_mode);
    let paint = keyboard_view(s, true);
    custom.append(&paint);
    {
        let w = paint.clone();
        on_profile_change(s, &paint, move |_| w.queue_draw());
    }
    let actions = row(8);
    let one = button("Paint selected key", false);
    let all = button("Paint all keys", false);
    let clear = button("Discard staged painting", false);
    {
        let (s, st, paint) = (s.clone(), status.clone(), paint.clone());
        one.connect_clicked(move |_| {
            let id = s.borrow().selected;
            if let Some(id) = id {
                edit(&s, |p| {
                    p.colors.insert(id, p.lighting.color);
                    p.lighting.mode = 20;
                });
                s.borrow_mut().revision += 1;
                paint.queue_draw();
                st.set_text("Key painted. Apply lighting to send your custom colors.");
            } else {
                st.set_text("Select a key in the keyboard above.");
            }
        });
    }
    {
        let (s, paint) = (s.clone(), paint.clone());
        all.connect_clicked(move |_| {
            let keys: Vec<_> = model::layout()
                .into_iter()
                .filter(|key| state_key_available(&s, key.id))
                .collect();
            edit(&s, |p| {
                for k in keys {
                    p.colors.insert(k.id, p.lighting.color);
                }
                p.lighting.mode = 20;
            });
            s.borrow_mut().revision += 1;
            paint.queue_draw();
        });
    }
    {
        let (s, paint) = (s.clone(), paint.clone());
        clear.connect_clicked(move |_| {
            edit(&s, |p| p.colors.clear());
            paint.queue_draw();
        });
    }
    actions.append(&one);
    actions.append(&all);
    actions.append(&clear);
    custom.append(&actions);
    let slots = gtk::DropDown::from_strings(&[
        "Custom slot 1",
        "Custom slot 2",
        "Custom slot 3",
        "Custom slot 4",
        "Custom slot 5",
        "Custom slot 6",
        "Custom slot 7",
        "Custom slot 8",
        "Custom slot 9",
        "Custom slot 10",
    ]);
    slots.set_selected(current(s).lighting.custom_slot as u32);
    {
        let s = s.clone();
        slots.connect_selected_notify(move |w| {
            edit(&s, |p| p.lighting.custom_slot = w.selected() as u8)
        });
    }
    {
        let w = slots.clone();
        on_profile_change(s, &slots, move |p| {
            w.set_selected(p.lighting.custom_slot as u32)
        });
    }
    custom.append(&slots);
    let apply_custom = button("Apply custom lighting", true);
    {
        let (s, tx, st) = (s.clone(), tx.clone(), status.clone());
        apply_custom.connect_clicked(move |_| {
            edit(&s, |p| p.lighting.mode = 20);
            s.borrow_mut().revision += 1;
            start_job(&s, &tx, &st, Job::Lighting(current(&s)));
        });
    }
    custom.append(&apply_custom);
    page.append(&custom);
    page
}

fn keymap_page(s: &Shared, tx: &mpsc::Sender<Job>, status: &gtk::Label) -> gtk::Box {
    let page = column(24);
    let kb = card(
        "KEY ASSIGNMENTS",
        "Dots mark keys with an assignment in this profile.",
    );
    let keys = keyboard_view(s, true);
    kb.append(&keys);
    page.append(&kb);
    let selected = label("Select a key above", "section-title");
    page.append(&selected);
    {
        let (s, l, k) = (s.clone(), selected.clone(), keys.clone());
        glib::timeout_add_local(Duration::from_millis(150), move || {
            let id = s.borrow().selected;
            l.set_text(
                &model::layout()
                    .iter()
                    .find(|key| Some(key.id) == id)
                    .map(|key| format!("Selected: {}", key.label))
                    .unwrap_or("Select a key above".into()),
            );
            k.queue_draw();
            glib::ControlFlow::Continue
        });
    }
    let editor = card(
        "Assign a function",
        "Fn is reserved by the keyboard firmware. Applying a profile replaces its onboard keymap.",
    );
    let choices = model::layout();
    let mut labels: Vec<String> = choices
        .iter()
        .filter(|k| k.id != 110)
        .map(|k| format!("Keyboard · {}", k.label))
        .collect();
    let base = labels.len();
    labels.extend(
        [
            "Media · Play / pause",
            "Media · Next track",
            "Media · Previous track",
            "Media · Volume up",
            "Media · Volume down",
            "Media · Mute",
            "Disabled",
        ]
        .iter()
        .map(|s| s.to_string()),
    );
    let names: Vec<&str> = labels.iter().map(|s| s.as_str()).collect();
    let targets = gtk::DropDown::from_strings(&names);
    targets.set_enable_search(true);
    editor.append(&targets);
    let set = button("Assign selected key", true);
    {
        let (s, st, targets) = (s.clone(), status.clone(), targets.clone());
        set.connect_clicked(move |_| {
            let Some(id) = s.borrow().selected else {
                st.set_text("Select a key first.");
                return;
            };
            if id == 110 {
                st.set_text("Fn is reserved by the keyboard firmware.");
                return;
            }
            let n = targets.selected() as usize;
            let assignment = if n < base {
                let keys: Vec<_> = model::layout()
                    .into_iter()
                    .filter(|k| k.id != 110)
                    .collect();
                let b = keys[n].assignment.to_be_bytes();
                [b[1], b[2], b[3]]
            } else {
                match n - base {
                    0 => [0x30, 0xcd, 0],
                    1 => [0x30, 0xb5, 0],
                    2 => [0x30, 0xb6, 0],
                    3 => [0x30, 0xe9, 0],
                    4 => [0x30, 0xea, 0],
                    5 => [0x30, 0xe2, 0],
                    _ => [0x20, 0, 0],
                }
            };
            edit(&s, |p| {
                p.assignments.insert(id, assignment);
            });
            st.set_text("Assignment staged. Apply keymap to send it to the keyboard.");
        });
    }
    editor.append(&set);
    let macro_dropdown = gtk::DropDown::from_strings(&["No macros saved"]);
    let refresh_macro_dropdown = {
        let macro_dropdown = macro_dropdown.clone();
        move |p: &Profile| {
            let names: Vec<String> = p
                .macros
                .iter()
                .enumerate()
                .map(|(i, m)| format!("{:02} · {}", i + 1, m.name))
                .collect();
            let name_slices: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
            macro_dropdown.set_model(Some(&gtk::StringList::new(if name_slices.is_empty() {
                &["No macros saved"]
            } else {
                &name_slices
            })));
            macro_dropdown.set_sensitive(!names.is_empty());
        }
    };
    refresh_macro_dropdown(&current(s));
    on_profile_change(s, &macro_dropdown, refresh_macro_dropdown);

    let mr = row(10);
    mr.append(&label("Macro", "muted"));
    mr.append(&macro_dropdown);
    let assign_macro = button("Assign macro", false);
    {
        let (s, st, macro_dropdown) = (s.clone(), status.clone(), macro_dropdown.clone());
        assign_macro.connect_clicked(move |_| {
            let Some(id) = s.borrow().selected else {
                st.set_text("Select a key first.");
                return;
            };
            let n = macro_dropdown.selected() as usize;
            if id == 110 || n >= current(&s).macros.len() {
                st.set_text("Choose an existing macro and a key other than Fn.");
                return;
            }
            edit(&s, |p| {
                p.assignments.insert(id, [0x70, n as u8, 1]);
            });
            st.set_text("Macro assignment staged.");
        });
    }
    mr.append(&assign_macro);
    editor.append(&mr);
    let actions = row(10);
    let reset = button("Restore selected key", false);
    {
        let (s, st) = (s.clone(), status.clone());
        reset.connect_clicked(move |_| {
            if let Some(id) = s.borrow().selected {
                edit(&s, |p| {
                    p.assignments.remove(&id);
                });
                st.set_text("Default assignment staged.");
            }
        });
    }
    let apply = button("Apply keymap", true);
    {
        let (s, tx, st) = (s.clone(), tx.clone(), status.clone());
        apply.connect_clicked(move |_| start_job(&s, &tx, &st, Job::Keys(current(&s))));
    }
    actions.append(&reset);
    actions.append(&apply);
    editor.append(&actions);
    page.append(&editor);
    page
}

#[derive(Clone)]
struct MacroEditorWidgets {
    s: Shared,
    status: gtk::Label,
    name_entry: gtk::Entry,
    text_view: gtk::TextView,
    events: Rc<RefCell<Vec<MacroEvent>>>,
    editing_index: Rc<RefCell<Option<usize>>>,
    editor_title: gtk::Label,
    save_btn: gtk::Button,
    save_as_new_btn: gtk::Button,
    seq_preview: gtk::Label,
    player: Rc<RefCell<crate::playback::Player>>,
    playback_status: gtk::Label,
    repeats: gtk::SpinButton,
    countdown: gtk::SpinButton,
    list: gtk::Box,
}

fn macros_page(s: &Shared, status: &gtk::Label, window: &gtk::ApplicationWindow) -> gtk::Box {
    let player = Rc::new(RefCell::new(crate::playback::Player::default()));
    let bg_recorder = Rc::new(RefCell::new(
        Option::<crate::playback::BackgroundRecorder>::None,
    ));
    let in_win_rec = Rc::new(RefCell::new(Option::<std::time::Instant>::None));
    let events = Rc::new(RefCell::new(Vec::<MacroEvent>::new()));
    let editing_index = Rc::new(RefCell::new(Option::<usize>::None));

    let page = column(20);

    // 1. TOP CARD: Macro Library
    let lib_card = card(
        "Saved Macros",
        "Hardware macros stored onboard your keyboard (capacity: 7168 bytes).",
    );
    let list = column(6);
    lib_card.append(&list);
    page.append(&lib_card);

    // 2. MIDDLE CARD: Unified Macro Editor
    let editor_card = card(
        "Macro Editor",
        "Record keystrokes globally in games/apps or write events manually.",
    );
    let editor_title = label("CREATE NEW MACRO", "eyebrow");
    editor_card.append(&editor_title);

    let name_row = row(10);
    let name_entry = gtk::Entry::builder()
        .placeholder_text("Macro name (e.g. Rapid Fire, Crouch Jump)")
        .text("My macro")
        .hexpand(true)
        .build();
    let new_btn = button("+ New Macro", false);
    name_row.append(&name_entry);
    name_row.append(&new_btn);
    editor_card.append(&name_row);

    // Sequence preview
    let seq_preview = label("No actions recorded yet.", "macro-keys");
    seq_preview.set_wrap(true);
    editor_card.append(&seq_preview);

    // Recording bar
    let rec_bar = row(12);
    let record_btn = button("●  Record keystrokes", false);
    record_btn.set_valign(gtk::Align::Center);
    let bg_switch = gtk::Switch::new();
    bg_switch.set_active(true);
    bg_switch.set_valign(gtk::Align::Center);
    let bg_label = label(
        "Background recording (captures in games & background apps)",
        "muted",
    );
    bg_label.set_valign(gtk::Align::Center);
    rec_bar.append(&record_btn);
    rec_bar.append(&bg_switch);
    rec_bar.append(&bg_label);
    editor_card.append(&rec_bar);

    // Recording active banner
    let rec_banner = label(
        "● Recording active — keystrokes are being captured. Click Stop when finished.",
        "recording-banner",
    );
    rec_banner.set_visible(false);
    rec_banner.set_wrap(true);
    editor_card.append(&rec_banner);

    // Monospace text view
    let text_view = gtk::TextView::new();
    text_view.set_monospace(true);
    text_view.set_height_request(150);
    text_view.buffer().set_text("down 4 0\nup 4 100");
    editor_card.append(&text_view);

    // Text buffer live preview sync
    {
        let seq_preview = seq_preview.clone();
        text_view.buffer().connect_changed(move |buf| {
            let text = buf.text(&buf.start_iter(), &buf.end_iter(), false);
            if let Ok(evs) = parse_macro(&text) {
                seq_preview.set_text(&visual_sequence_summary(&evs));
            }
        });
    }

    // Save & Action buttons
    let save_actions = row(10);
    let save_btn = button("Save Macro", true);
    let save_as_new_btn = button("Save as New", false);
    save_as_new_btn.set_visible(false);
    let clear_btn = button("Clear Editor", false);
    save_actions.append(&save_btn);
    save_actions.append(&save_as_new_btn);
    save_actions.append(&clear_btn);
    editor_card.append(&save_actions);
    page.append(&editor_card);

    // 3. BOTTOM CARD: Playback Testing
    let playback_card = card(
        "Playback Test",
        "Test your macro on your desktop through a virtual keyboard before assigning it to hardware keys.",
    );
    let play_controls = row(12);
    let choices = gtk::DropDown::from_strings(&["Save a macro first"]);
    let repeats = gtk::SpinButton::with_range(1., 100., 1.);
    let countdown = gtk::SpinButton::with_range(1., 10., 1.);
    countdown.set_value(2.);
    let play_btn = button("▶  Play Macro", true);
    let stop_btn = button("■  Stop", false);

    play_controls.append(&label("Macro", "muted"));
    play_controls.append(&choices);
    play_controls.append(&label("Repeats", "muted"));
    play_controls.append(&repeats);
    play_controls.append(&label("Countdown (s)", "muted"));
    play_controls.append(&countdown);
    play_controls.append(&play_btn);
    play_controls.append(&stop_btn);
    playback_card.append(&play_controls);

    let playback_status = label("Ready. Select a macro and click Play.", "muted");
    playback_status.set_wrap(true);
    playback_card.append(&playback_status);
    page.append(&playback_card);

    // Context bundle for macro operations
    let ctx = MacroEditorWidgets {
        s: s.clone(),
        status: status.clone(),
        name_entry: name_entry.clone(),
        text_view: text_view.clone(),
        events: events.clone(),
        editing_index: editing_index.clone(),
        editor_title: editor_title.clone(),
        save_btn: save_btn.clone(),
        save_as_new_btn: save_as_new_btn.clone(),
        seq_preview: seq_preview.clone(),
        player: player.clone(),
        playback_status: playback_status.clone(),
        repeats: repeats.clone(),
        countdown: countdown.clone(),
        list: list.clone(),
    };

    refresh_macros(&ctx);

    // Profile change listener
    {
        let ctx = ctx.clone();
        let choices = choices.clone();
        on_profile_change(s, &list, move |p| {
            refresh_macros(&ctx);
            let sel = choices
                .selected()
                .min(p.macros.len().saturating_sub(1) as u32);
            let names: Vec<_> = p.macros.iter().map(|m| m.name.as_str()).collect();
            choices.set_model(Some(&gtk::StringList::new(if names.is_empty() {
                &["Save a macro first"]
            } else {
                &names
            })));
            choices.set_selected(sel);
            choices.set_sensitive(!names.is_empty());
        });
    }

    // New macro button
    {
        let ctx = ctx.clone();
        new_btn.connect_clicked(move |_| {
            reset_macro_editor(&ctx);
            ctx.status.set_text("Switched to new macro mode.");
        });
    }

    // Clear editor button
    {
        let ctx = ctx.clone();
        clear_btn.connect_clicked(move |_| {
            ctx.text_view.buffer().set_text("");
            ctx.events.borrow_mut().clear();
            ctx.seq_preview.set_text("Editor cleared.");
        });
    }

    // Save macro handler
    let save_logic = {
        let ctx = ctx.clone();
        let choices = choices.clone();
        Rc::new(move |force_new: bool| {
            let buffer = ctx.text_view.buffer();
            let input = buffer.text(&buffer.start_iter(), &buffer.end_iter(), false);
            let result = (|| -> anyhow::Result<String> {
                let events = parse_macro(&input)?;
                anyhow::ensure!(
                    !events.is_empty(),
                    "Macro sequence has no valid key actions"
                );
                let mut p = current(&ctx.s);
                let cur_edit = if force_new {
                    None
                } else {
                    *ctx.editing_index.borrow()
                };
                let raw_name = ctx.name_entry.text().to_string();
                let safe_name = deduplicate_macro_name(&p.macros, &raw_name, cur_edit);

                let mac = Macro {
                    name: safe_name.clone(),
                    events,
                };

                if let Some(idx) = cur_edit {
                    if idx < p.macros.len() {
                        p.macros[idx] = mac;
                    } else {
                        p.macros.push(mac);
                        *ctx.editing_index.borrow_mut() = None;
                    }
                } else {
                    p.macros.push(mac);
                    *ctx.editing_index.borrow_mut() = Some(p.macros.len() - 1);
                }

                p.validate()?;
                edit(&ctx.s, |old| *old = p);
                ctx.s.borrow().library.save()?;
                ctx.s.borrow_mut().revision += 1;
                ctx.name_entry.set_text(&safe_name);
                ctx.editor_title
                    .set_text(&format!("EDITING MACRO: {safe_name}"));
                ctx.save_btn.set_label("Update Macro");
                ctx.save_as_new_btn.set_visible(true);
                refresh_macros(&ctx);

                // Update choices dropdown
                let p_curr = current(&ctx.s);
                let names: Vec<_> = p_curr.macros.iter().map(|m| m.name.as_str()).collect();
                choices.set_model(Some(&gtk::StringList::new(&names)));
                if let Some(idx) = *ctx.editing_index.borrow() {
                    choices.set_selected(idx as u32);
                }
                choices.set_sensitive(true);

                Ok(safe_name)
            })();

            match result {
                Ok(name) => {
                    ctx.status
                        .set_text(&format!("Macro '{name}' saved successfully."));
                }
                Err(e) => {
                    ctx.status.set_text(&format!("Save failed: {e:#}"));
                }
            }
        })
    };

    {
        let save_logic = save_logic.clone();
        save_btn.connect_clicked(move |_| save_logic(false));
    }
    {
        let save_logic = save_logic.clone();
        save_as_new_btn.connect_clicked(move |_| save_logic(true));
    }

    // In-window key capture controller (fallback or explicit in-window mode)
    let capture = gtk::EventControllerKey::new();
    capture.set_propagation_phase(gtk::PropagationPhase::Capture);
    {
        let (in_win_rec, events, text_view, seq_preview) = (
            in_win_rec.clone(),
            events.clone(),
            text_view.clone(),
            seq_preview.clone(),
        );
        capture.connect_key_pressed(move |_, key, _, _| {
            let last = *in_win_rec.borrow();
            if let Some(last) = last {
                if let Some(usage) = gdk_usage(key) {
                    let delay = last.elapsed().as_millis().min(65535) as u16;
                    *in_win_rec.borrow_mut() = Some(std::time::Instant::now());
                    if !events
                        .borrow()
                        .iter()
                        .rev()
                        .find(|e| e.usage == usage)
                        .is_some_and(|e| e.down)
                    {
                        events.borrow_mut().push(MacroEvent {
                            delay_ms: delay,
                            usage,
                            down: true,
                        });
                        text_view.buffer().set_text(&macro_text(&events.borrow()));
                        seq_preview.set_text(&visual_sequence_summary(&events.borrow()));
                    }
                }
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
    }
    {
        let (in_win_rec, events, text_view, seq_preview) = (
            in_win_rec.clone(),
            events.clone(),
            text_view.clone(),
            seq_preview.clone(),
        );
        capture.connect_key_released(move |_, key, _, _| {
            let last = *in_win_rec.borrow();
            if let Some(last) = last
                && let Some(usage) = gdk_usage(key)
            {
                let delay = last.elapsed().as_millis().min(65535) as u16;
                *in_win_rec.borrow_mut() = Some(std::time::Instant::now());
                events.borrow_mut().push(MacroEvent {
                    delay_ms: delay,
                    usage,
                    down: false,
                });
                text_view.buffer().set_text(&macro_text(&events.borrow()));
                seq_preview.set_text(&visual_sequence_summary(&events.borrow()));
            }
        });
    }

    // Record button handler
    {
        let (
            bg_recorder,
            in_win_rec,
            events,
            player,
            window,
            capture,
            record_btn,
            rec_banner,
            bg_switch,
            text_view,
            seq_preview,
            status,
        ) = (
            bg_recorder.clone(),
            in_win_rec.clone(),
            events.clone(),
            player.clone(),
            window.clone(),
            capture.clone(),
            record_btn.clone(),
            rec_banner.clone(),
            bg_switch.clone(),
            text_view.clone(),
            seq_preview.clone(),
            status.clone(),
        );
        record_btn.connect_clicked(move |b| {
            if player.borrow().is_running() {
                status.set_text("Stop playback before recording.");
                return;
            }

            let is_recording = bg_recorder.borrow().is_some() || in_win_rec.borrow().is_some();

            if is_recording {
                // Stop recording
                if let Some(mut rec) = bg_recorder.borrow_mut().take() {
                    rec.stop();
                }
                if in_win_rec.borrow().is_some() {
                    window.remove_controller(&capture);
                    *in_win_rec.borrow_mut() = None;
                }

                // Sanitize recorded sequence so no unclosed keys remain
                let fixed = model::sanitize_events(&events.borrow());
                *events.borrow_mut() = fixed;
                text_view.buffer().set_text(&macro_text(&events.borrow()));
                seq_preview.set_text(&visual_sequence_summary(&events.borrow()));

                b.set_label("●  Record keystrokes");
                b.remove_css_class("danger");
                rec_banner.set_visible(false);
                status.set_text(&format!(
                    "Recording stopped. {} actions captured. Review or save.",
                    events.borrow().len()
                ));
            } else {
                // Start recording
                events.borrow_mut().clear();
                text_view.buffer().set_text("");
                seq_preview.set_text("Recording in progress…");

                if bg_switch.is_active() {
                    match crate::playback::BackgroundRecorder::start() {
                        Ok((rx, rec)) => {
                            *bg_recorder.borrow_mut() = Some(rec);
                            b.set_label("■  Stop recording");
                            b.add_css_class("danger");
                            rec_banner.set_text("● Background recording active (global) — switch to any game or app and type.");
                            rec_banner.set_visible(true);
                            status.set_text("Recording system-wide keystrokes.");

                            let bg_recorder = bg_recorder.clone();
                            let events = events.clone();
                            let text_view = text_view.clone();
                            let seq_preview = seq_preview.clone();
                            let rec_banner = rec_banner.clone();
                            glib::timeout_add_local(Duration::from_millis(30), move || {
                                while let Ok(ev) = rx.try_recv() {
                                    events.borrow_mut().push(ev);
                                    text_view.buffer().set_text(&macro_text(&events.borrow()));
                                    seq_preview.set_text(&visual_sequence_summary(&events.borrow()));
                                    rec_banner.set_text(&format!(
                                        "● Recording active (global) — {} actions captured. Click Stop when done.",
                                        events.borrow().len()
                                    ));
                                }
                                if bg_recorder.borrow().is_none() {
                                    glib::ControlFlow::Break
                                } else {
                                    glib::ControlFlow::Continue
                                }
                            });
                        }
                        Err(e) => {
                            // Fallback to in-window capture if evdev access fails
                            *in_win_rec.borrow_mut() = Some(std::time::Instant::now());
                            window.add_controller(capture.clone());
                            b.set_label("■  Stop recording");
                            b.add_css_class("danger");
                            rec_banner.set_text(&format!(
                                "● Recording in this window only ({e}). Click Stop when done."
                            ));
                            rec_banner.set_visible(true);
                            status.set_text("Recording in this window only.");
                        }
                    }
                } else {
                    *in_win_rec.borrow_mut() = Some(std::time::Instant::now());
                    window.add_controller(capture.clone());
                    b.set_label("■  Stop recording");
                    b.add_css_class("danger");
                    rec_banner.set_text("● Recording keys in this window. Click Stop when finished.");
                    rec_banner.set_visible(true);
                    status.set_text("Recording keys in this window.");
                }
            }
        });
    }

    // Window close cleanup
    {
        let (player, window, capture, in_win_rec, bg_recorder) = (
            player.clone(),
            window.clone(),
            capture.clone(),
            in_win_rec.clone(),
            bg_recorder.clone(),
        );
        let w = window.clone();
        window.connect_close_request(move |_| {
            if let Some(mut r) = bg_recorder.borrow_mut().take() {
                r.stop();
            }
            if in_win_rec.borrow().is_some() {
                w.remove_controller(&capture);
            }
            player.borrow_mut().shutdown();
            glib::Propagation::Proceed
        });
    }

    // Playback card play button
    {
        let (s, player, pb_st, repeats, countdown, choices, bg_recorder, in_win_rec) = (
            s.clone(),
            player.clone(),
            playback_status.clone(),
            repeats.clone(),
            countdown.clone(),
            choices.clone(),
            bg_recorder.clone(),
            in_win_rec.clone(),
        );
        play_btn.connect_clicked(move |_| {
            if bg_recorder.borrow().is_some() || in_win_rec.borrow().is_some() {
                pb_st.set_text("Stop recording before playback.");
                return;
            }
            let Some(mac) = current(&s).macros.get(choices.selected() as usize).cloned() else {
                pb_st.set_text("Save a macro first, then select it here.");
                return;
            };
            play_macro_sequence(
                &player,
                mac,
                repeats.value_as_int() as u32,
                countdown.value_as_int() as u32,
                &pb_st,
            );
        });
    }

    // Playback card stop button
    {
        let player = player.clone();
        stop_btn.connect_clicked(move |_| player.borrow().stop());
    }

    page
}

fn play_macro_sequence(
    player: &Rc<RefCell<crate::playback::Player>>,
    mac: Macro,
    repeats: u32,
    countdown: u32,
    playback_status: &gtk::Label,
) {
    if player.borrow().is_running() {
        player.borrow().stop();
    }
    match player.borrow_mut().start(mac, repeats, countdown) {
        Err(e) => playback_status.set_text(&format!("{e:#}")),
        Ok(rx) => {
            playback_status.set_text("Preparing playback…");
            let st = playback_status.clone();
            glib::timeout_add_local(Duration::from_millis(40), move || {
                loop {
                    match rx.try_recv() {
                        Ok(crate::playback::Progress::Countdown(n)) => {
                            st.set_text(&format!(
                                "Starting in {n}s — switch to your destination window now."
                            ));
                        }
                        Ok(crate::playback::Progress::Playing(n, total)) => {
                            st.set_text(&format!("Playing repeat {n} of {total}…"));
                        }
                        Ok(crate::playback::Progress::Finished(result)) => {
                            st.set_text(&match result {
                                Ok(true) => "Playback finished successfully.".into(),
                                Ok(false) => "Playback stopped. Held keys released.".into(),
                                Err(e) => e,
                            });
                            return glib::ControlFlow::Break;
                        }
                        Err(mpsc::TryRecvError::Empty) => {
                            return glib::ControlFlow::Continue;
                        }
                        Err(mpsc::TryRecvError::Disconnected) => {
                            return glib::ControlFlow::Break;
                        }
                    }
                }
            });
        }
    }
}

fn refresh_macros(ctx: &MacroEditorWidgets) {
    while let Some(c) = ctx.list.first_child() {
        ctx.list.remove(&c);
    }
    let p = current(&ctx.s);
    if p.macros.is_empty() {
        let empty_box = column(6);
        empty_box.set_margin_top(12);
        empty_box.set_margin_bottom(12);
        let empty = label(
            "No macros saved yet. Use the editor below to record or create your first macro.",
            "muted",
        );
        empty.set_halign(gtk::Align::Center);
        empty_box.append(&empty);
        ctx.list.append(&empty_box);
        return;
    }

    for (i, m) in p.macros.iter().enumerate() {
        let r = row(12);
        r.add_css_class("macro-row");

        let badge = label(&format!("{:02}", i + 1), "macro-badge");
        r.append(&badge);

        let info = column(2);
        info.set_hexpand(true);
        let title_lbl = label(&m.name, "section-title");
        let desc_lbl = label(&model::event_summary(&m.events), "macro-keys");
        info.append(&title_lbl);
        info.append(&desc_lbl);
        r.append(&info);

        let actions = row(6);
        let play_btn = button("▶ Play", false);
        play_btn.add_css_class("action-small");
        let edit_btn = button("✎ Edit", false);
        edit_btn.add_css_class("action-small");
        let del_btn = button("✕ Delete", false);
        del_btn.add_css_class("action-small");
        del_btn.add_css_class("danger");

        // Play handler
        {
            let player = ctx.player.clone();
            let pb_st = ctx.playback_status.clone();
            let mac = m.clone();
            let rep = ctx.repeats.value_as_int() as u32;
            let cd = ctx.countdown.value_as_int() as u32;
            play_btn.connect_clicked(move |_| {
                play_macro_sequence(&player, mac.clone(), rep, cd, &pb_st);
            });
        }

        // Edit handler
        {
            let ctx = ctx.clone();
            let mac = m.clone();
            edit_btn.connect_clicked(move |_| {
                *ctx.editing_index.borrow_mut() = Some(i);
                ctx.name_entry.set_text(&mac.name);
                ctx.text_view.buffer().set_text(&macro_text(&mac.events));
                *ctx.events.borrow_mut() = mac.events.clone();
                ctx.editor_title
                    .set_text(&format!("EDITING MACRO: {}", mac.name));
                ctx.save_btn.set_label("Update Macro");
                ctx.save_as_new_btn.set_visible(true);
                ctx.seq_preview
                    .set_text(&visual_sequence_summary(&mac.events));
                ctx.status
                    .set_text(&format!("Loaded '{}' into editor.", mac.name));
            });
        }

        // Delete handler
        {
            let ctx = ctx.clone();
            let mac_name = m.name.clone();
            del_btn.connect_clicked(move |_| {
                let mut p = current(&ctx.s);
                if i < p.macros.len() {
                    p.macros.remove(i);
                    // Remap assignments referencing macros
                    let mut keys_to_remove = Vec::new();
                    for (&id, assignment) in p.assignments.iter_mut() {
                        if assignment[0] == 0x70 {
                            let assigned = assignment[1] as usize;
                            if assigned == i {
                                keys_to_remove.push(id);
                            } else if assigned > i {
                                assignment[1] -= 1;
                            }
                        }
                    }
                    for id in keys_to_remove {
                        p.assignments.remove(&id);
                    }
                    edit(&ctx.s, |old| *old = p);
                    let _ = ctx.s.borrow().library.save();
                    ctx.s.borrow_mut().revision += 1;

                    let mut cur_edit = ctx.editing_index.borrow_mut();
                    if *cur_edit == Some(i) {
                        *cur_edit = None;
                        drop(cur_edit);
                        reset_macro_editor(&ctx);
                    } else if let Some(ref mut idx) = *cur_edit
                        && *idx > i
                    {
                        *idx -= 1;
                    }
                    ctx.status.set_text(&format!("Macro '{mac_name}' removed."));
                    refresh_macros(&ctx);
                }
            });
        }

        actions.append(&play_btn);
        actions.append(&edit_btn);
        actions.append(&del_btn);
        r.append(&actions);

        ctx.list.append(&r);
    }
}

fn reset_macro_editor(ctx: &MacroEditorWidgets) {
    *ctx.editing_index.borrow_mut() = None;
    ctx.name_entry.set_text("");
    ctx.name_entry
        .set_placeholder_text(Some("Macro name (e.g. Rapid Fire)"));
    ctx.text_view.buffer().set_text("");
    ctx.events.borrow_mut().clear();
    ctx.editor_title.set_text("CREATE NEW MACRO");
    ctx.save_btn.set_label("Save Macro");
    ctx.save_as_new_btn.set_visible(false);
    ctx.seq_preview.set_text("No actions recorded yet.");
}

fn deduplicate_macro_name(existing: &[Macro], name: &str, exclude_idx: Option<usize>) -> String {
    let trimmed = name.trim();
    let base_name = if trimmed.is_empty() {
        "Macro".to_string()
    } else {
        trimmed.to_string()
    };

    let name_taken = |n: &str| {
        existing.iter().enumerate().any(|(i, m)| {
            if Some(i) == exclude_idx {
                false
            } else {
                m.name.eq_ignore_ascii_case(n)
            }
        })
    };

    if !name_taken(&base_name) {
        return base_name;
    }

    for counter in 2..=250 {
        let candidate = format!("{base_name} ({counter})");
        if !name_taken(&candidate) {
            return candidate;
        }
    }

    format!(
        "{base_name} ({})",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    )
}

fn visual_sequence_summary(events: &[MacroEvent]) -> String {
    if events.is_empty() {
        return "No actions recorded yet.".into();
    }
    let total_ms: u32 = events.iter().map(|e| e.delay_ms as u32).sum();
    let mut parts = Vec::new();
    for e in events.iter().take(10) {
        let name = model::usage_name(e.usage);
        let arrow = if e.down { "↓" } else { "↑" };
        parts.push(format!("{name}{arrow} ({}ms)", e.delay_ms));
    }
    if events.len() > 10 {
        parts.push(format!("… +{} more", events.len() - 10));
    }
    format!(
        "{} actions ({}ms) · {}",
        events.len(),
        total_ms,
        parts.join(" → ")
    )
}

fn macro_text(events: &[MacroEvent]) -> String {
    events
        .iter()
        .map(|e| {
            format!(
                "{} {} {}",
                if e.down { "down" } else { "up" },
                e.usage,
                e.delay_ms
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn parse_macro(text: &str) -> anyhow::Result<Vec<MacroEvent>> {
    let mut raw_events = Vec::new();
    for (i, line) in text.lines().filter(|l| !l.trim().is_empty()).enumerate() {
        let p: Vec<_> = line.split_whitespace().collect();
        anyhow::ensure!(
            p.len() == 3 && matches!(p[0], "down" | "up"),
            "Line {}: use 'down <key> <delay>' or 'up <key> <delay>'",
            i + 1
        );
        let usage = if let Ok(u) = p[1].parse::<u8>() {
            u
        } else {
            let target = p[1].to_ascii_lowercase();
            let mut found = None;
            for u in 4..=231 {
                if model::usage_name(u).to_ascii_lowercase() == target {
                    found = Some(u);
                    break;
                }
            }
            found.ok_or_else(|| anyhow::anyhow!("Line {}: unknown key '{}'", i + 1, p[1]))?
        };
        let delay_ms: u16 = p[2]
            .parse()
            .context(format!("Line {}: invalid delay", i + 1))?;
        raw_events.push(MacroEvent {
            down: p[0] == "down",
            usage,
            delay_ms,
        });
    }
    let sanitized = model::sanitize_events(&raw_events);
    anyhow::ensure!(!sanitized.is_empty(), "Macro has no valid key actions");
    Ok(sanitized)
}

fn gdk_usage(key: gdk::Key) -> Option<u8> {
    if let Some(c) = key.to_unicode() {
        let c = c.to_ascii_lowercase();
        if c.is_ascii_lowercase() {
            return Some(c as u8 - b'a' + 4);
        }
        if ('1'..='9').contains(&c) {
            return Some(c as u8 - b'1' + 0x1e);
        }
        if c == '0' {
            return Some(0x27);
        }
    }
    match key {
        gdk::Key::Return => Some(0x28),
        gdk::Key::Escape => Some(0x29),
        gdk::Key::BackSpace => Some(0x2a),
        gdk::Key::Tab => Some(0x2b),
        gdk::Key::space => Some(0x2c),
        gdk::Key::minus => Some(0x2d),
        gdk::Key::equal => Some(0x2e),
        gdk::Key::bracketleft => Some(0x2f),
        gdk::Key::bracketright => Some(0x30),
        gdk::Key::backslash => Some(0x31),
        gdk::Key::semicolon => Some(0x33),
        gdk::Key::apostrophe => Some(0x34),
        gdk::Key::grave => Some(0x35),
        gdk::Key::comma => Some(0x36),
        gdk::Key::period => Some(0x37),
        gdk::Key::slash => Some(0x38),
        gdk::Key::Caps_Lock => Some(0x39),
        gdk::Key::F1 => Some(0x3a),
        gdk::Key::F2 => Some(0x3b),
        gdk::Key::F3 => Some(0x3c),
        gdk::Key::F4 => Some(0x3d),
        gdk::Key::F5 => Some(0x3e),
        gdk::Key::F6 => Some(0x3f),
        gdk::Key::F7 => Some(0x40),
        gdk::Key::F8 => Some(0x41),
        gdk::Key::F9 => Some(0x42),
        gdk::Key::F10 => Some(0x43),
        gdk::Key::F11 => Some(0x44),
        gdk::Key::F12 => Some(0x45),
        gdk::Key::Print => Some(0x46),
        gdk::Key::Scroll_Lock => Some(0x47),
        gdk::Key::Pause => Some(0x48),
        gdk::Key::Insert => Some(0x49),
        gdk::Key::Home => Some(0x4a),
        gdk::Key::Page_Up => Some(0x4b),
        gdk::Key::Delete => Some(0x4c),
        gdk::Key::End => Some(0x4d),
        gdk::Key::Page_Down => Some(0x4e),
        gdk::Key::Right => Some(0x4f),
        gdk::Key::Left => Some(0x50),
        gdk::Key::Down => Some(0x51),
        gdk::Key::Up => Some(0x52),
        gdk::Key::Control_L => Some(0xe0),
        gdk::Key::Shift_L => Some(0xe1),
        gdk::Key::Alt_L => Some(0xe2),
        gdk::Key::Super_L => Some(0xe3),
        gdk::Key::Control_R => Some(0xe4),
        gdk::Key::Shift_R => Some(0xe5),
        gdk::Key::Alt_R => Some(0xe6),
        gdk::Key::Super_R => Some(0xe7),
        _ => None,
    }
}

fn profiles_page(s: &Shared, status: &gtk::Label, window: &gtk::ApplicationWindow) -> gtk::Box {
    let page = column(24);
    let c = card(
        "Profile library",
        "Profiles are stored locally. Applying settings to the keyboard is always a separate action.",
    );
    let names = gtk::StringList::new(
        &s.borrow()
            .library
            .profiles
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>(),
    );
    let select = gtk::DropDown::new(Some(names.clone()), None::<gtk::Expression>);
    select.set_selected(s.borrow().library.selected as u32);
    c.append(&select);
    let entry = gtk::Entry::new();
    entry.set_text(&current(s).name);
    c.append(&entry);
    {
        let (s, entry, st) = (s.clone(), entry.clone(), status.clone());
        select.connect_selected_notify(move |d| {
            let i = d.selected() as usize;
            if i < s.borrow().library.profiles.len() {
                {
                    let mut state = s.borrow_mut();
                    state.library.selected = i;
                    state.revision += 1;
                }
                entry.set_text(&current(&s).name);
                st.set_text("Profile selected. Editors updated.");
            }
        });
    }
    let actions = row(10);
    for title in ["Save / rename", "Duplicate", "Delete"] {
        let b = button(title, title == "Save / rename");
        let (s, names, entry, select, st) = (
            s.clone(),
            names.clone(),
            entry.clone(),
            select.clone(),
            status.clone(),
        );
        b.connect_clicked(move |_| {
            let result = (|| -> anyhow::Result<()> {
                match title {
                    "Save / rename" => {
                        let name = entry.text().to_string();
                        anyhow::ensure!(
                            !name.trim().is_empty() && name.len() <= 80,
                            "Use a name of 1–80 characters"
                        );
                        edit(&s, |p| p.name = name);
                    }
                    "Duplicate" => {
                        let mut p = current(&s);
                        p.name = format!("{} copy", p.name);
                        let mut state = s.borrow_mut();
                        state.library.profiles.push(p);
                        state.library.selected = state.library.profiles.len() - 1;
                    }
                    _ => {
                        anyhow::ensure!(
                            s.borrow().library.profiles.len() > 1,
                            "Keep at least one profile"
                        );
                        let mut state = s.borrow_mut();
                        let i = state.library.selected;
                        state.library.profiles.remove(i);
                        state.library.selected = 0;
                    }
                }
                let lib = s.borrow().library.clone();
                let labels: Vec<&str> = lib.profiles.iter().map(|p| p.name.as_str()).collect();
                names.splice(0, names.n_items(), &labels);
                select.set_selected(lib.selected as u32);
                entry.set_text(&lib.profiles[lib.selected].name);
                lib.save()
            })();
            status_set(&st, result, "Profile library saved.");
        });
        actions.append(&b);
    }
    c.append(&actions);
    page.append(&c);
    let share = card(
        "Take your setup with you",
        "Import Windows keyboard settings or portable Phantom Studio JSON. Export your Linux profiles.",
    );
    let actions = row(10);
    for importing in [true, false] {
        let b = button(
            if importing {
                "Import profile"
            } else {
                "Export profile"
            },
            false,
        );
        let (s, st, w, names) = (s.clone(), status.clone(), window.clone(), names.clone());
        b.connect_clicked(move |_| {
            let dialog = gtk::FileDialog::builder()
                .title(if importing {
                    "Import profile"
                } else {
                    "Export profile"
                })
                .initial_name("phantom-profile.json")
                .build();
            let (s, st, names) = (s.clone(), st.clone(), names.clone());
            let done = move |file: Result<gtk::gio::File, glib::Error>| {
                if let Ok(file) = file
                    && let Some(path) = file.path()
                {
                    let result = (|| -> anyhow::Result<()> {
                        if importing {
                            let p = model::import_profile(&path)?;
                            names.append(&p.name);
                            s.borrow_mut().library.profiles.push(p);
                            s.borrow().library.save()?;
                        } else {
                            model::atomic_json(&path, &current(&s))?;
                        }
                        Ok(())
                    })();
                    status_set(&st, result, "Profile file saved / imported.");
                }
            };
            if importing {
                dialog.open(Some(&w), None::<&gtk::gio::Cancellable>, done);
            } else {
                dialog.save(Some(&w), None::<&gtk::gio::Cancellable>, done);
            }
        });
        actions.append(&b);
    }
    share.append(&actions);
    page.append(&share);
    page
}

fn device_page(
    s: &Shared,
    tx: &mpsc::Sender<Job>,
    status: &gtk::Label,
    window: &gtk::ApplicationWindow,
) -> gtk::Box {
    let page = column(24);
    let c = card("Phantom TKL", "USB 320f:505b  ·  Evision RGB Keyboard");
    let details = label("Waiting for device information…", "muted");
    details.set_wrap(true);
    c.append(&details);
    {
        let (s, d) = (s.clone(), details.clone());
        glib::timeout_add_local(Duration::from_secs(1), move || {
            if let Some(snapshot) = &s.borrow().snapshot {
                d.set_text(&format!("Vendor HID interface connected\n{} firmware key slots · {}-byte reports\nSettings are backed up before lighting changes.",snapshot.info[5],64));
            }
            glib::ControlFlow::Continue
        });
    }
    let read = button("Read lighting from keyboard", false);
    {
        let (s, tx, st) = (s.clone(), tx.clone(), status.clone());
        read.connect_clicked(move |_| start_job(&s, &tx, &st, Job::ReadLighting));
    }
    c.append(&read);
    page.append(&c);
    let system = card("Gaming preferences", "Stored on the keyboard.");
    for (title, field) in [
        ("Lock Super / Windows key", 0),
        ("Use six-key rollover", 1),
        ("Swap WASD and arrow keys", 2),
    ] {
        let r = row(10);
        let l = label(title, "");
        l.set_hexpand(true);
        r.append(&l);
        let sw = gtk::Switch::new();
        let p = current(s);
        sw.set_active(match field {
            0 => p.win_lock,
            1 => p.six_key_rollover,
            _ => p.swap_wasd,
        });
        {
            let s = s.clone();
            sw.connect_active_notify(move |w| {
                edit(&s, |p| match field {
                    0 => p.win_lock = w.is_active(),
                    1 => p.six_key_rollover = w.is_active(),
                    _ => p.swap_wasd = w.is_active(),
                })
            });
        }
        {
            let w = sw.clone();
            on_profile_change(s, &sw, move |p| {
                w.set_active(match field {
                    0 => p.win_lock,
                    1 => p.six_key_rollover,
                    _ => p.swap_wasd,
                })
            });
        }
        r.append(&sw);
        system.append(&r);
    }
    system.append(&label("USB polling rate", "muted"));
    let polling = gtk::DropDown::from_strings(&["1000 Hz", "500 Hz", "250 Hz", "125 Hz"]);
    polling.set_selected(current(s).polling_rate as u32);
    {
        let s = s.clone();
        polling.connect_selected_notify(move |w| edit(&s, |p| p.polling_rate = w.selected() as u8));
    }
    {
        let w = polling.clone();
        on_profile_change(s, &polling, move |p| w.set_selected(p.polling_rate as u32));
    }
    system.append(&polling);
    let apply = button("Apply device settings", true);
    {
        let (s, tx, st) = (s.clone(), tx.clone(), status.clone());
        apply.connect_clicked(move |_| start_job(&s, &tx, &st, Job::System(current(&s))));
    }
    let unlock_win = button("Unlock Super / Windows key", false);
    {
        let (s, tx, st) = (s.clone(), tx.clone(), status.clone());
        unlock_win.connect_clicked(move |_| {
            edit(&s, |p| p.win_lock = false);
            s.borrow_mut().revision += 1;
            start_job(&s, &tx, &st, Job::System(current(&s)));
        });
    }
    let btn_row = row(10);
    btn_row.append(&apply);
    btn_row.append(&unlock_win);
    system.append(&btn_row);
    page.append(&system);
    let appearance = card("Appearance", "Let a little of your desktop through.");
    let frost = gtk::CheckButton::with_label("Frosted background");
    frost.set_active(window.has_css_class("frosted"));
    {
        let (window, status) = (window.clone(), status.clone());
        frost.connect_toggled(move |w| {
            if w.is_active() {
                window.add_css_class("frosted");
            } else {
                window.remove_css_class("frosted");
            }
            status_set(
                &status,
                model::atomic_json(&model::config_dir().join("appearance.json"), &w.is_active()),
                "Appearance saved.",
            );
        });
    }
    appearance.append(&frost);
    let note = label(
        "Uses native compositor blur when configured. Text and controls stay sharp. Turn off for an opaque background.",
        "muted",
    );
    note.set_wrap(true);
    appearance.append(&note);
    page.append(&appearance);
    let ver = label(&format!("v{}", env!("CARGO_PKG_VERSION")), "subtle");
    ver.set_halign(gtk::Align::Center);
    page.append(&ver);
    page
}

#[cfg(test)]
mod tests {
    use super::*;
    fn descendants(w: &gtk::Widget, out: &mut Vec<gtk::Widget>) {
        out.push(w.clone());
        let mut child = w.first_child();
        while let Some(c) = child {
            descendants(&c, out);
            child = c.next_sibling();
        }
    }

    #[test]
    #[ignore = "Requires a GTK display; does not open or write to a keyboard"]
    fn lighting_editor_regression() {
        gtk::init().unwrap();
        let s = Rc::new(RefCell::new(State {
            library: Library::default(),
            selected: None,
            snapshot: Some(
                serde_json::from_str(include_str!("../tests/fixtures/phantom-320f-505b.json"))
                    .unwrap(),
            ),
            busy: false,
            revision: 0,
        }));
        let (tx, rx) = mpsc::channel();
        let page = lighting_page(&s, &tx, &gtk::Label::new(None), &keyboard_view(&s, false));
        let mut widgets = vec![];
        descendants(page.upcast_ref(), &mut widgets);
        let find_button = |title: &str| {
            widgets
                .iter()
                .find_map(|w| {
                    w.clone()
                        .downcast::<gtk::Button>()
                        .ok()
                        .filter(|b| b.label().as_deref() == Some(title))
                })
                .unwrap()
        };
        let modes = widgets
            .iter()
            .find_map(|w| w.clone().downcast::<gtk::DropDown>().ok())
            .unwrap();
        let picker = find_button("Open color picker");
        for (i, &(mode, _)) in model::EFFECTS.iter().enumerate() {
            modes.set_selected(i as u32);
            assert_eq!(
                picker.is_sensitive(),
                !matches!(mode, 4 | 11 | 12 | 17 | 18)
            );
        }
        find_button("Paint all keys").emit_clicked();
        let p = current(&s);
        assert_eq!(p.lighting.mode, 20);
        assert_eq!(p.colors.len(), 86);
        assert!(!p.colors.contains_key(&13));
        p.validate().unwrap();
        find_button("Apply lighting  ↗").emit_clicked();
        let Job::Lighting(p) = rx.try_recv().unwrap() else {
            panic!("Expected lighting job")
        };
        assert_eq!(p.colors.len(), 86);
        s.borrow_mut().snapshot = None;
        find_button("Discard staged painting").emit_clicked();
        find_button("Paint all keys").emit_clicked();
        assert!(!current(&s).colors.contains_key(&13));
    }
}
