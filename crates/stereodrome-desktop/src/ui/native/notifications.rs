use std::{collections::HashSet, path::Path};

#[cfg(unix)]
use notify_rust::Notification;

#[cfg(unix)]
fn show(summary: &str, body: &str, image: Option<&Path>) -> Result<(), String> {
    let mut notification = Notification::new();
    notification.summary(summary).body(body);
    if let Some(path) = image.and_then(Path::to_str) {
        notification.image_path(path);
    }
    notification
        .show()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[cfg(windows)]
fn show(summary: &str, body: &str, image: Option<&Path>) -> Result<(), String> {
    use std::process::Command;

    const SCRIPT: &str = r#"
$summary = [Security.SecurityElement]::Escape($args[0])
$body = [Security.SecurityElement]::Escape($args[1])
$image = if ($args[2]) {
  $uri = [Security.SecurityElement]::Escape(([Uri]$args[2]).AbsoluteUri)
  "<image placement=`"appLogoOverride`" src=`"$uri`"/>"
} else { "" }
$xml = New-Object Windows.Data.Xml.Dom.XmlDocument
$xml.LoadXml("<toast><visual><binding template=`"ToastGeneric`">$image<text>$summary</text><text>$body</text></binding></visual></toast>")
[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] > $null
$toast = [Windows.UI.Notifications.ToastNotification]::new($xml)
[Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier("Stereodrome").Show($toast)
"#;
    let status = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            SCRIPT,
            summary,
            body,
            image.and_then(Path::to_str).unwrap_or_default(),
        ])
        .status()
        .map_err(|error| error.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("notification process exited with {status}"))
    }
}

#[derive(Default)]
pub struct NotificationService {
    last_song_id: Option<String>,
    update_versions: HashSet<String>,
}

impl NotificationService {
    pub fn begin_song(&mut self, song_id: &str) -> bool {
        if self.last_song_id.as_deref() == Some(song_id) {
            return false;
        }
        self.last_song_id = Some(song_id.to_string());
        true
    }

    pub fn clear_song(&mut self) {
        self.last_song_id = None;
    }

    pub fn send_now_playing(
        &self,
        title: &str,
        artist: Option<&str>,
        cover_art_path: Option<&Path>,
    ) -> Result<(), String> {
        let body = artist
            .filter(|artist| !artist.is_empty())
            .map(|artist| format!("{artist} - {title}"))
            .unwrap_or_else(|| title.to_string());
        show("Now Playing", &body, cover_art_path)
            .map_err(|error| format!("Failed to send now-playing notification: {error}"))
    }

    pub fn send_update_available(&mut self, version: &str) -> Result<(), String> {
        if !self.update_versions.insert(version.to_string()) {
            return Ok(());
        }
        if let Err(error) = show(
            "Stereodrome Update Available",
            &format!("Version {version} is available to install."),
            None,
        ) {
            self.update_versions.remove(version);
            return Err(format!("Failed to send update notification: {error}"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::NotificationService;

    #[test]
    fn song_transitions_are_deduplicated_until_cleared() {
        let mut notifications = NotificationService::default();
        assert!(notifications.begin_song("song-a"));
        assert!(!notifications.begin_song("song-a"));
        assert!(notifications.begin_song("song-b"));
        notifications.clear_song();
        assert!(notifications.begin_song("song-b"));
    }
}
