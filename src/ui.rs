use crate::{
    device::{Keyboard, Snapshot},
    model::{self, Library, Macro, MacroEvent, Profile},
};
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
fn heading(title: &str, sub: &str) -> gtk::Box {
    let b = column(8);
    b.append(&label(title, "page-title"));
    b.append(&label(sub, "muted"));
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
    let (library, load_error) = match Library::load() {
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
        .default_width(1100)
        .default_height(780)
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
    header.set_title_widget(Some(&label("PHANTOM STUDIO", "eyebrow")));
    window.set_titlebar(Some(&header));
    let root = column(0);
    let body = row(0);
    body.set_vexpand(true);
    let sidebar = column(10);
    sidebar.add_css_class("sidebar");
    sidebar.set_width_request(178);
    sidebar.append(&label("PHANTOM", "brand"));
    sidebar.append(&label("KEYBOARD STUDIO", "eyebrow"));
    let gap = column(0);
    gap.set_height_request(26);
    sidebar.append(&gap);
    let stack = gtk::Stack::new();
    stack.set_transition_type(gtk::StackTransitionType::Crossfade);
    stack.set_transition_duration(160);
    stack.set_hexpand(true);
    stack.set_vexpand(true);
    let status = label(
        load_error
            .as_deref()
            .unwrap_or("Ready. Connect your keyboard to get started."),
        "status",
    );
    status.set_ellipsize(gtk::pango::EllipsizeMode::End);
    status.set_hexpand(true);
    let badge = label("NOT CONNECTED", "pill");
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
    sidebar.append(&label("YOUR DEVICE", "eyebrow"));
    sidebar.append(&label("Phantom TKL", "section-title"));
    sidebar.append(&label("87 keys  ·  RGB", "muted"));
    sidebar.append(&badge);
    let refresh = button("Reconnect", false);
    {
        let (s, tx, st) = (state.clone(), job_tx.clone(), status.clone());
        refresh.connect_clicked(move |_| start_job(&s, &tx, &st, Job::Probe));
    }
    sidebar.append(&refresh);
    sidebar.append(&label("LINUX EDITION  /  0.1", "subtle"));
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
        page.set_margin_top(28);
        page.set_margin_bottom(28);
        page.set_margin_start(32);
        page.set_margin_end(32);
        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&page)
            .build();
        stack.add_named(&scroll, Some(name));
    }
    body.append(&sidebar);
    body.append(&stack);
    root.append(&body);
    let footer = row(12);
    footer.add_css_class("statusbar");
    footer.append(&status);
    footer.append(&label("GTK 4  ·  Rust", "subtle"));
    root.append(&footer);
    window.set_child(Some(&root));
    {
        let (s, st, badge, k) = (
            state.clone(),
            status.clone(),
            badge.clone(),
            keyboard.clone(),
        );
        glib::timeout_add_local(Duration::from_millis(80), move || {
            while let Ok(done) = done_rx.try_recv() {
                s.borrow_mut().busy = false;
                match done.result {
                    Ok(snapshot) => {
                        if let Some(snapshot) = snapshot {
                            if done.read_into_profile {
                                let p = snapshot.profile();
                                edit(&s, |old| {
                                    old.lighting = p.lighting;
                                    old.colors = p.colors;
                                    old.win_lock = p.win_lock;
                                    old.six_key_rollover = p.six_key_rollover;
                                    old.swap_wasd = p.swap_wasd;
                                    old.polling_rate = p.polling_rate;
                                });
                                s.borrow_mut().revision += 1;
                            }
                            s.borrow_mut().snapshot = Some(snapshot);
                        }
                        badge.set_text("●  CONNECTED");
                        badge.add_css_class("connected");
                        st.set_text(done.message);
                    }
                    Err(e) => {
                        st.set_text(&e);
                        badge.set_text("CHECK CONNECTION");
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
    // Reproducible visual QA without a screenshot utility or desktop capture permissions.
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

fn keyboard_view(state: &Shared, select: bool) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_content_width(700);
    area.set_content_height(285);
    area.set_hexpand(true);
    {
        let state = state.clone();
        area.set_draw_func(move |_, cr, w, h| {
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
                let color = if p.lighting.mode != 20 {
                    p.lighting.color
                } else {
                    p.colors.get(&key.id).copied().unwrap_or_else(|| {
                        s.snapshot
                            .as_ref()
                            .filter(|snap| snap.settings[25] == p.lighting.custom_slot)
                            .and_then(|snap| {
                                snap.led_index(&key)
                                    .and_then(|led| snap.custom_colors.get(led * 3..led * 3 + 3))
                            })
                            .map(|rgb| [rgb[0], rgb[1], rgb[2]])
                            .unwrap_or([0, 0, 0])
                    })
                };
                let (r, g, b) = if (p.lighting.rainbow
                    || !model::effect_controls(p.lighting.mode).color)
                    && p.lighting.mode != 20
                {
                    let t = key.x / 888.0;
                    (0.5 + 0.35 * t, 0.35 + 0.2 * (1.0 - t), 0.95 - 0.3 * t)
                } else {
                    (
                        color[0] as f64 / 255.,
                        color[1] as f64 / 255.,
                        color[2] as f64 / 255.,
                    )
                };
                rounded(cr, key.x, key.y, key.width, key.height, 6.0);
                cr.set_source_rgb(0.07, 0.075, 0.105);
                let _ = cr.fill();
                rounded(cr, key.x, key.y, key.width, key.height - 3., 6.);
                cr.set_source_rgb(0.15 + r * 0.07, 0.15 + g * 0.07, 0.19 + b * 0.07);
                let _ = cr.fill_preserve();
                cr.set_line_width(if selected { 2.5 } else { 1. });
                cr.set_source_rgba(r, g, b, if selected { 1.0 } else { 0.4 });
                let _ = cr.stroke();
                cr.set_source_rgba(r, g, b, 0.6 * p.lighting.brightness as f64 / 4.);
                cr.rectangle(key.x + 8., key.y + key.height - 3., key.width - 16., 1.5);
                let _ = cr.fill();
                cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
                cr.set_font_size(if key.label.len() > 5 { 10. } else { 12. });
                cr.set_source_rgba(0.86, 0.84, 0.93, if available { 1.0 } else { 0.25 });
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
                    cr.set_source_rgb(0.78, 0.65, 1.);
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
    let h = heading(
        "Make it yours.",
        "A little atmosphere. A lot of personality.",
    );
    h.set_hexpand(true);
    top.append(&h);
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
    let hint = label(
        "Preview colors are illustrative. Apply to see the keyboard's native animation.",
        "subtle",
    );
    hint.set_halign(gtk::Align::Center);
    hero.append(&hint);
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
    page.append(&heading(
        "Every key, your way.",
        "Select a key and give it a new purpose.",
    ));
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
    let macro_index = gtk::SpinButton::with_range(1., 250., 1.);
    let mr = row(10);
    mr.append(&label("Macro number", "muted"));
    mr.append(&macro_index);
    let assign_macro = button("Assign macro", false);
    {
        let (s, st) = (s.clone(), status.clone());
        assign_macro.connect_clicked(move |_| {
            let Some(id) = s.borrow().selected else {
                st.set_text("Select a key first.");
                return;
            };
            let n = macro_index.value_as_int() as usize - 1;
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

fn macros_page(s: &Shared, status: &gtk::Label, window: &gtk::ApplicationWindow) -> gtk::Box {
    let player = Rc::new(RefCell::new(crate::playback::Player::default()));
    {
        let player = player.clone();
        window.connect_close_request(move |_| {
            player.borrow_mut().shutdown();
            glib::Propagation::Proceed
        });
    }
    let page = column(24);
    page.append(&heading(
        "One key. More possibilities.",
        "Build precise sequences with editable timing.",
    ));
    let editor = card(
        "Macro editor",
        "One event per line: down or up, USB key usage, delay in milliseconds. Example: down 4 0, then up 4 100.",
    );
    let name = gtk::Entry::builder()
        .placeholder_text("Macro name")
        .text("My macro")
        .build();
    editor.append(&name);
    let text = gtk::TextView::new();
    text.set_monospace(true);
    text.set_height_request(180);
    text.buffer().set_text("down 4 0\nup 4 100");
    editor.append(&text);
    let recording = Rc::new(RefCell::new(None::<std::time::Instant>));
    let events = Rc::new(RefCell::new(Vec::<MacroEvent>::new()));
    let record = button("●  Record keystrokes", false);
    let capture = gtk::EventControllerKey::new();
    capture.set_propagation_phase(gtk::PropagationPhase::Capture);
    {
        let (rec, ev, text) = (recording.clone(), events.clone(), text.clone());
        capture.connect_key_pressed(move |_, key, _, _| {
            let last = *rec.borrow();
            if let Some(last) = last {
                if let Some(usage) = gdk_usage(key) {
                    let delay = last.elapsed().as_millis().min(65535) as u16;
                    *rec.borrow_mut() = Some(std::time::Instant::now());
                    if !ev
                        .borrow()
                        .iter()
                        .rev()
                        .find(|e| e.usage == usage)
                        .is_some_and(|e| e.down)
                    {
                        ev.borrow_mut().push(MacroEvent {
                            delay_ms: delay,
                            usage,
                            down: true,
                        });
                        text.buffer().set_text(&macro_text(&ev.borrow()));
                    }
                }
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
    }
    {
        let (rec, ev, text) = (recording.clone(), events.clone(), text.clone());
        capture.connect_key_released(move |_, key, _, _| {
            let last = *rec.borrow();
            if let Some(last) = last
                && let Some(usage) = gdk_usage(key)
            {
                let delay = last.elapsed().as_millis().min(65535) as u16;
                *rec.borrow_mut() = Some(std::time::Instant::now());
                ev.borrow_mut().push(MacroEvent {
                    delay_ms: delay,
                    usage,
                    down: false,
                });
                text.buffer().set_text(&macro_text(&ev.borrow()));
            }
        });
    }
    window.add_controller(capture);
    {
        let (rec, ev, st, player) = (
            recording.clone(),
            events.clone(),
            status.clone(),
            player.clone(),
        );
        record.connect_clicked(move |b| {
            if player.borrow().is_running() {
                st.set_text("Stop playback before recording.");
                return;
            }
            if rec.borrow().is_some() {
                *rec.borrow_mut() = None;
                b.set_label("●  Record keystrokes");
                st.set_text("Recording stopped. Review events before saving.");
            } else {
                ev.borrow_mut().clear();
                *rec.borrow_mut() = Some(std::time::Instant::now());
                b.set_label("■  Stop recording");
                st.set_text("Recording keys in this window. Click Stop when finished.");
            }
        });
    }
    let list = column(8);
    refresh_macros(&list, &current(s));
    let add = button("Save new macro", true);
    {
        let (s, st, text, list) = (s.clone(), status.clone(), text.clone(), list.clone());
        add.connect_clicked(move |_| {
            let buffer = text.buffer();
            let input = buffer.text(&buffer.start_iter(), &buffer.end_iter(), false);
            let result = (|| -> anyhow::Result<()> {
                let events = parse_macro(&input)?;
                let mac = Macro {
                    name: name.text().to_string(),
                    events,
                };
                anyhow::ensure!(!mac.name.trim().is_empty(), "Give the macro a name");
                let mut p = current(&s);
                p.macros.push(mac);
                p.validate()?;
                edit(&s, |old| *old = p);
                s.borrow().library.save()?;
                refresh_macros(&list, &current(&s));
                s.borrow_mut().revision += 1;
                Ok(())
            })();
            status_set(
                &st,
                result,
                "Macro saved. Assign it to a key on the Key assignments page.",
            );
        });
    }
    let actions = row(10);
    actions.append(&record);
    actions.append(&add);
    editor.append(&actions);
    page.append(&editor);
    let saved = card(
        "Your macro library",
        "Onboard capacity: 7168 bytes. Keystrokes are recorded only inside this window.",
    );
    saved.append(&list);
    {
        let w = list.clone();
        on_profile_change(s, &list, move |p| refresh_macros(&w, p));
    }
    let delete = button("Delete last macro", false);
    {
        let (s, list, st) = (s.clone(), list.clone(), status.clone());
        delete.connect_clicked(move |_| {
            let mut p = current(&s);
            if p.macros.is_empty() {
                return;
            }
            let index = p.macros.len() - 1;
            if p.assignments
                .values()
                .any(|a| a[0] == 0x70 && a[1] as usize == index)
            {
                st.set_text("Remove the key assignment referencing this macro first.");
                return;
            }
            p.macros.pop();
            edit(&s, |old| *old = p);
            refresh_macros(&list, &current(&s));
            s.borrow_mut().revision += 1;
            status_set(&st, s.borrow().library.save(), "Macro removed.");
        });
    }
    saved.append(&delete);
    let playback = card(
        "Play a macro",
        "Choose a saved macro, press Play, then focus the app that should receive the keystrokes during the countdown.",
    );
    let choices = gtk::DropDown::from_strings(&["Save a macro first"]);
    let refresh_choices = {
        let choices = choices.clone();
        move |p: &Profile| {
            let selected = choices
                .selected()
                .min(p.macros.len().saturating_sub(1) as u32);
            let names: Vec<_> = p.macros.iter().map(|m| m.name.as_str()).collect();
            choices.set_model(Some(&gtk::StringList::new(if names.is_empty() {
                &["Save a macro first"]
            } else {
                &names
            })));
            choices.set_selected(selected);
            choices.set_sensitive(!names.is_empty());
        }
    };
    refresh_choices(&current(s));
    on_profile_change(s, &choices, refresh_choices);
    playback.append(&choices);
    let timing = row(12);
    let repeats = gtk::SpinButton::with_range(1., 100., 1.);
    let countdown = gtk::SpinButton::with_range(1., 10., 1.);
    countdown.set_value(3.);
    timing.append(&label("Repeat", "muted"));
    timing.append(&repeats);
    timing.append(&label("Start delay (seconds)", "muted"));
    timing.append(&countdown);
    playback.append(&timing);
    let playback_status = label(
        "Ready. Playback works on Wayland and X11 through a virtual keyboard.",
        "muted",
    );
    playback_status.set_wrap(true);
    playback.append(&playback_status);
    let actions = row(10);
    let play = button("Play selected macro", true);
    let stop = button("Stop playback", false);
    {
        let player = player.clone();
        stop.connect_clicked(move |_| player.borrow().stop());
    }
    {
        let (s, player, st, recording) = (
            s.clone(),
            player.clone(),
            playback_status.clone(),
            recording.clone(),
        );
        play.connect_clicked(move |_| {
            if recording.borrow().is_some() {
                st.set_text("Stop recording before playback.");
                return;
            }
            let Some(mac) = current(&s).macros.get(choices.selected() as usize).cloned() else {
                st.set_text("Save a macro first, then select it here.");
                return;
            };
            match player.borrow_mut().start(
                mac,
                repeats.value_as_int() as u32,
                countdown.value_as_int() as u32,
            ) {
                Err(e) => st.set_text(&format!("{e:#}")),
                Ok(rx) => {
                    st.set_text("Preparing playback…");
                    let st = st.clone();
                    glib::timeout_add_local(Duration::from_millis(40), move || {
                        loop {
                            match rx.try_recv() {
                                Ok(crate::playback::Progress::Countdown(n)) => {
                                    st.set_text(&format!(
                                        "Starting in {n} seconds. Focus the destination app now."
                                    ))
                                }
                                Ok(crate::playback::Progress::Playing(n, total)) => {
                                    st.set_text(&format!("Playing repeat {n} of {total}…"))
                                }
                                Ok(crate::playback::Progress::Finished(result)) => {
                                    st.set_text(&match result {
                                        Ok(true) => "Playback finished.".into(),
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
        });
    }
    actions.append(&play);
    actions.append(&stop);
    playback.append(&actions);
    page.append(&playback);
    page.append(&saved);
    page
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
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .enumerate()
        .map(|(i, line)| {
            let p: Vec<_> = line.split_whitespace().collect();
            anyhow::ensure!(
                p.len() == 3 && matches!(p[0], "down" | "up"),
                "Line {}: use down/up, usage, delay",
                i + 1
            );
            Ok(MacroEvent {
                down: p[0] == "down",
                usage: p[1].parse()?,
                delay_ms: p[2].parse()?,
            })
        })
        .collect()
}
fn refresh_macros(list: &gtk::Box, p: &Profile) {
    while let Some(c) = list.first_child() {
        list.remove(&c);
    }
    for (i, m) in p.macros.iter().enumerate() {
        let r = row(10);
        r.add_css_class("macro-row");
        let l = label(&format!("{:02}   {}", i + 1, m.name), "");
        l.set_hexpand(true);
        r.append(&l);
        r.append(&label(&format!("{} events", m.events.len()), "muted"));
        list.append(&r);
    }
    if p.macros.is_empty() {
        list.append(&label(
            "No macros yet. Create your first sequence above.",
            "muted",
        ));
    }
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
        gdk::Key::Control_L => Some(0xe0),
        gdk::Key::Shift_L => Some(0xe1),
        gdk::Key::Alt_L => Some(0xe2),
        gdk::Key::Super_L => Some(0xe3),
        gdk::Key::Control_R => Some(0xe4),
        gdk::Key::Shift_R => Some(0xe5),
        gdk::Key::Alt_R => Some(0xe6),
        gdk::Key::Left => Some(0x50),
        gdk::Key::Right => Some(0x4f),
        gdk::Key::Up => Some(0x52),
        gdk::Key::Down => Some(0x51),
        _ => None,
    }
}

fn profiles_page(s: &Shared, status: &gtk::Label, window: &gtk::ApplicationWindow) -> gtk::Box {
    let page = column(24);
    page.append(&heading(
        "A setup for every mood.",
        "Keep lighting, assignments, and macros together.",
    ));
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
    page.append(&heading(
        "Meet your Phantom.",
        "Native USB control. No Windows runtime.",
    ));
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
    system.append(&apply);
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
    let info = card(
        "About this Linux edition",
        "Built in Rust with GTK 4. Protocol recovered from your supplied Windows executable using GhidraMCP.",
    );
    let l = label(
        "Lighting and device settings support readback verification. Keymap and macro writes are acknowledged by the device; their behavior needs physical testing. Firmware updates and vendor cloud services are not implemented.",
        "muted",
    );
    l.set_wrap(true);
    info.append(&l);
    page.append(&info);
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
        // Offline editing must not reintroduce the unavailable key either.
        s.borrow_mut().snapshot = None;
        find_button("Discard staged painting").emit_clicked();
        find_button("Paint all keys").emit_clicked();
        assert!(!current(&s).colors.contains_key(&13));
    }
}
