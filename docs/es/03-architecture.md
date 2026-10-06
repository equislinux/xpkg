# xpkg - Arquitectura

Cómo está estructurado el proyecto, cómo se produce un paquete y los formatos
que lee y escribe.

Documentos relacionados en esta carpeta:

- [01 - Resumen y estado](01-overview-and-status.md)
- [02 - Uso](02-usage.md)
- [04 - Integración futura](04-future-integration.md)

---

## Layout del workspace de Cargo

`xpkg` es un workspace de Cargo (edition 2021) con dos crates:

| Crate | Tipo | Rol |
|-------|------|-----|
| `crates/xpkg` | Binario | Frontend CLI: `main.rs` (punto de entrada, dispatch, logging, carga de config) y `cli.rs` (definiciones clap) |
| `crates/xpkg-core` | Librería | Toda la lógica de negocio; re-exporta `XpkgConfig`, `XpkgError`, `XpkgResult` desde su `lib.rs` |

El `Cargo.toml` raíz del workspace centraliza dependencias y metadatos
compartidos (versión `0.1.0`, edition 2021, GPL-3.0-or-later, org `equislinux`).
Dependencias de terceros destacadas: `clap` (CLI), `serde`/`serde_json`/`toml`,
`thiserror`/`anyhow` (errores), `tracing` (logging), `ureq` (HTTP),
`sha2` (checksums), `flate2`/`tar`/`xz2`/`bzip2`/`zstd`/`zip` (archivos),
`sequoia-openpgp` (firma), `tempfile` (tests).

## Módulos de `xpkg-core`

| Módulo | Responsabilidad | Ficheros clave |
|--------|-----------------|----------------|
| `config` | Parser de configuración TOML (`XpkgConfig`) | `config.rs` |
| `error` | Tipos de error (`XpkgError`, `XpkgResult`) | `error.rs` |
| `recipe` | Parsing XBUILD y PKGBUILD, validación, srcinfo, plantillas `new` | `recipe/{mod,types,validate,xbuild,pkgbuild}.rs` |
| `source` | Descarga, checksums, extracción, git, caché | `source/{mod,download,checksum,extract,git,cache}.rs` |
| `builder` | Pipeline de build + fakeroot + dirs/env/exec/log del build | `builder/{mod,dirs,env,exec,log,pipeline,types}.rs` |
| `metadata` | Generación de `.PKGINFO`, `.BUILDINFO`, `.MTREE`, `.INSTALL` | `metadata/{mod,pkginfo,buildinfo,mtree,install}.rs` |
| `archive` | Creación del archivo `.xp` y stripping de ELF | `archive/{mod,pack,strip}.rs` |
| `lint` | Framework de linting + reglas (permisos, rutas, metadatos, dependencias, ELF) | `lint/{mod,rules,permissions,paths,metadata,dependency,elf,report}.rs` |
| `signing` | Firma/verificación OpenPGP (sequoia-openpgp) | `signing/{mod,keys,sign,verify}.rs` |
| `repo` | Gestión de bases de datos de repositorio (leer/escribir, add/remove, inspect, deploy) | `repo/{mod,types,desc,db,inspect,deploy}.rs` |

## El pipeline de build

`xpkg build` orquesta, en orden:

1. Parsear y validar la receta (XBUILD o PKGBUILD).
2. Aplicar las anulaciones CLI de builddir/outdir.
3. Preparar los directorios de build aislados y el entorno.
4. Ejecutar las fases de build: `prepare`, luego `build`, luego `check`
   (opcional) y luego `package`, ejecutando cada fase de la receta como
   scripts de shell.
5. Hacer stripping de binarios ELF (si `strip_binaries = true`).
6. Crear el archivo `.xp` (tar.zst por defecto).
7. Firmar el paquete (si `--sign` o `sign = true` en la config).

### Empaquetado sin root

La fase `package()` escribe dentro de un contexto fakeroot para que los
ficheros se registren con `uid=0`/`gid=0` sin privilegios reales de root.
xpkg usa un fallback de 3 capas: `unshare --user` (namespaces del kernel,
Linux >= 3.8) cuando está disponible, si no la herramienta `fakeroot`, y si no
ejecución directa con reescritura de cabeceras tar.

### Entorno

El builder fija `PKGDIR`, `SRCDIR`, `BUILDDIR`, `MAKEFLAGS`, `CFLAGS`,
`CXXFLAGS` y `LDFLAGS` para los scripts de fase. La fase `package()` debe
instalar todo en `$PKGDIR` (nunca en `/`).

## El formato de paquete `.xp`

`.xp` es un archivo tar comprimido compatible con ALPM (tar.zst por defecto).
En la raíz del archivo lleva los ficheros de metadatos que genera el módulo
`metadata`:

