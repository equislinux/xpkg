# xpkg - Uso

Cómo instalar y manejar `xpkg`: instalación, superficie de comandos, flags,
configuración y los flujos típicos de empaquetado.

Documentos relacionados en esta carpeta:

- [01 - Resumen y estado](01-overview-and-status.md)
- [03 - Arquitectura](03-architecture.md)
- [04 - Integración futura](04-future-integration.md)

Para el detalle completo de cada comando y flag, ver los documentos planos
existentes: [CLI Reference](../CLI.md), [XBUILD Specification](../XBUILD.md),
[Packaging Guide](../PACKAGING-GUIDE.md), [Installation Guide](../INSTALLATION.md),
[Source Management](../SOURCES.md), [Package Signing](../SIGNING.md),
[Repository Management](../REPOSITORY.md) y [Linting Rules](../LINTING.md).

---

## Instalación

Requisitos: Rust 1.70+ (edition 2021), Cargo y git. Herramientas runtime
opcionales: `fakeroot` (método preferido para empaquetar sin root,
auto-detectado) y `strip` (stripping de ELF, de binutils).

```bash
git clone https://github.com/equislinux/xpkg.git
cd xpkg
cargo build --release
sudo install -Dm755 target/release/xpkg /usr/local/bin/xpkg
```

Los valores por defecto de configuración funcionan sin ajustes. Para
personalizar, copia `etc/xpkg.conf.example` a `~/.config/xpkg/xpkg.conf` y
edítalo.

## Comandos

| Comando | Descripción |
|---------|-------------|
| `xpkg build` | Construye un paquete `.xp` desde una receta XBUILD o PKGBUILD |
| `xpkg lint <pkg>` | Ejecuta comprobaciones de calidad sobre un paquete construido |
| `xpkg info <pkg>` | Muestra los metadatos del paquete (soporta `--files` y `--json`) |
| `xpkg verify <pkg>` | Verifica la integridad del paquete y su firma OpenPGP |
| `xpkg new <name>` | Genera una plantilla XBUILD nueva |
| `xpkg srcinfo` | Genera salida estilo `.SRCINFO` desde un XBUILD |
| `xpkg repo-add <db> <pkg>` | Añade un paquete a una base de datos de repositorio |
| `xpkg repo-remove <db> <name>` | Quita un paquete de una base de datos de repositorio |

### Flags globales

| Flag | Short | Descripción |
|------|-------|-------------|
| `--config <PATH>` | `-c` | Fichero de configuración propio (por defecto `~/.config/xpkg/xpkg.conf`) |
| `--verbose` | `-v` | Aumenta la verbosidad (`-v`, `-vv`, `-vvv`) |
| `--no-confirm` | — | Omite los prompts de confirmación |
| `--no-color` | — | Desactiva la salida en color |

### build - Construir un paquete

Ejecuta el pipeline completo: parsear receta, obtener fuentes, prepare, build,
check, package, strip, archive, sign.

| Flag | Short | Descripción |
|------|-------|-------------|
| `--file <PATH>` | `-f` | Fichero de receta (por defecto `./XBUILD`) |
| `--pkgbuild` | — | Parsear la receta como PKGBUILD en vez de XBUILD |
| `--builddir <PATH>` | `-d` | Directorio de build (anula la config) |
| `--outdir <PATH>` | `-o` | Directorio de salida del `.xp` (anula la config) |
| `--no-check` | — | Omite la fase `check()` |
| `--sign` | — | Firma el paquete tras construirlo (requiere `sign_key`) |

```bash
xpkg build                             # Construye desde ./XBUILD
xpkg build -f path/to/XBUILD          # Construye desde un fichero concreto
xpkg build --pkgbuild -f ./PKGBUILD   # Construye desde un PKGBUILD
xpkg build --no-check -o ./out        # Omite tests, salida en ./out
xpkg build --sign                      # Construye y firma el paquete
```

### lint - Lint de un archivo de paquete

Extrae el archivo, lee `.PKGINFO` y ejecuta todas las reglas de lint.

| Argumento/Flag | Descripción |
|----------------|-------------|
| `PACKAGE` | Ruta al archivo de paquete `.xp` |
| `--strict` | Trata los warnings de lint como errores (exit code 1) |

```bash
xpkg lint hello-2.12-1-x86_64.xp
xpkg lint hello-2.12-1-x86_64.xp --strict
```

Categorías de lint: permisos, rutas, metadatos, dependencias y análisis ELF.
Ver [Linting Rules](../LINTING.md) para la lista completa.

### info - Mostrar metadatos del paquete

Inspecciona un archivo `.xp` sin instalarlo.

| Argumento/Flag | Short | Descripción |
|----------------|-------|-------------|
| `PACKAGE` | — | Ruta al archivo de paquete `.xp` |
| `--files` | `-l` | Lista todos los ficheros contenidos en el paquete |
| `--json` | — | Salida de metadatos en JSON (legible por máquina) |

```bash
xpkg info hello-2.12-1-x86_64.xp           # Metadatos legibles
xpkg info hello-2.12-1-x86_64.xp --files   # Incluye el listado de ficheros
xpkg info hello-2.12-1-x86_64.xp --json    # Salida JSON para scripting
```

