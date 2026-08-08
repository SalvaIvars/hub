# Hub

hub personal de feeds (RSS/Atom/JSON). Aplicación de escritorio **Tauri 2** con backend Rust (workspace Cargo, arquitectura hexagonal) y frontend React + TypeScript + Vite.

## Ejecución

- `npm run tauri dev` — app completa (compila Rust y arranca Vite en `:5173`).
- `npm run dev` — solo el frontend.
- `cargo test` — tests Rust.
- `npm test` — tests del frontend (vitest).
- `npm run build` — typecheck + build de producción.

## Estructura

- `crates/domain` — modelos puros sin dependencias de infra.
- `crates/feeds` — descubrimiento y parseo de feeds.
- `crates/extractor` — extracción de contenido limpio.
- `crates/storage` — SQLite + búsqueda FTS5 + embeddings vectoriales.
- `crates/pipeline` — orquestación asíncrona.
- `crates/embeddings` — embeddings semánticos locales (ONNX).
- `crates/app` — binario Tauri (config, comandos, wiring).
- `src/` — frontend React + TS.

## Licencia

MIT