```text
package-1.0-1-x86_64.xp (tar.zst)
+-- .PKGINFO       identidad del paquete, versión, dependencias, tamaños
+-- .BUILDINFO     registro del entorno de build (packager, builddate, toolchain)
+-- .MTREE          manifiesto de integridad de ficheros (hashes, permisos, ownership, symlinks)
+-- .INSTALL        scripts de hook opcionales pre/post install/upgrade/remove
+-- usr/            árbol de ficheros instalados
+-- ...
```

La firma opcional produce un fichero de firma detached OpenPGP junto al
archivo (`package-...-x86_64.xp.sig`).

## Formato de base de datos de repositorio

Un repositorio es un conjunto de paquetes `.xp` más un índice de base de datos
que `xpm` puede consultar: un archivo tar comprimido compatible con ALPM
(`.db.tar.zst` por defecto; `.db.tar.gz` y `.db.tar.xz` se auto-detectan).
Dentro, un directorio por paquete contiene `desc` (metadatos del paquete,
`%FILENAME%`, `%NAME%`, `%VERSION%`, `%DESC%`, tamaños, checksum, ...) y
`depends` (información de dependencias), en un formato clave-valor compatible
con ALPM. El módulo `repo` lee/escribe esas bases de datos y puede generar un
layout de repositorio estático para hospedaje HTTP.

## El formato de receta XBUILD

XBUILD es el formato de receta TOML nativo (fichero `XBUILD`, TOML v1.0,
UTF-8), estructurado en cuatro secciones de nivel superior. Ver la
[XBUILD Specification](../XBUILD.md) completa.

| Sección | Propósito | Campos clave |
|---------|-----------|--------------|
| `[package]` | Identidad y metadatos (obligatoria) | name, version, release, description, url, license, arch, provides, conflicts, replaces |
| `[dependencies]` | Declaraciones de dependencias (opcional) | depends, makedepends, checkdepends, optdepends |
| `[source]` | Fuentes e integridad (opcional) | urls, sha256sums, sha512sums, patches |
| `[build]` | Scripts de shell por fase (opcional) | prepare, build, check, package |

Ejemplo:

```toml
[package]
name = "hello"
version = "2.12"
release = 1
description = "GNU Hello - the friendly greeter"
url = "https://www.gnu.org/software/hello/"
license = ["GPL-3.0-or-later"]
arch = ["x86_64"]

[dependencies]
depends = ["glibc"]
makedepends = ["gcc", "make"]

[source]
urls = ["https://ftp.gnu.org/gnu/hello/hello-2.12.tar.gz"]
sha256sums = ["cf04af86dc085268c5f4470fbae49b18afbc221b78096aab842d934a76bad0ab"]

[build]
build = """
cd hello-2.12
./configure --prefix=/usr
make
"""
package = """
cd hello-2.12
make DESTDIR=$PKGDIR install
"""
```

Reglas de validación que aplica el parser: `name` debe cumplir las reglas de
nombrado (inicio ASCII en minúscula, minúsculas/dígitos/guiones/bajos, máx
128); `version` no vacío; `release` >= 1; `arch` en `x86_64`, `aarch64`,
`i686`, `armv7h`, `any`; esquemas de URL de fuente en `http`, `https`, `ftp`,
`file`; los arrays de checksums deben coincidir en longitud con `urls`. Los
errores se recogen y se reportan juntos, no uno a uno.

### Compatibilidad PKGBUILD

`xpkg build --pkgbuild` parsea scripts bash PKGBUILD heredados de Arch Linux y
extrae variables (`pkgname`, `pkgver`, `pkgrel`, arrays de depends, `source`,
`sha256sums`) y funciones (`prepare`, `build`, `check`, `package`) para la
migración desde el ecosistema Arch.

### Gestión de fuentes

Las fuentes declaradas se descargan (HTTP/HTTPS/FTP/file vía `ureq`, repos
Git vía el `git` del sistema), se verifican con SHA-256 y/o SHA-512 (cada
entrada emparejada por índice con `urls`, `SKIP` la omite), se extraen por
extensión (tar.gz/tgz, tar.xz/txz, tar.bz2/tbz2, tar.zst/tzst, zip; el resto
de ficheros se conservan tal cual), y se cachean en
`$XDG_CACHE_HOME/xpkg/sources/` (clave = SHA-256 truncado de la URL) para
evitar re-descargas. Ver [Source Management](../SOURCES.md).

## Modelo de configuración

La configuración vive en `~/.config/xpkg/xpkg.conf` (TOML) con secciones
`[options]`, `[environment]` y `[lint]`, se carga al arrancar y se usa para
construir un `XpkgConfig`. Los subcomandos clonan la config cargada y aplican
anulaciones CLI antes de actuar. Ver `etc/xpkg.conf.example` para todas las
opciones.

## Receta de auto-hospedaje

El repositorio lleva su propia receta de build en `packaging/xpkg/XBUILD`:
copia el árbol del repo al directorio de fuentes, ejecuta
`cargo build -p xpkg --release --locked`, e instala el binario, la licencia,
el README y `etc/xpkg.conf.example` en el paquete - un ejemplo de receta local
sin `[source]`.
