use std::process::Command;

pub enum ClipboardData {
    Text(String),
    Image { bytes: Vec<u8>, mime_type: String },
}

pub fn read() -> Result<ClipboardData, String> {
    if let Ok(types) = Command::new("wl-paste").arg("--list-types").output()
        && types.status.success()
    {
        let types = String::from_utf8_lossy(&types.stdout);
        for mime_type in ["image/png", "image/jpeg"] {
            if types.lines().any(|line| line.trim() == mime_type) {
                let output = Command::new("wl-paste")
                    .args(["--no-newline", "--type", mime_type])
                    .output()
                    .map_err(|error| format!("Could not read clipboard image: {error}"))?;
                if output.status.success() && !output.stdout.is_empty() {
                    return Ok(ClipboardData::Image {
                        bytes: output.stdout,
                        mime_type: mime_type.into(),
                    });
                }
            }
        }
    }
    if let Ok(targets) = Command::new("xclip")
        .args(["-selection", "clipboard", "-t", "TARGETS", "-o"])
        .output()
        && targets.status.success()
    {
        let targets = String::from_utf8_lossy(&targets.stdout);
        for mime_type in ["image/png", "image/jpeg"] {
            if targets.lines().any(|line| line.trim() == mime_type) {
                let output = Command::new("xclip")
                    .args(["-selection", "clipboard", "-t", mime_type, "-o"])
                    .output()
                    .map_err(|error| format!("Could not read clipboard image: {error}"))?;
                if output.status.success() && !output.stdout.is_empty() {
                    return Ok(ClipboardData::Image {
                        bytes: output.stdout,
                        mime_type: mime_type.into(),
                    });
                }
            }
        }
    }
    for (program, args) in [
        ("wl-paste", vec!["--no-newline"]),
        ("xclip", vec!["-selection", "clipboard", "-o"]),
    ] {
        match Command::new(program).args(args).output() {
            Ok(output) if output.status.success() => {
                return String::from_utf8(output.stdout)
                    .map(ClipboardData::Text)
                    .map_err(|_| {
                        "Clipboard contains an unsupported image or non-text data".into()
                    });
            }
            Ok(_) | Err(_) => continue,
        }
    }
    #[cfg(target_os = "windows")]
    {
        let output = Command::new("powershell")
            .args(["-NoProfile", "-Command", "Get-Clipboard"])
            .output();
        if let Ok(output) = output
            && output.status.success()
        {
            let text = String::from_utf8_lossy(&output.stdout).trim_end().to_string();
            if !text.is_empty() {
                return Ok(ClipboardData::Text(text));
            }
        }
    }

    if cfg!(target_os = "linux") {
        Err("Unable to read the clipboard: wl-paste or xclip is required".into())
    } else if cfg!(target_os = "windows") {
        Err("Unable to read clipboard or clipboard is empty".into())
    } else {
        Err("Clipboard is not supported on this operating system".into())
    }
}
