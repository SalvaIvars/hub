//! Pruebas de integración reales (requieren red): ingesta de un blog con feed y
//! de una portada sin feed. Se ejecutan explícitamente con:
//! cargo test --test real_ingest -- --ignored

use reader_extractor::TrafilaturaExtractor;
use reader_feeds::{FeedRsParser, WebpageDiscoverer};
use reader_pipeline::{Pipeline, ReqwestClient};
use reader_storage::{ArticleRepo, SourceRepo, open_db_in_memory, ArticleRepository, SourceRepository};
use std::sync::{Arc, Mutex};

#[tokio::test]
#[ignore = "requiere red"]
async fn ingest_real_blog_with_feed() {
    let conn = Arc::new(Mutex::new(open_db_in_memory().unwrap()));
    let articles = ArticleRepo::new(conn.clone());
    let sources = SourceRepo::new(conn);
    let http = ReqwestClient::new();

    let pl = Pipeline {
        http: &http,
        extractor: &TrafilaturaExtractor,
        discoverer: &WebpageDiscoverer,
        parser: &FeedRsParser,
        articles: &articles,
        sources: &sources,
    };

    // La raíz del blog es una portada: debe añadir el source con sus posts del
    // feed y NO guardar la portada como artículo basura.
    let result = pl.ingest_url("https://blog.rust-lang.org/").await.expect("ingesta falló");

    let summary = result.source.expect("debería descubrir el feed de blog.rust-lang.org");
    assert!(summary.feed_url.is_some(), "debería tener feed_url");
    assert!(summary.article_count > 0, "debería guardar posts del feed");
    // El resultado es el primer post del feed (sin artículo basura de portada).
    let article_id = result.article_id.expect("debería devolver el primer post del feed");
    let article = articles.get(article_id).unwrap().unwrap();
    assert_eq!(article.source_id, Some(summary.id), "el post debe pertenecer al source");
    assert!(!article.title.is_empty());

    // No debe existir ningún artículo que sea la propia portada.
    let all = articles.list_all().unwrap();
    assert!(
        all.iter().all(|a| a.url != "https://blog.rust-lang.org/"),
        "no debe guardarse la portada como artículo"
    );

    // Refresh: no debe duplicar.
    let added = pl.refresh_source(summary.id).await.expect("refresh falló");
    assert_eq!(added, 0, "un refresh inmediato no debe añadir nada");
    assert_eq!(sources.list().unwrap().len(), 1, "no debe duplicar el source");
}

#[tokio::test]
#[ignore = "requiere red"]
async fn ingest_real_hn_front_page_creates_source() {
    let conn = Arc::new(Mutex::new(open_db_in_memory().unwrap()));
    let articles = ArticleRepo::new(conn.clone());
    let sources = SourceRepo::new(conn);
    let http = ReqwestClient::new();

    let pl = Pipeline {
        http: &http,
        extractor: &TrafilaturaExtractor,
        discoverer: &WebpageDiscoverer,
        parser: &FeedRsParser,
        articles: &articles,
        sources: &sources,
    };

    // La portada de HN no anuncia <link rel="alternate">: el source debe salir
    // de la heurística de rutas comunes (índice) → /rss.
    let result = pl.ingest_url("https://news.ycombinator.com/").await.expect("ingesta falló");

    let summary = result.source.expect("la portada de HN debe crear un source");
    assert!(summary.feed_url.is_some(), "debe tener feed_url (/rss)");
    assert!(summary.article_count > 0, "debe guardar posts del feed");
}

#[tokio::test]
#[ignore = "requiere red"]
async fn ingest_real_hn_item_is_single_article() {
    let conn = Arc::new(Mutex::new(open_db_in_memory().unwrap()));
    let articles = ArticleRepo::new(conn.clone());
    let sources = SourceRepo::new(conn);
    let http = ReqwestClient::new();

    let pl = Pipeline {
        http: &http,
        extractor: &TrafilaturaExtractor,
        discoverer: &WebpageDiscoverer,
        parser: &FeedRsParser,
        articles: &articles,
        sources: &sources,
    };

    // Un item de HN no es un índice: no debe derivar a un source con el feed
    // de la portada, sino guardarse como artículo suelto.
    let result = pl
        .ingest_url("https://news.ycombinator.com/item?id=49224294")
        .await
        .expect("ingesta falló");

    assert!(result.source.is_none(), "no debe crearse un source con el feed del host");
    assert!(result.article_id.is_some(), "el item debe guardarse como artículo");
    let article = articles.get(result.article_id.unwrap()).unwrap().unwrap();
    assert_eq!(article.source_id, None, "el item es un artículo suelto");
    let all = articles.list_all().unwrap();
    assert_eq!(all.len(), 1, "no deben guardarse los posts de la portada");
}

#[tokio::test]
#[ignore = "requiere red"]
async fn ingest_real_root_without_feed_saves_nothing() {
    let conn = Arc::new(Mutex::new(open_db_in_memory().unwrap()));
    let articles = ArticleRepo::new(conn.clone());
    let sources = SourceRepo::new(conn);
    let http = ReqwestClient::new();

    let pl = Pipeline {
        http: &http,
        extractor: &TrafilaturaExtractor,
        discoverer: &WebpageDiscoverer,
        parser: &FeedRsParser,
        articles: &articles,
        sources: &sources,
    };

    // Una portada sin feed descubierto no debe guardar nada como artículo.
    let result = pl
        .ingest_url("https://www.rust-lang.org/")
        .await
        .expect("ingesta falló");

    assert!(result.article_id.is_none(), "una portada sin feed no crea artículo");
    assert!(result.source.is_none() || result.source.as_ref().unwrap().feed_url.is_none());
    assert!(articles.list_all().unwrap().is_empty());
}

#[tokio::test]
#[ignore = "requiere red"]
async fn ingest_real_hub_with_feed_skips_portada() {
    let conn = Arc::new(Mutex::new(open_db_in_memory().unwrap()));
    let articles = ArticleRepo::new(conn.clone());
    let sources = SourceRepo::new(conn);
    let http = ReqwestClient::new();

    let pl = Pipeline {
        http: &http,
        extractor: &TrafilaturaExtractor,
        discoverer: &WebpageDiscoverer,
        parser: &FeedRsParser,
        articles: &articles,
        sources: &sources,
    };

    // Caso reportado: la portada de interconnects.ai (JSON-LD WebSite/WebPage)
    // debe añadir el source con sus posts, sin guardar la portada como artículo.
    let result = pl
        .ingest_url("https://www.interconnects.ai/")
        .await
        .expect("ingesta falló");

    let summary = result.source.expect("debería descubrir el feed de interconnects.ai");
    assert!(summary.feed_url.is_some());
    assert!(summary.article_count > 0);

    let all = articles.list_all().unwrap();
    assert!(
        all.iter().all(|a| a.url != "https://www.interconnects.ai/"),
        "no debe guardarse la portada como artículo"
    );
    assert_eq!(all.len() as i64, summary.article_count);
    assert!(result.article_id.is_some(), "el resultado debe ser el primer post del feed");
}