### verify - Verificar la integridad del paquete

Verifica la firma detached OpenPGP de un paquete `.xp`. Busca un fichero
`.xp.sig` junto al paquete.

| Argumento/Flag | Short | Descripción |
|----------------|-------|-------------|
| `PACKAGE` | — | Ruta al archivo de paquete `.xp` |
| `--key <PATH>` | `-k` | Fichero de clave pública o keyring |

```bash
xpkg verify hello-2.12-1-x86_64.xp --key packager.pub
xpkg verify hello-2.12-1-x86_64.xp -k /etc/xpkg/trusted.gpg
```

### new - Crear una plantilla XBUILD

| Argumento/Flag | Short | Descripción |
|----------------|-------|-------------|
| `PKGNAME` | — | Nombre del paquete |
| `--outdir <PATH>` | `-o` | Directorio de salida (por defecto `./<PKGNAME>/`) |

```bash
xpkg new hello                  # Crea hello/XBUILD
xpkg new mylib -o packages/     # Crea packages/XBUILD
```

### srcinfo - Generar source info

Produce salida estilo `.SRCINFO` desde un XBUILD parseado y validado.

| Flag | Short | Descripción |
|------|-------|-------------|
| `--file <PATH>` | `-f` | Fichero XBUILD (por defecto `./XBUILD`) |

```bash
xpkg srcinfo                     # Imprime en stdout
xpkg srcinfo > .SRCINFO          # Escribe la salida a .SRCINFO
```

### repo-add / repo-remove - Gestión de repositorio

Gestiona una base de datos compatible con ALPM (`myrepo.db.tar.zst` por
defecto; también `.db.tar.gz` y `.db.tar.xz`, auto-detectados por extensión).
La base de datos se crea automáticamente en el primer add.

```bash
xpkg repo-add myrepo.db.tar.zst hello-2.12-1-x86_64.xp
xpkg repo-add myrepo.db.tar.zst hello-2.12-1-x86_64.xp --sign

xpkg repo-remove myrepo.db.tar.zst hello
xpkg repo-remove myrepo.db.tar.zst hello --sign
```

Añadir un nombre de paquete ya existente reemplaza la entrada por la nueva
versión. Ver [Repository Management](../REPOSITORY.md) para instrucciones de
hospedaje.

## Códigos de salida

| Código | Significado |
|--------|-------------|
| `0` | Éxito |
| `1` | Error general (receta inválida, fallo de build, errores de lint, firma mala) |
| `2` | Uso inválido (argumentos ausentes, flags desconocidos) |

## Variables de entorno

| Variable | Descripción |
|----------|-------------|
| `RUST_LOG` | Anula el filtro de nivel de log de tracing (p. ej. `RUST_LOG=debug`) |

Durante los builds, estas variables se exponen a los scripts de build:

| Variable | Descripción |
|----------|-------------|
| `PKGDIR` | Directorio destino de los ficheros instalados |
| `SRCDIR` | Directorio con los ficheros fuente extraídos |
| `BUILDDIR` | Directorio de build de nivel superior |
| `MAKEFLAGS` | Flags de make desde la config |
| `CFLAGS` | Flags del compilador C desde la config |
| `CXXFLAGS` | Flags del compilador C++ desde la config |
| `LDFLAGS` | Flags del linker desde la config |

## Configuración

Fichero de configuración: `~/.config/xpkg/xpkg.conf` (TOML), o cualquier ruta
pasada con `--config`. Secciones clave:

- `[options]` - builddir, outdir, sign, sign_key, método/nivel de compresión,
  strip_binaries
- `[environment]` - MAKEFLAGS, CFLAGS, CXXFLAGS, LDFLAGS
- `[lint]` - activar/desactivar linting, modo estricto

```toml
[options]
builddir = "/tmp/xpkg-build"
outdir = "."
strip_binaries = true
compress = "zstd"       # zstd | gzip | xz
compress_level = 19

[environment]
makeflags = "-j$(nproc)"
cflags = "-march=x86-64 -O2 -pipe"
cxxflags = "-march=x86-64 -O2 -pipe"
```

Ver `etc/xpkg.conf.example` para todas las opciones.

## Flujos típicos

1. **Paquete nuevo** - `xpkg new hello`, editar `hello/XBUILD`, `xpkg build`,
   inspeccionar con `xpkg info`, comprobar calidad con `xpkg lint`, instalar
   con `sudo xpm install hello-...-x86_64.xp`.
2. **Compatibilidad Arch** - conservar un PKGBUILD existente y ejecutar
   `xpkg build --pkgbuild -f ./PKGBUILD`.
3. **Publicación** - `xpkg repo-add x.db.tar.zst pkg-...-x86_64.xp` dentro del
   directorio hospedado; opcionalmente `--sign` de la base de datos.
4. **Configuración de firma** - exportar una clave secreta a
   `~/.config/xpkg/signing.key`, fijar `sign = true` y `sign_key` en la config,
   construir con `--sign`.
