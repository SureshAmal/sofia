use serde::Serialize;
use std::{fs, path::PathBuf, process::Command};

#[derive(Clone, Debug, Serialize)]
pub struct Application {
    pub name: String,
    pub desktop_id: String,
    pub executable: Option<String>,
}

#[cfg(target_os = "linux")]
fn directories() -> Vec<PathBuf> {
    let mut dirs = vec![
        PathBuf::from("/usr/share/applications"),
        PathBuf::from("/usr/local/share/applications"),
    ];
    if let Some(home) = std::env::var_os("HOME") {
        dirs.push(PathBuf::from(home).join(".local/share/applications"));
    }
    dirs
}

#[cfg(target_os = "windows")]
fn directories() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(appdata) = std::env::var_os("APPDATA") {
        dirs.push(PathBuf::from(appdata).join(r"Microsoft\Windows\Start Menu\Programs"));
    }
    if let Some(progdata) = std::env::var_os("ProgramData") {
        dirs.push(PathBuf::from(progdata).join(r"Microsoft\Windows\Start Menu\Programs"));
    }
    dirs
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn directories() -> Vec<PathBuf> {
    Vec::new()
}

pub fn list(query: Option<&str>) -> Result<Vec<Application>, String> {
    let query = query
        .map(|q| q.trim().to_lowercase())
        .filter(|q| !q.is_empty());
    let mut apps = Vec::new();

    #[cfg(target_os = "linux")]
    for dir in directories() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
                continue;
            }
            let text = match fs::read_to_string(&path) {
                Ok(text) => text,
                Err(_) => continue,
            };
            if !text.lines().any(|line| line == "Type=Application")
                || text
                    .lines()
                    .any(|line| line == "Hidden=true" || line == "NoDisplay=true")
            {
                continue;
            }
            let name = field(&text, "Name");
            let exec = field(&text, "Exec");
            let Some(name) = name else { continue };
            let id = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
            let score = query
                .as_ref()
                .map(|q| fuzzy_score(q, &name, &id))
                .unwrap_or(Some(0));
            if let Some(score) = score {
                apps.push((
                    score,
                    Application {
                        name,
                        desktop_id: id,
                        executable: exec,
                    },
                ));
            }
        }
    }

    #[cfg(target_os = "windows")]
    for dir in directories() {
        let mut stack = vec![dir];
        while let Some(current) = stack.pop() {
            let Ok(entries) = fs::read_dir(&current) else { continue };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                if path.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("lnk")).unwrap_or(false) {
                    let stem = path.file_stem().unwrap_or_default().to_string_lossy().into_owned();
                    let score = query
                        .as_ref()
                        .map(|q| fuzzy_score(q, &stem, &stem))
                        .unwrap_or(Some(0));
                    if let Some(score) = score {
                        apps.push((
                            score,
                            Application {
                                name: stem.clone(),
                                desktop_id: path.to_string_lossy().into_owned(),
                                executable: Some(path.to_string_lossy().into_owned()),
                            },
                        ));
                    }
                }
            }
        }
    }

    apps.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| a.1.name.to_lowercase().cmp(&b.1.name.to_lowercase()))
    });
    Ok(apps.into_iter().map(|(_, app)| app).collect())
}

pub fn launch(query: &str) -> Result<Application, String> {
    let mut matches = list(Some(query))?;
    let app = matches
        .drain(..)
        .next()
        .ok_or_else(|| format!("No installed application matched '{query}'"))?;

    #[cfg(target_os = "linux")]
    {
        let desktop_id = app.desktop_id.trim_end_matches(".desktop");
        let result = Command::new("gtk-launch")
            .arg(desktop_id)
            .status()
            .or_else(|_| {
                Command::new("gio")
                    .args(["launch", &app.desktop_id])
                    .status()
            })
            .map_err(|e| format!("Could not launch '{}': {e}", app.name))?;
        if !result.success() {
            return Err(format!("Application '{}' refused to start", app.name));
        }
    }

    #[cfg(target_os = "windows")]
    {
        open::that(&app.desktop_id).map_err(|e| format!("Could not launch '{}': {e}", app.name))?;
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        return Err("Application launching is not supported on this operating system".into());
    }

    Ok(app)
}

fn field(text: &str, key: &str) -> Option<String> {
    text.lines()
        .find_map(|line| line.strip_prefix(&format!("{key}=")))
        .map(|v| v.split(" %").next().unwrap_or(v).trim().to_string())
}
fn fuzzy_score(query: &str, name: &str, id: &str) -> Option<u8> {
    let hay = format!("{} {}", name.to_lowercase(), id.to_lowercase());
    if hay == query {
        Some(0)
    } else if hay.starts_with(query) {
        Some(1)
    } else if hay.contains(query) {
        Some(2)
    } else if query.chars().all(|c| hay.contains(c)) {
        Some(3)
    } else {
        None
    }
}
