use serde_json::Value;
use std::fs;
use std::io;
use std::path::PathBuf;

/// products.json lives next to the running executable (or in the same
/// directory as the dev binary during `tauri dev`) so the bakery can find,
/// back up, and copy it just by looking in the folder — no OS-specific
/// app-data path.
fn products_path() -> io::Result<PathBuf> {
    let exe = std::env::current_exe()?;
    let dir = exe
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "executable has no parent directory"))?;
    Ok(dir.join("products.json"))
}

#[tauri::command]
fn read_products() -> String {
    let path = match products_path() {
        Ok(p) => p,
        Err(_) => return "{}".to_string(),
    };

    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return "{}".to_string(),
    };

    let trimmed = content.trim();
    if trimmed.is_empty() {
        return "{}".to_string();
    }

    match serde_json::from_str::<Value>(trimmed) {
        Ok(_) => trimmed.to_string(),
        Err(_) => {
            // Corrupt file — rename it aside so nothing is silently destroyed,
            // then start empty, matching the old Python server's behaviour.
            let broken = path.with_extension("json.broken");
            let _ = fs::rename(&path, &broken);
            "{}".to_string()
        }
    }
}

#[tauri::command]
fn write_products(data: String) -> Result<(), String> {
    let parsed: Value = serde_json::from_str(&data).map_err(|e| format!("invalid JSON: {e}"))?;
    if !parsed.is_object() {
        return Err("expected a JSON object".to_string());
    }

    let path = products_path().map_err(|e| e.to_string())?;
    let tmp_path = path.with_extension("json.tmp");

    fs::write(&tmp_path, &data).map_err(|e| format!("could not write temp file: {e}"))?;
    fs::rename(&tmp_path, &path).map_err(|e| format!("could not finalize write: {e}"))?;

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![read_products, write_products])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
