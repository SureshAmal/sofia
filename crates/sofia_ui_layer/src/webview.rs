//! Browser-backed HTML content on Linux Wayland.
use serde_json::json;
use std::{
    io::Write,
    process::{Child, ChildStdin, Command, Stdio},
};

pub(crate) struct WebHost {
    child: Child,
    stdin: ChildStdin,
    revision: Option<i64>,
    rect: Option<(i32, i32, i32, i32)>,
}

impl WebHost {
    pub(crate) fn start() -> Result<Self, String> {
        let python = if std::path::Path::new("/usr/bin/python3").exists() {
            "/usr/bin/python3"
        } else {
            "python3"
        };
        let mut child = Command::new(python)
            .args(["-u", "-c", include_str!("webview_host.py")])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
            .map_err(|error| format!("Webview could not start: {error}"))?;
        let stdin = child.stdin.take().ok_or("Webview has no input")?;
        Ok(Self {
            child,
            stdin,
            revision: None,
            rect: None,
        })
    }

    pub(crate) fn show(
        &mut self,
        revision: i64,
        html: &str,
        rect: (i32, i32, i32, i32),
    ) -> Result<(), String> {
        if self.revision == Some(revision) && self.rect == Some(rect) {
            return Ok(());
        }
        let mut message = json!({
            "type": "show",
            "x": rect.0,
            "y": rect.1,
            "width": rect.2,
            "height": rect.3,
            "opacity": 0.95,
        });
        if self.revision != Some(revision) {
            message["html"] = html.into();
        }
        writeln!(self.stdin, "{message}").map_err(|error| format!("Webview stopped: {error}"))?;
        self.revision = Some(revision);
        self.rect = Some(rect);
        Ok(())
    }

    pub(crate) fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
}

impl Drop for WebHost {
    fn drop(&mut self) {
        let _ = writeln!(self.stdin, "{{\"type\":\"close\"}}");
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
