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

/// Rejects anything that could climb out of the exports directory. These
/// names are built by the frontend rather than typed by hand, but a path
/// separator slipping into a filename would silently write outside the app
/// folder, so they're checked here rather than trusted.
fn safe_path_component(part: &str, what: &str) -> Result<String, String> {
    let trimmed = part.trim();
    if trimmed.is_empty() {
        return Err(format!("{what} is empty"));
    }
    if trimmed == "." || trimmed == ".." || trimmed.contains("..") {
        return Err(format!("{what} is not a valid folder name"));
    }
    if trimmed.contains('/') || trimmed.contains('\\') || trimmed.contains(':') {
        return Err(format!("{what} must not contain a path separator"));
    }
    Ok(trimmed.to_string())
}

/// Writes a finished CSV into `<app folder>/<folder>/<week>/<filename>`,
/// creating the folders on the way. Saving next to the executable keeps the
/// exports beside `products.json`, so backing the whole thing up is still a
/// matter of copying one folder.
#[tauri::command]
fn save_export(
    folder: String,
    week: String,
    filename: String,
    contents: String,
) -> Result<String, String> {
    let folder = safe_path_component(&folder, "Folder name")?;
    let week = safe_path_component(&week, "Week folder name")?;
    let filename = safe_path_component(&filename, "File name")?;

    let base = products_path()
        .map_err(|e| e.to_string())?
        .parent()
        .ok_or_else(|| "could not find the application folder".to_string())?
        .to_path_buf();

    let dir = base.join(&folder).join(&week);
    fs::create_dir_all(&dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;

    let path = dir.join(&filename);
    fs::write(&path, contents).map_err(|e| format!("could not write {}: {e}", path.display()))?;

    Ok(path.to_string_lossy().to_string())
}

// Prints the labels straight to the default printer with no dialog at all,
// via WebView2's Print() API (the COM equivalent of PrintAsync). The page
// size is pinned to the label size in inches — WebView2's unit here — so it
// can't fall back to the printer's default media and rescale the label.
//
// This command MUST stay `async`. Tauri runs sync commands on the main
// thread, and WebView2 delivers the print-completed callback on that same
// thread — so a sync version that waits for the callback deadlocks itself,
// times out, and (previously) fell through to a surprise system print
// dialog. As an async command it runs on the async runtime, leaving the main
// thread free to pump messages and actually deliver the callback.
#[tauri::command]
async fn print_labels(
    window: tauri::WebviewWindow,
    width_mm: f64,
    height_mm: f64,
) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use webview2_com::Microsoft::Web::WebView2::Win32::{
            ICoreWebView2Environment6, ICoreWebView2_16,
            COREWEBVIEW2_PRINT_STATUS_PRINTER_UNAVAILABLE, COREWEBVIEW2_PRINT_STATUS_SUCCEEDED,
        };
        use webview2_com::PrintCompletedHandler;
        use windows::core::Interface;

        let (tx, mut rx) = tauri::async_runtime::channel::<Result<(), String>>(1);

        window
            .with_webview(move |webview| {
                let tx_err = tx.clone();
                let outcome: windows::core::Result<()> = (|| unsafe {
                    let env = webview.environment().cast::<ICoreWebView2Environment6>()?;
                    let settings = env.CreatePrintSettings()?;
                    settings.SetPageWidth(width_mm / 25.4)?;
                    settings.SetPageHeight(height_mm / 25.4)?;
                    settings.SetMarginTop(0.0)?;
                    settings.SetMarginBottom(0.0)?;
                    settings.SetMarginLeft(0.0)?;
                    settings.SetMarginRight(0.0)?;
                    settings.SetShouldPrintBackgrounds(true)?;
                    settings.SetShouldPrintHeaderAndFooter(false)?;

                    let core = webview
                        .controller()
                        .CoreWebView2()?
                        .cast::<ICoreWebView2_16>()?;
                    core.Print(
                        &settings,
                        &PrintCompletedHandler::create(Box::new(move |result, status| {
                            let outcome = match result {
                                Err(e) => Err(e.to_string()),
                                Ok(()) if status == COREWEBVIEW2_PRINT_STATUS_SUCCEEDED => Ok(()),
                                Ok(()) if status == COREWEBVIEW2_PRINT_STATUS_PRINTER_UNAVAILABLE => {
                                    Err("No printer available. Check the label printer is \
                                         switched on, connected, and set as the default \
                                         printer in Windows."
                                        .to_string())
                                }
                                Ok(()) => Err("The printer reported an error while printing \
                                               the labels."
                                    .to_string()),
                            };
                            let _ = tx.try_send(outcome);
                            Ok(())
                        })),
                    )
                })();

                if let Err(e) = outcome {
                    let _ = tx_err.try_send(Err(e.to_string()));
                }
            })
            .map_err(|e| e.to_string())?;

        return rx
            .recv()
            .await
            .unwrap_or_else(|| Err("The print job finished without reporting a result.".to_string()));
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (window, width_mm, height_mm);
        Err("print_labels is only implemented on Windows".to_string())
    }
}

