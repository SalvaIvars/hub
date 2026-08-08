# hub landing page

Página estática de descarga para hub (HTML/CSS/JS plano, sin build). Inspirada en la estética de [pi.dev](https://pi.dev).

## Ejecutar en local

```sh
npx serve docs
```

## Desplegar en GitHub Pages

La web vive en `docs/` y se publica desde **Settings → Pages → Build and deployment → Source → "Deploy from a branch"** (rama `main`, carpeta `/docs`). Cada push a `main` actualiza la web automáticamente.

## Capturas

Los PNG de `screenshots/` se generan corriendo la app real y capturando la ventana (macOS). Para regenerarlas: back-up de la DB dev, lanzar la app con el tema deseado y capturar con `Cmd+Shift+4`.
