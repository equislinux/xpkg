# xpkg - Resumen y estado

Este es uno de los cuatro documentos que describen `xpkg`, el constructor de
paquetes en Rust de la distribución X. Cubre qué es la herramienta, su papel en
el ecosistema X y su estado actual en el toolchain.

Documentos relacionados en esta carpeta:

- [02 - Usage](02-usage.md)
- [03 - Architecture](03-architecture.md)
- [04 - Future integration](04-future-integration.md)

---

## Qué es xpkg

`xpkg` es el **constructor de paquetes de la distribución X**. Lee recetas de
build (**XBUILD** o **PKGBUILD** heredadas), descarga fuentes, compila software
en un entorno aislado y produce paquetes `.xp` listos para instalar con `xpm`.
Es la herramienta compañera de `xpm` para desarrolladores.

Piensa en él como el equivalente a `makepkg` + `repo-add` + `namcap` para el
ecosistema X, escrito enteramente en Rust.

| Característica | Descripción |
|---------|-------------|
| Rust puro | Cero dependencias C — consistente con el ecosistema xpm |
| Formato XBUILD | Recetas declarativas en TOML, alternativa moderna a PKGBUILD |
| Compatibilidad PKGBUILD | Construye sin fricción desde PKGBUILD de Arch Linux |
| Builds fakeroot | Empaquetado aislado sin root real (unshare / fakeroot / tar-rewrite) |
| Firma de paquetes | Firmas detached OpenPGP vía sequoia-openpgp (Rust puro) |
| Linting | Chequeos automáticos: dependencias, permisos, rutas, metadata, análisis ELF |
| Herramientas de repositorio | Crea y mantiene bases ALPM para `xpm` |
| Gestión de fuentes | Descarga HTTP con reintentos, verificación SHA-256/512, clonado Git, caché local |

Metadata del proyecto: versión `0.1.0`, edición 2021, licencia
GPL-3.0-or-later, org `equislinux`, repositorio `https://github.com/equislinux/xpkg`.

## Relación con xpm

| Herramienta | Rol | Analogía |
|------|------|---------|
| **xpm** | Gestor de paquetes — instalar, eliminar, actualizar, resolver deps | `pacman` |
| **xpkg** | Constructor de paquetes — compilar, empaquetar, lint, gestionar repos | `makepkg` + `repo-add` + `namcap` |

`xpkg` produce paquetes `.xp` que `xpm` instala. Comparten el mismo formato de
paquete y estructuras de metadata, pero son binarios independientes.

## Estado actual

`xpkg` no es un proyecto pendiente: es el constructor del **endpoint nativo
`.xp`** que `xpm` consume de extremo a extremo.

- **Pipeline del endpoint.** `x-repo/scripts/build-xp.sh` construye y firma
  todos los paquetes X (`xpm`, `xpkg`, `x-release`, `x-dev` vía XBUILD;
  `opencode-bin`, `xfetch-bin`, `xtop-git`, `x-scripts` vía PKGBUILD) y
  regenera `public/x/x86_64` (base de datos, `history.json`, `SHA256SUMS`),
  publicado en GitHub Pages en `https://equislinux.github.io/x-repo/x/x86_64/`.
- **Dos vías paralelas.** El sistema se aprovisiona todavía a través del
  repositorio pacman `[x]` construido con `build-packages.sh` (makepkg +
  `repo-add`); el endpoint nativo `.xp` es el canal orientado a `xpm`. Cada
  release regenera ambos.
- **Consumido de extremo a extremo.** `xpm sync`, `search`, la resolución de
  dependencias e `install` se validaron contra el endpoint publicado; la suite
  de ciclo de vida xpkg↔xpm construye con xpkg e instala/actualiza/elimina con
  xpm.
- **Paridad con makepkg.** Revivir el endpoint cerró huecos reales de paridad en
  xpkg: sources locales junto a la receta, sintaxis `nombre::URL`, expansión de
  variables de receta (incluidos ayudantes privados `_*`) en sources y fases,
  exportación de `CARCH`/`CHOST` a las fases y reset de `$srcdir` antes del
  fetch.

## Madurez del código

La `ROADMAP.md` de este repositorio reporta:

- **Fases 0-3 completas** — workspace Cargo creado, CLI con 8 subcomandos,
  parser de configuración TOML, parsers XBUILD/PKGBUILD, validación de recetas,
  generador de srcinfo, `xpkg new`, descargador HTTP con reintentos,
  verificación SHA-256/512, extracción de archivos (tar.gz/xz/bz2/zst, zip),
  soporte de clonado Git y caché de fuentes. `xpkg build` conecta ese pipeline
  al build: las fuentes declaradas se descargan (cacheadas en
  `~/.cache/xpkg/sources` por defecto), se verifican y se extraen antes de que
  corran las fases, y las fases se ejecutan envueltas en una función shell para
  que `local`/`return` de PKGBUILDs reales funcionen.
- Las **fases posteriores** (motor de build, generación de metadata, creación
  de archivos, linting, gestión de repositorios, comandos verify/info) están
  completas, incluido el mantenimiento y la firma de la base `.files`. La Fase
  9 cerró con los tests de ciclo de vida xpkg↔xpm y el benchmark comparativo
  contra `makepkg` (`cargo test -p xpkg --test bench_makepkg`; metodología y
  números en `docs/BENCHMARKS.md`). La suite suma 265+ tests unitarios más
  tests de integración contra un servidor HTTP local, paquetes reales de
  `makepkg` y archivos gzip/xz. La Fase 10 (objetivos post-v1.0) sigue abierta.
- `docs/realexample.md` registra un run real de extremo a extremo: un paquete
  `xfetch` construido a `.xp` con xpkg, inspeccionado, linteado, agregado a una
  base ALPM local y a un layout `file://` bajo `x-repo`, y sincronizado por
  `xpm`.

## Convención de versiones

| Release | Hito |
|---------|-----------|
| `v0.1.0` | Fases 0-1 — CLI funcional con configuración |
| `v0.3.0` | Fases 2-3 — parseo de recetas y gestión de fuentes |
| `v0.5.0` | Fases 4-6 — motor de build y creación de archivos |
| `v0.7.0` | Fase 7 — framework de linting |
| `v0.9.0` | Fases 8-9 — repositorios e integración |
| `v1.0.0` | Con benchmarks, testeado, listo para producción |

El `Cargo.toml` del workspace declara actualmente la versión `0.1.0` para
ambos crates.
