//! Desktop notifications: Windows toasts, the macOS notification centre, the freedesktop
//! notification service on Linux.

use super::engine::Notifier;

/// Shows notifications through the system.
pub struct Desktop;

impl Notifier for Desktop {
    fn notify(&self, title: &str, body: &str) {
        let mut notification = notify_rust::Notification::new();
        notification.summary(title).body(body);
        #[cfg(target_os = "linux")]
        {
            notification
                .appname(super::login::NAME)
                .icon("dutchman-sync");
        }
        // A desktop without a notification service is not an error worth stopping for.
        if let Err(e) = notification.show() {
            eprintln!("dutchman-sync: cannot show a notification: {e}");
        }
    }
}