/// Diagnostic: dump the exact PDF WebView2 renders internally for the current
/// page to `label_debug.pdf`, next to `products.json`. This is how we proved
/// the barcode corruption happened inside WebView2's own rendering rather
/// than in the printer driver. Not wired to any button — invoke it from the
/// devtools console (`__TAURI__.core.invoke('save_debug_pdf')`) while the
/// labels are laid out, and never during a real print job (two concurrent
/// WebView2 print operations hang each other).
#[tauri::command]
async fn save_debug_pdf(
    window: tauri::WebviewWindow,
    width_mm: f64,
    height_mm: f64,
) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use webview2_com::Microsoft::Web::WebView2::Win32::{
            ICoreWebView2Environment6, ICoreWebView2_7,
        };
        use webview2_com::PrintToPdfCompletedHandler;
        use windows::core::Interface;

        let path = products_path()
            .map_err(|e| e.to_string())?
            .parent()
            .ok_or_else(|| "no parent directory".to_string())?
            .join("label_debug.pdf");
        let path_str = path
            .to_str()
            .ok_or_else(|| "debug pdf path is not valid UTF-8".to_string())?
            .to_string();

        // Same reason as print_labels: must be async so the main thread stays
        // free to deliver WebView2's completion callback.
        let (tx, mut rx) = tauri::async_runtime::channel::<Result<(), String>>(1);

        window
            .with_webview(move |webview| {
                let tx_err = tx.clone();
                let outcome: windows::core::Result<()> = (|| unsafe {
                    let env = webview.environment().cast::<ICoreWebView2Environment6>()?;
                    let settings = env.CreatePrintSettings()?;
                    settings.SetPageWidth(width_mm / 25.4)?;
                    settings.SetPageHeight(height_mm / 25.4)?;
                    settings.SetMarginTop(0.0)?;
                    settings.SetMarginBottom(0.0)?;
                    settings.SetMarginLeft(0.0)?;
                    settings.SetMarginRight(0.0)?;
                    settings.SetShouldPrintBackgrounds(true)?;
                    settings.SetShouldPrintHeaderAndFooter(false)?;

                    let core = webview
                        .controller()
                        .CoreWebView2()?
                        .cast::<ICoreWebView2_7>()?;
                    core.PrintToPdf(
                        &windows::core::HSTRING::from(path_str.as_str()),
                        &settings,
                        &PrintToPdfCompletedHandler::create(Box::new(move |result, _| {
                            let _ = tx.try_send(result.map_err(|e| e.to_string()));
                            Ok(())
                        })),
                    )
                })();

                if let Err(e) = outcome {
                    let _ = tx_err.try_send(Err(e.to_string()));
                }
            })
            .map_err(|e| e.to_string())?;

        return rx
            .recv()
            .await
            .unwrap_or_else(|| Err("debug PDF finished without reporting a result".to_string()));
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (window, width_mm, height_mm);
        Err("save_debug_pdf is only implemented on Windows".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::safe_path_component;

    #[test]
    fn accepts_the_names_the_app_actually_generates() {
        for name in [
            "Craftybase exports",
            "Invoice list backups",
            "week-commencing-2026-08-03",
            "craftybase-purchases-2026-08-06.csv",
        ] {
            assert!(safe_path_component(name, "x").is_ok(), "rejected {name}");
        }
    }

    #[test]
    fn rejects_attempts_to_escape_the_exports_folder() {
        for name in [
            "..",
            ".",
            "../secrets",
            "a/b",
            "a\\b",
            "C:windows",
            "",
            "   ",
        ] {
            assert!(safe_path_component(name, "x").is_err(), "accepted {name:?}");
        }
    }

    #[test]
    fn trims_surrounding_whitespace() {
        assert_eq!(safe_path_component("  week-1  ", "x").unwrap(), "week-1");
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            read_products,
            write_products,
            save_export,
            print_labels,
            save_debug_pdf
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
