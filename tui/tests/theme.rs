//! `Theme::load` reads the plugin's palette roles, and the light set when the
//! server's `theme` option says `light` or, under `detect`, when the client
//! reports a light theme (#240), through a fake tmux.
mod fake;

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
    fake::write_exec(&bin, &format!("#!/bin/sh\ncat '{}'\n", out.display()));
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
fn faint_is_its_own_role_and_meta_stays_dim() {
    let th = load("@thm_faint \"#0a0b0c\"\ntheme detect\n");
    assert_eq!(th.faint, Color::Rgb(10, 11, 12));
    // meta and label carry text, so they stay on dim (4.5:1).
    assert_eq!(th.meta, Color::Rgb(0x90, 0x97, 0xa3));
    assert_eq!(th.label, Color::Rgb(0x90, 0x97, 0xa3));
    let f = Theme::fixed();
    assert_ne!(f.faint, f.label);
}

#[test]
fn sel_reads_its_option_and_its_light_twin() {
    let th = load("@thm_sel \"#0d0e0f\"\n@thm_sel_light \"#101112\"\ntheme detect\n");
    assert_eq!(th.sel, Color::Rgb(13, 14, 15));
    let th = load("@thm_sel \"#0d0e0f\"\n@thm_sel_light \"#101112\"\ntheme light\n");
    assert_eq!(th.sel, Color::Rgb(16, 17, 18));
    // statusbar.conf's defaults, and nothing at all without colour
    assert_eq!(Theme::fixed().sel, Color::Rgb(0x3a, 0x42, 0x52));
    assert_eq!(Theme::no_color().sel, Color::Reset);
}

// #240: the light set follows the popup's client under `detect`.
const OPTS: &str = "@thm_needs \"#010203\"\n@thm_needs_light \"#040506\"\n";
const DARK: Color = Color::Rgb(1, 2, 3);
const LIGHT: Color = Color::Rgb(4, 5, 6);

#[test]
fn detect_follows_the_client_theme() {
    assert_eq!(load(&format!("{OPTS}theme detect\nclient_theme=light\n")).needs, LIGHT);
    assert_eq!(load(&format!("{OPTS}theme detect\nclient_theme=dark\n")).needs, DARK);
    // No theme line is detect, tmux's default.
    assert_eq!(load(&format!("{OPTS}client_theme=light\n")).needs, LIGHT);
}

#[test]
fn an_unknown_client_theme_is_dark() {
    // Empty until the terminal sends a mode 2031 or DSR 996 report.
    assert_eq!(load(&format!("{OPTS}theme detect\nclient_theme=\n")).needs, DARK);
    // An older tmux that prints no client_theme line at all.
    assert_eq!(load(&format!("{OPTS}theme detect\n")).needs, DARK);
}

#[test]
fn the_theme_option_overrides_the_client() {
    assert_eq!(load(&format!("{OPTS}theme dark\nclient_theme=light\n")).needs, DARK);
    assert_eq!(load(&format!("{OPTS}theme light\nclient_theme=dark\n")).needs, LIGHT);
    // terminal is the opt-out: the dark set, as before #240.
    assert_eq!(load(&format!("{OPTS}theme terminal\nclient_theme=light\n")).needs, DARK);
}

#[test]
fn no_color_ignores_a_light_client() {
    let _guard = ENV.lock().unwrap_or_else(|e| e.into_inner());
    // SAFETY: ENV is held, so no other test thread reads these.
    unsafe {
        std::env::set_var("NO_COLOR", "1");
        std::env::set_var("COCKPIT_TMUX", "/nonexistent/tmux");
    }
    let th = Theme::load();
    unsafe {
        std::env::remove_var("NO_COLOR");
        std::env::remove_var("COCKPIT_TMUX");
    }
    assert_eq!(th.needs, Color::Reset);
}

#[test]
fn a_failed_client_theme_read_keeps_the_roles() {
    let _guard = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join(format!("cockpit-theme-fail-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("options");
    std::fs::write(&out, format!("{OPTS}theme detect\n")).unwrap();
    // tmux prints what ran before the failing command, then exits 1.
    let bin = dir.join("tmux");
    let script = format!(
        "#!/bin/sh\ncat '{}'\ncase \"$*\" in *display-message*) exit 1 ;; esac\n",
        out.display()
    );
    fake::write_exec(&bin, &script);
    // SAFETY: ENV is held, so no other test thread reads these.
    unsafe {
        std::env::remove_var("NO_COLOR");
        std::env::set_var("COCKPIT_TMUX", &bin);
    }
    let th = Theme::load();
    unsafe { std::env::remove_var("COCKPIT_TMUX") };
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(th.needs, DARK);
}
