//! Preserve native paste shortcuts even when the clipboard contains files rather than text.
//! egui-winit otherwise consumes the shortcut without emitting a Paste event.
use std::sync::{Arc, OnceLock};

use eframe::egui;
use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, ElementState, StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::WindowId;

pub(crate) fn editing(ctx: &egui::Context) -> bool {
    ctx.memory(|memory| memory.focused())
        .is_some_and(|id| egui::TextEdit::load_state(ctx, id).is_some())
}

pub(crate) fn take(ctx: &egui::Context) -> bool {
    ctx.data_mut(|data| data.remove_temp::<bool>(egui::Id::new("native_paste")))
        .unwrap_or(false)
}

pub(crate) fn request(ctx: &egui::Context) {
    ctx.data_mut(|data| data.insert_temp(egui::Id::new("native_paste"), true));
    ctx.request_repaint();
}

/// Runs eframe with a shortcut bridge; the renderer and all other events stay with eframe.
pub fn run(
    app_name: &str,
    mut options: eframe::NativeOptions,
    creator: eframe::AppCreator<'_>,
) -> eframe::Result {
    let mut builder = EventLoop::<eframe::UserEvent>::with_user_event();
    if let Some(hook) = options.event_loop_builder.take() {
        hook(&mut builder);
    }
    let event_loop = builder.build()?;
    let context = Arc::new(OnceLock::new());
    let shared = context.clone();
    let inner = eframe::create_native(
        app_name,
        options,
        Box::new(move |cc| {
            let _ = shared.set(cc.egui_ctx.clone());
            creator(cc)
        }),
        &event_loop,
    );
    let mut app = Bridge {
        inner,
        context,
        modifiers: ModifiersState::empty(),
    };
    event_loop.run_app(&mut app)?;
    Ok(())
}

struct Bridge<'a> {
    inner: eframe::EframeWinitApplication<'a>,
    context: Arc<OnceLock<egui::Context>>,
    modifiers: ModifiersState,
}

fn is_paste(key: &Key, modifiers: ModifiersState) -> bool {
    let command = if cfg!(target_os = "macos") {
        modifiers.super_key()
    } else {
        modifiers.control_key()
    };
    (command && matches!(key, Key::Character(text) if text.eq_ignore_ascii_case("v")))
        || (modifiers.shift_key() && *key == Key::Named(NamedKey::Insert))
}

impl ApplicationHandler<eframe::UserEvent> for Bridge<'_> {
    fn window_event(&mut self, event_loop: &ActiveEventLoop, window: WindowId, event: WindowEvent) {
        match &event {
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
            WindowEvent::Focused(false) => self.modifiers = ModifiersState::empty(),
            WindowEvent::KeyboardInput { event, .. }
                if event.state == ElementState::Pressed
                    && !event.repeat
                    && is_paste(&event.logical_key, self.modifiers) =>
            {
                if let Some(ctx) = self.context.get().filter(|ctx| !editing(ctx)) {
                    request(ctx);
                    return;
                }
            }
            _ => {}
        }
        self.inner.window_event(event_loop, window, event);
    }
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.inner.resumed(event_loop);
    }
    fn suspended(&mut self, event_loop: &ActiveEventLoop) {
        self.inner.suspended(event_loop);
    }
    fn exiting(&mut self, event_loop: &ActiveEventLoop) {
        self.inner.exiting(event_loop);
    }
    fn memory_warning(&mut self, event_loop: &ActiveEventLoop) {
        self.inner.memory_warning(event_loop);
    }
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.inner.about_to_wait(event_loop);
    }
    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        self.inner.new_events(event_loop, cause);
    }
    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: eframe::UserEvent) {
        self.inner.user_event(event_loop, event);
    }
    fn device_event(&mut self, event_loop: &ActiveEventLoop, device: DeviceId, event: DeviceEvent) {
        self.inner.device_event(event_loop, device, event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_paste_shortcut_is_queued_once_even_without_clipboard_text() {
        let ctx = egui::Context::default();
        let modifiers = if cfg!(target_os = "macos") {
            ModifiersState::SUPER
        } else {
            ModifiersState::CONTROL
        };
        assert!(is_paste(&Key::Character("v".into()), modifiers));
        assert!(!is_paste(
            &Key::Character("v".into()),
            ModifiersState::empty()
        ));
        request(&ctx);
        assert!(take(&ctx));
        assert!(!take(&ctx));
    }
}
