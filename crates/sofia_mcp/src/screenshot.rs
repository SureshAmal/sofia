use std::process::Command;

#[cfg(target_os = "linux")]
pub fn capture() -> Result<Vec<u8>, String> {
    let output = Command::new("grim")
        .arg("-")
        .output()
        .map_err(|e| format!("Could not run grim: {e}"))?;
    if !output.status.success() {
        return Err("grim could not capture the screen".into());
    }
    if output.stdout.is_empty() {
        return Err("grim returned an empty screenshot".into());
    }
    Ok(output.stdout)
}

#[cfg(target_os = "windows")]
pub fn capture() -> Result<Vec<u8>, String> {
    let script = r#"
        Add-Type -AssemblyName System.Windows.Forms
        Add-Type -AssemblyName System.Drawing
        $bounds = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
        $bmp = New-Object System.Drawing.Bitmap $bounds.Width, $bounds.Height
        $graphics = [System.Drawing.Graphics]::FromImage($bmp)
        $graphics.CopyFromScreen($bounds.Location, [System.Drawing.Point]::Empty, $bounds.Size)
        $ms = New-Object System.IO.MemoryStream
        $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
        $bytes = $ms.ToArray()
        [System.Convert]::ToBase64String($bytes)
    "#;
    let output = Command::new("powershell")
        .args(["-NoProfile", "-Command", script])
        .output()
        .map_err(|e| format!("Could not run screenshot capture: {e}"))?;
    if !output.status.success() {
        return Err("Windows screenshot capture failed".into());
    }
    let b64 = String::from_utf8_lossy(&output.stdout).trim().to_string();
    base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| format!("Invalid screenshot image data: {e}"))
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
pub fn capture() -> Result<Vec<u8>, String> {
    Err("Screenshot is not supported on this operating system".into())
}
