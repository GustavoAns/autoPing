//! Ícone da bandeja: círculo na cor do status (verde = ligado, vermelho =
//! desligado, azul = sleep). Sem conexão com o Discord, vira um anel (vazado).
//! Com o alarme tocando, pisca em laranja; clicar no ícone para o alarme.

use crate::config::Status;
use crate::discord::gateway::ConnState;
use crate::engine::{Engine, Snapshot};
use crate::window;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

const ID: &str = "main";
const SIZE: u32 = 32;

pub const GREEN: [u8; 3] = [0x22, 0xc5, 0x5e];
pub const RED: [u8; 3] = [0xef, 0x44, 0x44];
pub const BLUE: [u8; 3] = [0x3b, 0x82, 0xf6];
pub const ORANGE: [u8; 3] = [0xf5, 0x9e, 0x0b];

struct Items {
    toggle: MenuItem<Wry>,
    stop_alarm: MenuItem<Wry>,
}

static BLINKING: AtomicBool = AtomicBool::new(false);

pub fn status_color(s: Status) -> [u8; 3] {
    match s {
        Status::On => GREEN,
        Status::Off => RED,
        Status::Sleep => BLUE,
    }
}

/// Círculo antisserrilhado RGBA. `filled = false` desenha só o anel.
fn circle(rgb: [u8; 3], filled: bool) -> Image<'static> {
    let mut px = vec![0u8; (SIZE * SIZE * 4) as usize];
    let c = (SIZE as f32 - 1.0) / 2.0;
    let r = SIZE as f32 / 2.0 - 2.0;
    let ring = 4.0;
    for y in 0..SIZE {
        for x in 0..SIZE {
            let d = ((x as f32 - c).powi(2) + (y as f32 - c).powi(2)).sqrt();
            let outer = (r + 0.5 - d).clamp(0.0, 1.0);
            let a = if filled { outer } else { outer * (d - (r - ring) + 0.5).clamp(0.0, 1.0) };
            let i = ((y * SIZE + x) * 4) as usize;
            px[i] = rgb[0];
            px[i + 1] = rgb[1];
            px[i + 2] = rgb[2];
            px[i + 3] = (a * 255.0) as u8;
        }
    }
    Image::new_owned(px, SIZE, SIZE)
}

fn status_label(s: Status) -> &'static str {
    match s {
        Status::On => "Ligado",
        Status::Off => "Desligado",
        Status::Sleep => "Sleep",
    }
}

fn conn_label(c: ConnState) -> &'static str {
    match c {
        ConnState::Connected => "conectado",
        ConnState::Connecting => "conectando...",
        ConnState::Reconnecting => "reconectando...",
        ConnState::InvalidToken => "token inválido",
        ConnState::NoToken => "sem token",
        ConnState::Error => "erro de conexão",
    }
}

fn base_icon(snap: &Snapshot) -> Image<'static> {
    circle(status_color(snap.status), snap.connection == ConnState::Connected)
}

pub fn create(app: &AppHandle, engine: Arc<Engine>) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Abrir", true, None::<&str>)?;
    let stop_alarm = MenuItem::with_id(app, "stop_alarm", "Parar alarme", false, None::<&str>)?;
    let toggle = MenuItem::with_id(app, "toggle", "Desligar", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Sair", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&open, &stop_alarm, &toggle, &sep, &quit])?;

    let menu_engine = engine.clone();
    TrayIconBuilder::with_id(ID)
        .icon(circle(GREEN, false))
        .tooltip("AutoPing")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id.as_ref() {
            "open" => window::show(app),
            "stop_alarm" => {
                menu_engine.stop_alarm();
            }
            "toggle" => {
                let next = if menu_engine.snapshot().status == Status::On { Status::Off } else { Status::On };
                menu_engine.set_status(next);
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(move |tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                if engine.is_alarm_ringing() {
                    engine.stop_alarm();
                    window::show(tray.app_handle());
                } else {
                    window::toggle(tray.app_handle());
                }
            }
        })
        .build(app)?;

    app.manage(Items { toggle, stop_alarm });
    Ok(())
}

pub fn update(app: &AppHandle, snap: &Snapshot) {
    let Some(tray) = app.tray_by_id(ID) else { return };

    if snap.alert.is_some() {
        start_blink(app);
    } else {
        BLINKING.store(false, Ordering::SeqCst);
        let _ = tray.set_icon(Some(base_icon(snap)));
    }

    let channel = snap.channel.name.as_ref().map(|n| format!("#{n}")).unwrap_or_else(|| "sem canal".into());
    let alarm = snap
        .alert
        .as_ref()
        .map(|a| format!("\n🔔 {} Clique para parar o alarme", a.reason))
        .unwrap_or_default();
    let _ = tray.set_tooltip(Some(format!(
        "AutoPing - {} ({})\n{}{}",
        status_label(snap.status),
        conn_label(snap.connection),
        channel,
        alarm
    )));
    if let Some(items) = app.try_state::<Items>() {
        let _ = items.toggle.set_text(if snap.status == Status::On { "Desligar" } else { "Ligar" });
        let _ = items.stop_alarm.set_enabled(snap.alert.is_some());
    }
}

/// Pisca o ícone (laranja ↔ cor do status) enquanto o alarme toca.
fn start_blink(app: &AppHandle) {
    if BLINKING.swap(true, Ordering::SeqCst) {
        return; // já piscando
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut on = true;
        while BLINKING.load(Ordering::SeqCst) {
            if let Some(tray) = app.tray_by_id(ID) {
                let icon = if on {
                    circle(ORANGE, true)
                } else {
                    app.try_state::<Arc<Engine>>().map(|e| base_icon(&e.snapshot())).unwrap_or_else(|| circle(GREEN, true))
                };
                let _ = tray.set_icon(Some(icon));
            }
            on = !on;
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        // Restaura o ícone normal ao parar.
        if let (Some(tray), Some(e)) = (app.tray_by_id(ID), app.try_state::<Arc<Engine>>()) {
            let _ = tray.set_icon(Some(base_icon(&e.snapshot())));
        }
    });
}
