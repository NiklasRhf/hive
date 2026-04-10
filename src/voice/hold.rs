use std::sync::atomic::{AtomicBool, Ordering};

pub static HOLD_PRESSED: AtomicBool = AtomicBool::new(false);

#[cfg(target_os = "linux")]
pub fn spawn(key_name: &str) {
    let key = match resolve_key(key_name) {
        Some(k) => k,
        None => {
            eprintln!("hive voice: unknown hold key {key_name:?} — hold-to-talk disabled");
            return;
        }
    };
    std::thread::spawn(move || run(key));
}

#[cfg(not(target_os = "linux"))]
pub fn spawn(_key_name: &str) {
    eprintln!("hive voice: hold-to-talk listener only supported on linux");
}

#[cfg(target_os = "linux")]
fn run(target: evdev::Key) {
    use evdev::{Device, InputEventKind};

    let keyboards: Vec<(std::path::PathBuf, Device)> = evdev::enumerate()
        .filter(|(_, d)| {
            d.supported_keys()
                .map(|k| k.contains(target))
                .unwrap_or(false)
        })
        .collect();

    if keyboards.is_empty() {
        eprintln!(
            "hive voice: no input device exposes {target:?} — hold-to-talk disabled \
             (are you in the `input` group?)"
        );
        return;
    }

    eprintln!(
        "hive voice: hold-to-talk armed on {:?} across {} device(s)",
        target,
        keyboards.len()
    );

    for (path, mut dev) in keyboards {
        std::thread::spawn(move || loop {
            match dev.fetch_events() {
                Ok(events) => {
                    for ev in events {
                        if let InputEventKind::Key(k) = ev.kind() {
                            if k == target {
                                match ev.value() {
                                    1 => HOLD_PRESSED.store(true, Ordering::SeqCst),
                                    0 => HOLD_PRESSED.store(false, Ordering::SeqCst),
                                    _ => {}
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    eprintln!(
                        "hive voice: evdev fetch error on {}: {e} — retrying",
                        path.display()
                    );
                    std::thread::sleep(std::time::Duration::from_secs(1));
                }
            }
        });
    }
}

#[cfg(target_os = "linux")]
fn resolve_key(name: &str) -> Option<evdev::Key> {
    use evdev::Key;
    let upper = name.trim().to_ascii_uppercase();
    Some(match upper.as_str() {
        "F1" => Key::KEY_F1,
        "F2" => Key::KEY_F2,
        "F3" => Key::KEY_F3,
        "F4" => Key::KEY_F4,
        "F5" => Key::KEY_F5,
        "F6" => Key::KEY_F6,
        "F7" => Key::KEY_F7,
        "F8" => Key::KEY_F8,
        "F9" => Key::KEY_F9,
        "F10" => Key::KEY_F10,
        "F11" => Key::KEY_F11,
        "F12" => Key::KEY_F12,
        "CAPSLOCK" | "CAPS" => Key::KEY_CAPSLOCK,
        "RIGHTCTRL" | "RCTRL" => Key::KEY_RIGHTCTRL,
        "LEFTCTRL" | "LCTRL" => Key::KEY_LEFTCTRL,
        "RIGHTALT" | "RALT" => Key::KEY_RIGHTALT,
        "LEFTALT" | "LALT" => Key::KEY_LEFTALT,
        "RIGHTSHIFT" | "RSHIFT" => Key::KEY_RIGHTSHIFT,
        "LEFTSHIFT" | "LSHIFT" => Key::KEY_LEFTSHIFT,
        "SPACE" => Key::KEY_SPACE,
        "ENTER" => Key::KEY_ENTER,
        "TAB" => Key::KEY_TAB,
        "ESC" | "ESCAPE" => Key::KEY_ESC,
        "PAUSE" => Key::KEY_PAUSE,
        "SCROLLLOCK" => Key::KEY_SCROLLLOCK,
        "INSERT" => Key::KEY_INSERT,
        _ => return None,
    })
}
