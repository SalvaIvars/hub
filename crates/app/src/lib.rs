//! Crate principal de la app Tauri.
//!
//! Conecta el frontend (React/TS) con el pipeline de ingesta y los
//! repositorios SQLite a través de comandos Tauri.

mod commands;
mod opml;
mod state;

use reader_extractor::TrafilaturaExtractor;
use reader_feeds::{FeedRsParser, WebpageDiscoverer};
use reader_pipeline::{Pipeline, ReqwestClient};
use reader_storage::{
    ArticleRepo, EmbeddingRepo, EmbeddingRepository, SettingsRepo, SettingsRepository, SourceRepo,
};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::webview::{NewWindowResponse, WebviewWindowBuilder};
use tauri::{Emitter, Manager};
use tauri_plugin_opener::OpenerExt;

/// Devuelve el host de una URL (p. ej. "https://a.com/x" → "a.com").
/// Se usa como título por defecto al importar OPML sin `text`.
fn host_of(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_string()))
        .unwrap_or_else(|| url.to_string())
}

/// ¿Debe abrirse esta URL en el navegador del sistema en lugar de navegar?
fn is_external_url(url: &tauri::Url) -> bool {
    matches!(url.scheme(), "http" | "https" | "mailto" | "tel")
}

/// Punto de entrada de la aplicación.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let app_handle = app.handle().clone();
            let app_nav = app_handle.clone();
            let app_newwin = app_handle.clone();

            // La ventana principal se crea desde Rust (no desde tauri.conf.json)
            // para poder interceptar a nivel nativo la navegación y las
            // ventanas nuevas (target="_blank" / window.open): cualquier URL
            // externa se abre en el navegador del sistema y se deniega aquí.
            let dev_url = app.config().build.dev_url.clone();
            let url = match dev_url {
                Some(u) => tauri::WebviewUrl::External(u),
                None => tauri::WebviewUrl::App("index.html".into()),
            };
            WebviewWindowBuilder::new(app, "main", url)
                .title("Lector")
                .inner_size(1280.0, 800.0)
                .min_inner_size(900.0, 600.0)
                .center()
                .on_navigation(move |url| {
                    if url.scheme() == "tauri" {
                        return true;
                    }
                    if let Some(host) = url.host_str() {
                        if host == "localhost" || host == "127.0.0.1" {
                            return true;
                        }
                    }
                    if is_external_url(url) {
                        let _ = app_nav.opener().open_url(url.as_str(), None::<&str>);
                    }
                    false
                })
                .on_new_window(move |url, _features| {
                    let _ = app_newwin.opener().open_url(url.as_str(), None::<&str>);
                    NewWindowResponse::Deny
                })
                .build()?;

            let data_dir = app_data_dir(app.handle())?;
            std::fs::create_dir_all(&data_dir)
                .map_err(|e| format!("No se pudo crear el directorio de datos: {e}"))?;
            let db_path = data_dir.join("lector.db");
            let conn = reader_storage::open_db(&db_path)
                .map_err(|e| format!("No se pudo abrir la base de datos: {e}"))?;
            let conn = Arc::new(Mutex::new(conn));
            let app_state = state::AppState::new(conn.clone());
            app.manage(app_state);

            spawn_refresh_task(app.handle().clone(), conn.clone());
            spawn_embedding_backfill_task(app.handle().clone(), conn);

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::add_url,
            commands::extract_article,
            commands::list_sources,
            commands::list_articles,
            commands::list_single_articles,
            commands::list_category_articles,
            commands::get_article,
            commands::mark_read,
            commands::mark_all_read,
            commands::toggle_star,
            commands::delete_article,
            commands::delete_source,
            commands::rename_source,
            commands::refresh_source,
            commands::refresh_all_sources,
            commands::get_refresh_interval,
            commands::set_refresh_interval,
            commands::get_vector_similarity_threshold,
            commands::set_vector_similarity_threshold,
            commands::get_theme,
            commands::set_theme,
            commands::get_reader_settings,
            commands::set_reader_settings,
            commands::export_opml,
            commands::import_opml,
            commands::list_categories,
            commands::set_category,
            commands::delete_category,
            commands::list_smart_feeds,
            commands::create_smart_feed,
            commands::delete_smart_feed,
            commands::get_smart_feed_articles,
            commands::generate_embedding,
            commands::regenerate_embedding,
            commands::generate_all_embeddings,
            commands::get_embedding_status,
        ])
        .run(tauri::generate_context!())
        .expect("error al ejecutar Lector");
}

fn app_data_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map_err(|e| format!("No se pudo resolver el directorio de datos: {e}"))
}

/// Lanza el refresco automático de sources en segundo plano.
///
/// Cada iteración relee el intervalo desde `settings`, de modo que un cambio
/// de configuración se aplica en el siguiente ciclo. Al terminar un refresco,
/// emite el evento `sources-refreshed` (con el nº de artículos nuevos) para
/// que el frontend recargue.
fn spawn_refresh_task(app: tauri::AppHandle, conn: Arc<Mutex<rusqlite::Connection>>) {
    tauri::async_runtime::spawn(async move {
        let http = ReqwestClient::new();
        let extractor = TrafilaturaExtractor;
        let discoverer = WebpageDiscoverer;
        let parser = FeedRsParser;
        let articles = ArticleRepo::new(conn.clone());
        let sources = SourceRepo::new(conn.clone());

        loop {
            let minutes: u64 = {
                let settings = SettingsRepo::new(conn.clone());
                settings
                    .get("refresh_interval_minutes")
                    .ok()
                    .flatten()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(30)
            };
            // Mínimo 1 minuto para evitar ciclos absurdos con configs inválidas.
            let seconds = minutes.saturating_mul(60).max(60);
            tokio::time::sleep(tokio::time::Duration::from_secs(seconds)).await;

            let pipeline = Pipeline {
                http: &http,
                extractor: &extractor,
                discoverer: &discoverer,
                parser: &parser,
                articles: &articles,
                sources: &sources,
            };
            if let Ok(added) = pipeline.refresh_all().await {
                let _ = app.emit("sources-refreshed", added);
            }
        }
    });
}

/// Genera en segundo plano los embeddings que falten (backfill al arrancar).
///
/// Espera unos segundos para no competir con el renderizado inicial y sale
/// pronto si no hay nada pendiente (así no descarga el modelo innecesariamente).
/// Emite `embedding-backfill-started`, `embedding-backfill-done(n)` y
/// `embedding-backfill-error(msg)` para que el frontend informe al usuario.
fn spawn_embedding_backfill_task(
    app: tauri::AppHandle,
    conn: Arc<Mutex<rusqlite::Connection>>,
) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;

        let articles = ArticleRepo::new(conn.clone());
        let embeddings = EmbeddingRepo::new(conn.clone());

        // Early exit: si no hay artículos pendientes, no se toca el modelo.
        let missing = match embeddings.articles_without_embedding(1) {
            Ok(ids) => ids,
            Err(_) => return,
        };
        if missing.is_empty() {
            return;
        }

        let embedder = match reader_embeddings::FastEmbedGenerator::new() {
            Ok(e) => e,
            Err(e) => {
                let _ = app.emit("embedding-backfill-error", e.to_string());
                return;
            }
        };

        let _ = app.emit("embedding-backfill-started", ());

        match commands::embed_missing_articles(&articles, &embeddings, &embedder).await {
            Ok(n) => {
                let _ = app.emit("embedding-backfill-done", n);
            }
            Err(e) => {
                let _ = app.emit("embedding-backfill-error", e);
            }
        }
    });
}
