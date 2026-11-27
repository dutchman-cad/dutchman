//! The tray icon (the menu bar on macOS) and its menu: Pause or Resume, Add folder, Open
//! Dutchman, Quit.
//!
//! Windows and macOS use `tray-icon`, which needs the platform's event loop on the main
//! thread: [`run`] owns that loop and runs the controller on another thread. Linux uses `ksni`,
//! a StatusNotifierItem over D-Bus (no GTK, no loop of its own): [`run`] runs the controller
//! on the calling thread. Where no tray exists (a desktop with no StatusNotifier host), the
//! controller still runs and the window is the way in.

use std::sync::mpsc::Sender;
use std::sync::Arc;

use super::controller::Msg;
use super::icon;

/// The menu's labels, one place for both backends.
pub const PAUSE: &str = "Pause";
pub const RESUME: &str = "Resume";
pub const ADD_FOLDER: &str = "Add folder\u{2026}";
pub const OPEN_APP: &str = "Open Dutchman";
pub const QUIT: &str = "Quit Dutchman Sync";
pub const TOOLTIP: &str = "Dutchman Sync";

/// What the controller does to the tray.
pub trait TrayControl: Send + Sync {
    /// The menu says Resume (`true`) or Pause (`false`).
    fn set_paused(&self, paused: bool);
    /// Ends the tray's loop, and so the program.
    fn exit(&self);
}

/// The menu label for the pause item.
pub fn pause_label(paused: bool) -> &'static str {
    if paused {
        RESUME
    } else {
        PAUSE
    }
}

