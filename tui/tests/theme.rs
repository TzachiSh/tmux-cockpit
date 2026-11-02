//! `Theme::load` reads the plugin's palette roles, and the light set when the
//! server's `theme` option says `light`, through a fake tmux.
use cockpit_tui::theme::Theme;
use ratatui::style::Color;
use std::sync::Mutex;

static ENV: Mutex<()> = Mutex::new(());

fn load(options: &str) -> Theme {
    let _guard = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join(format!("cockpit-theme-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("options");
    std::fs::write(&out, options).unwrap();
    let bin = dir.join("tmux");
    std::fs::write(&bin, format!("#!/bin/sh\ncat '{}'\n", out.display())).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    // SAFETY: every test here takes ENV first, so no other thread reads these.
    unsafe {
        std::env::remove_var("NO_COLOR");
        std::env::set_var("COCKPIT_TMUX", &bin);
    }
    let th = Theme::load();
    unsafe { std::env::remove_var("COCKPIT_TMUX") };
    std::fs::remove_dir_all(&dir).ok();
    th
}

#[test]
fn the_dark_set_reads_the_role_options() {
    let th = load("@thm_needs \"#010203\"\n@thm_needs_light \"#040506\"\ntheme detect\n");
    assert_eq!(th.needs, Color::Rgb(1, 2, 3));
    // An unset role takes the shipped dark default.
    assert_eq!(th.value, Color::Rgb(0xe6, 0xe4, 0xdf));
}

#[test]
fn theme_light_reads_the_light_set() {
    let th = load("@thm_needs \"#010203\"\n@thm_needs_light \"#040506\"\ntheme light\n");
    assert_eq!(th.needs, Color::Rgb(4, 5, 6));
    assert_eq!(th.value, Color::Rgb(0x1b, 0x1e, 0x24));
}

#[test]
fn meta_reads_the_faint_role_apart_from_label() {
    let th = load("@thm_faint \"#0a0b0c\"\ntheme detect\n");
    assert_eq!(th.meta, Color::Rgb(10, 11, 12));
    // label stays dim: a frame in label is never drawn in meta.
    assert_eq!(th.label, Color::Rgb(0x90, 0x97, 0xa3));
    assert_ne!(Theme::fixed().meta, Theme::fixed().label);
}
