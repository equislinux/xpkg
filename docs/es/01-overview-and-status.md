# xpkg - Resumen y estado

Este es uno de los cuatro documentos que describen `xpkg`, el constructor de
paquetes en Rust de la distribución X. Cubre qué es la herramienta, su rol en
el ecosistema X y su estado honesto dentro de la iniciativa *reboot* de equislinux.

Documentos relacionados en esta carpeta:

- [02 - Uso](02-usage.md)
- [03 - Arquitectura](03-architecture.md)
- [04 - Integración futura](04-future-integration.md)

---

## Qué es xpkg

`xpkg` es el **constructor de paquetes de la distribución X**. Lee recetas de
construcción (ficheros **XBUILD** o **PKGBUILD** heredados), obtiene fuentes,
compila software en un entorno aislado y produce paquetes `.xp` listos para
instalar con `xpm`. Es el compañero de desarrollo de `xpm`.

Piensa en él como el equivalente a `makepkg` + `repo-add` + `namcap` para el
ecosistema X, escrito íntegramente en Rust.

| Característica | Descripción |
|----------------|-------------|
| Rust puro | Cero dependencias C - coherente con el ecosistema xpm |
| Formato XBUILD | Recetas declarativas en TOML, alternativa moderna a PKGBUILD |
| Compatibilidad PKGBUILD | Construye sin fricción desde ficheros PKGBUILD de Arch Linux |
| Builds fakeroot | Empaquetado aislado sin privilegios de root reales (unshare / fakeroot / tar-rewrite) |
| Firmado de paquetes | Firmas OpenPGP detached vía sequoia-openpgp (Rust puro) |
| Linting | Comprobaciones de calidad: dependencias, permisos, rutas, metadatos, análisis ELF |
| Herramientas de repo | Crear y gestionar bases de datos de paquetes compatibles con ALPM para `xpm` |
| Gestión de fuentes | Descarga HTTP con reintentos, verificación SHA-256/512, clonado Git, caché local |

Metadatos del proyecto: versión `0.1.0`, edition 2021, licencia
GPL-3.0-or-later, organización `equislinux`, repositorio `https://github.com/equislinux/xpkg`.

## Relación con xpm

| Herramienta | Rol | Analogía |
|-------------|-----|----------|
| **xpm** | Gestor de paquetes - instalar, quitar, actualizar, resolver deps | `pacman` |
| **xpkg** | Constructor de paquetes - compilar, empaquetar, lint, gestionar repos | `makepkg` + `repo-add` + `namcap` |

`xpkg` produce paquetes `.xp` que instala `xpm`. Ambos comparten el mismo
formato de paquete y las mismas estructuras de metadatos, pero son binarios
independientes.

## Estado dentro del reboot de equislinux

La iniciativa *reboot* de equislinux (ver `ROADMAP.md` y `DECISIONS.md` en la raíz
del workspace, fuera de este repo) reorganizó la organización y marcó una
nueva dirección para la distribución. Su estatus para el tooling Rust es:

- La fase **"Fase 4 - Tooling Rust (xpm/xpkg)"** del roadmap del workspace
  está **POSPUESTA**. Por decisión del mantenedor, xpm/xpkg **no se trabajan
  durante el reboot**; el tooling se retoma solo cuando vuelva a necesitarse el
  stack Rust (resolver SAT, repositorio `.xp`).
- El empaquetado del payload de la distribución durante el reboot se hace con
  **PKGBUILD + makepkg** (paquete `x-scripts`), publicado en el repositorio
  binario **`x-repo`**. El reboot flow **no usa xpkg actualmente**.
- La decisión **ADR-0004** registra que xpkg/xpm siguen siendo la base
  prevista de empaquetado y gestión de paquetes de la distribución, con una
  condición previa: el resolver SAT de `xpm` debe conectarse a `install` y
  deben funcionar las instalaciones locales de ficheros `.xp` antes de
  depender de `xpm` para el bootstrap del sistema.

En resumen: xpkg está en estado **REBOOT PENDING**. El código es funcional y
se mantiene como futuro constructor de paquetes, pero está des-priorizado por
ahora y no participa en el flujo de entrega actual del reboot.

## Madurez del código

El `ROADMAP.md` propio del repositorio informa de:

- **Sección "Current Status"**: fases 0-3 completas - workspace de Cargo
  montado, CLI con 8 subcomandos, parser de configuración TOML, parsers
  XBUILD/PKGBUILD, validación de recetas, generador de srcinfo, `xpkg new`,
  descargador HTTP con reintentos, verificación de checksums SHA-256/512,
  extracción de archivos (tar.gz/xz/bz2/zst, zip), soporte de clonado Git y
  caché de fuentes, con 72 tests unitarios pasando.
- Las **listas de fases** del mismo fichero marcan las fases posteriores
  (motor de build, generación de metadatos, creación de archivos, linting de
  paquetes, gestión de repositorios, comandos verify/info) como completas,
  quedando dos ítems abiertos en la Fase 9: tests de integración con `xpm` y
  benchmarks comparativos frente a `makepkg`. La Fase 10 (objetivos futuros
  post-v1.0) está íntegramente abierta.
- `docs/realexample.md` registra una ejecución real de punta a punta: se
  construyó un paquete `xfetch` a `.xp` con xpkg, se inspeccionó, se hizo lint,
  se añadió a una base de datos ALPM local y a un layout de repositorio
  `file://` bajo `x-repo`, y `xpm` lo sincronizó.

## Convención de versiones

| Release | Hito |
|---------|------|
| `v0.1.0` | Fases 0-1 - CLI funcional con configuración |
| `v0.3.0` | Fases 2-3 - parsing de recetas y gestión de fuentes |
| `v0.5.0` | Fases 4-6 - motor de build y creación de archivos |
| `v0.7.0` | Fase 7 - framework de linting |
| `v0.9.0` | Fases 8-9 - tooling de repositorio e integración |
| `v1.0.0` | Benchmarked, probado, listo para producción |

El `Cargo.toml` del workspace declara actualmente la versión `0.1.0` para
ambos crates.