/// Shows the tray and runs `controller` until it returns, then returns. `Err` when the
/// platform's loop cannot start.
pub fn run(
    messages: Sender<Msg>,
    controller: impl FnOnce(Arc<dyn TrayControl>) + Send + 'static,
) -> Result<(), String> {
    backend::run(messages, controller)
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
mod backend {
    use super::*;
    use tray_icon::menu::{IsMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::{Icon, TrayIcon, TrayIconBuilder};
    use winit::application::ApplicationHandler;
    use winit::event::StartCause;
    use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};

    enum UserEvent {
        Menu(MenuEvent),
        #[cfg_attr(target_os = "macos", allow(dead_code))]
        Click,
        SetPaused(bool),
        Exit,
        Wake,
    }

    struct Control(EventLoopProxy<UserEvent>);

    impl TrayControl for Control {
        fn set_paused(&self, paused: bool) {
            let _ = self.0.send_event(UserEvent::SetPaused(paused));
        }
        fn exit(&self) {
            let _ = self.0.send_event(UserEvent::Exit);
        }
    }

    struct Items {
        pause: MenuItem,
        add: MenuItem,
        open: MenuItem,
        quit: MenuItem,
        // Kept alive: the icon disappears when it is dropped.
        _tray: TrayIcon,
    }

    struct Tray {
        messages: Sender<Msg>,
        proxy: EventLoopProxy<UserEvent>,
        items: Option<Items>,
        failure: Option<String>,
    }

    impl Tray {
        fn build(&self) -> Result<Items, String> {
            let rgba = icon::mark_rgba(64).ok_or("cannot draw the tray icon")?;
            let icon = Icon::from_rgba(rgba, 64, 64).map_err(|e| e.to_string())?;
            let pause = MenuItem::new(PAUSE, true, None);
            let add = MenuItem::new(ADD_FOLDER, true, None);
            let open = MenuItem::new(OPEN_APP, true, None);
            let quit = MenuItem::new(QUIT, true, None);
            let menu = Menu::new();
            let separator = PredefinedMenuItem::separator();
            let all: [&dyn IsMenuItem; 5] = [&pause, &add, &open, &separator, &quit];
            menu.append_items(&all).map_err(|e| e.to_string())?;
            let tray = TrayIconBuilder::new()
                .with_menu(Box::new(menu))
                .with_tooltip(TOOLTIP)
                .with_icon(icon)
                .build()
                .map_err(|e| e.to_string())?;
            Ok(Items {
                pause,
                add,
                open,
                quit,
                _tray: tray,
            })
        }
    }

    impl ApplicationHandler<UserEvent> for Tray {
        fn resumed(&mut self, _event_loop: &ActiveEventLoop) {}

        fn window_event(
            &mut self,
            _event_loop: &ActiveEventLoop,
            _window: winit::window::WindowId,
            _event: winit::event::WindowEvent,
        ) {
        }

        // The icon is made once the loop is running: made earlier, macOS can lose it.
        fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
            if cause == StartCause::Init && self.items.is_none() {
                match self.build() {
                    Ok(items) => {
                        self.items = Some(items);
                        // An event makes the loop turn once more, which shows the icon.
                        let _ = self.proxy.send_event(UserEvent::Wake);
                    }
                    Err(why) => {
                        self.failure = Some(why);
                        event_loop.exit();
                    }
                }
            }
        }

        fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
            match event {
                UserEvent::Menu(event) => {
                    let Some(items) = &self.items else { return };
                    let msg = if event.id == *items.pause.id() {
                        Msg::TogglePause
                    } else if event.id == *items.add.id() {
                        Msg::ShowWindow
                    } else if event.id == *items.open.id() {
                        Msg::OpenApp
                    } else if event.id == *items.quit.id() {
                        Msg::Quit
                    } else {
                        return;
                    };
                    let _ = self.messages.send(msg);
                }
                UserEvent::Click => {
                    let _ = self.messages.send(Msg::ShowWindow);
                }
                UserEvent::SetPaused(paused) => {
                    if let Some(items) = &self.items {
                        items.pause.set_text(pause_label(paused));
                    }
                }
                UserEvent::Exit => event_loop.exit(),
                UserEvent::Wake => {}
            }
        }
    }

    pub fn run(
        messages: Sender<Msg>,
        controller: impl FnOnce(Arc<dyn TrayControl>) + Send + 'static,
    ) -> Result<(), String> {
        let event_loop = EventLoop::<UserEvent>::with_user_event()
            .build()
            .map_err(|e| format!("cannot start the event loop: {e}"))?;
        let proxy = event_loop.create_proxy();
        let forward = event_loop.create_proxy();
        MenuEvent::set_event_handler(Some(move |event| {
            let _ = forward.send_event(UserEvent::Menu(event));
        }));
        // A click on the icon opens the window; on macOS the click opens the menu instead.
        #[cfg(target_os = "windows")]
        {
            let forward = event_loop.create_proxy();
            tray_icon::TrayIconEvent::set_event_handler(Some(move |event| {
                if matches!(
                    event,
                    tray_icon::TrayIconEvent::Click {
                        button: tray_icon::MouseButton::Left,
                        button_state: tray_icon::MouseButtonState::Up,
                        ..
                    }
                ) {
                    let _ = forward.send_event(UserEvent::Click);
                }
            }));
        }
        let control: Arc<dyn TrayControl> = Arc::new(Control(event_loop.create_proxy()));
        let worker = std::thread::Builder::new()
            .name("sync-controller".to_owned())
            .spawn(move || controller(control))
            .map_err(|e| format!("cannot start the controller: {e}"))?;
        let mut tray = Tray {
            messages,
            proxy,
            items: None,
            failure: None,
        };
        let ran = event_loop.run_app(&mut tray);
        drop(tray.items.take());
        if let Some(why) = tray.failure {
            return Err(why);
        }
        ran.map_err(|e| e.to_string())?;
        let _ = worker.join();
        Ok(())
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
mod backend {
    use super::*;
    use ksni::blocking::{Handle, TrayMethods};
    use ksni::menu::StandardItem;
    use ksni::MenuItem;
    use std::sync::Mutex;

    struct SyncTray {
        messages: Sender<Msg>,
        paused: bool,
        pixmap: Vec<u8>,
    }

    impl SyncTray {
        fn item(&self, label: &str, msg: Msg) -> MenuItem<Self> {
            let messages = self.messages.clone();
            StandardItem {
                label: label.to_owned(),
                activate: Box::new(move |_: &mut Self| {
                    let _ = messages.send(msg);
                }),
                ..Default::default()
            }
            .into()
        }
    }

    impl ksni::Tray for SyncTray {
        fn id(&self) -> String {
            "dutchman-sync".to_owned()
        }
        fn title(&self) -> String {
            TOOLTIP.to_owned()
        }
        fn icon_pixmap(&self) -> Vec<ksni::Icon> {
            vec![ksni::Icon {
                width: 64,
                height: 64,
                data: self.pixmap.clone(),
            }]
        }
        fn activate(&mut self, _x: i32, _y: i32) {
            let _ = self.messages.send(Msg::ShowWindow);
        }
        fn menu(&self) -> Vec<MenuItem<Self>> {
            vec![
                self.item(pause_label(self.paused), Msg::TogglePause),
                self.item(ADD_FOLDER, Msg::ShowWindow),
                self.item(OPEN_APP, Msg::OpenApp),
                MenuItem::Separator,
                self.item(QUIT, Msg::Quit),
            ]
        }
    }

    struct Control(Mutex<Handle<SyncTray>>);

    impl TrayControl for Control {
        fn set_paused(&self, paused: bool) {
            if let Ok(handle) = self.0.lock() {
                let _ = handle.update(|tray: &mut SyncTray| tray.paused = paused);
            }
        }
        fn exit(&self) {}
    }

    /// No tray: the controller runs, and the window is the way in.
    struct Absent;

    impl TrayControl for Absent {
        fn set_paused(&self, _paused: bool) {}
        fn exit(&self) {}
    }

    pub fn run(
        messages: Sender<Msg>,
        controller: impl FnOnce(Arc<dyn TrayControl>) + Send + 'static,
    ) -> Result<(), String> {
        let pixmap = icon::mark_rgba(64)
            .map(|rgba| icon::argb(&rgba))
            .unwrap_or_default();
        let tray = SyncTray {
            messages,
            paused: false,
            pixmap,
        };
        let control: Arc<dyn TrayControl> = match tray.spawn() {
            Ok(handle) => Arc::new(Control(Mutex::new(handle))),
            Err(e) => {
                eprintln!("dutchman-sync: no system tray ({e}); running without one");
                Arc::new(Absent)
            }
        };
        controller(control);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pause_item_names_the_next_action() {
        assert_eq!(pause_label(false), "Pause");
        assert_eq!(pause_label(true), "Resume");
    }
}
