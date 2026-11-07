//! Janela principal: pequena, sem bordas, perto da bandeja.
//! Ao ocultar, a webview é DESTRUÍDA (libera a RAM do WebView2); ao abrir de
//! novo, é recriada. O bot continua rodando no processo principal.

use tauri::{AppHandle, Manager, PhysicalPosition, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

pub const LABEL: &str = "main";
pub const WIDTH: f64 = 340.0;
pub const MAIN_HEIGHT: f64 = 196.0;
const MARGIN: i32 = 12;

pub fn show(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
        return;
    }
    let built = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html".into()))
        .title("AutoPing")
        .inner_size(WIDTH, MAIN_HEIGHT)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .skip_taskbar(true)
        // Widget da bandeja: sempre visível por cima (senão abre atrás de
        // outras janelas, pois o Windows bloqueia roubo de foco).
        .always_on_top(true)
        .visible(false)
        .build();
    match built {
        Ok(w) => {
            place_bottom_right(&w);
            let _ = w.show();
            let _ = w.set_focus();
        }
        Err(e) => crate::logf!("Falha ao abrir janela: {e}"),
    }
}

pub fn hide(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.destroy();
    }
}

pub fn toggle(app: &AppHandle) {
    match app.get_webview_window(LABEL) {
        Some(w) if w.is_visible().unwrap_or(false) => hide(app),
        _ => show(app),
    }
}

/// Muda a altura mantendo a borda inferior no mesmo lugar.
pub fn resize(app: &AppHandle, height: f64) {
    let Some(w) = app.get_webview_window(LABEL) else { return };
    let scale = w.scale_factor().unwrap_or(1.0);
    let (Ok(pos), Ok(size)) = (w.outer_position(), w.outer_size()) else { return };
    let new_h = (height * scale).round() as i32;
    let bottom = pos.y + size.height as i32;
    let _ = w.set_size(tauri::LogicalSize::new(WIDTH, height));
    let _ = w.set_position(PhysicalPosition::new(pos.x, bottom - new_h));
}

fn place_bottom_right(w: &WebviewWindow) {
    let Ok(Some(monitor)) = w.current_monitor().or_else(|_| w.primary_monitor()) else { return };
    let Ok(size) = w.outer_size() else { return };
    let area = monitor.work_area();
    let scale = monitor.scale_factor();
    let margin = (MARGIN as f64 * scale) as i32;
    let x = area.position.x + area.size.width as i32 - size.width as i32 - margin;
    let y = area.position.y + area.size.height as i32 - size.height as i32 - margin;
    let _ = w.set_position(PhysicalPosition::new(x, y));
}
