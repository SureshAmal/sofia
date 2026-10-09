use std::process::Command;

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
