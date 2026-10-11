# xpkg - Notas de integración

Cómo encaja `xpkg` en el flujo de la distribución: lo que ya funciona de
extremo a extremo, las piezas abiertas y los acuerdos que conviene mantener
estables.

Documentos relacionados en esta carpeta:

- [01 - Overview and status](01-overview-and-status.md)
- [02 - Usage](02-usage.md)
- [03 - Architecture](03-architecture.md)

---

## Dónde está el empaquetado hoy

Hay **dos vías vivas** que se regeneran en cada release:

- **Repositorio pacman `[x]`** — `x-repo/build-packages.sh` construye con
  **makepkg + repo-add** los paquetes del sistema (`x-release`, `x-dev`,
  `x-scripts`, `opencode-bin`, `xfetch-bin`, `xtop-git`, `xpm`). Es la vía que
  usa el instalador y los sistemas instalados.
- **Endpoint nativo `.xp`** — `x-repo/scripts/build-xp.sh` construye y firma
  los mismos paquetes con **xpkg** (XBUILD o PKGBUILD) y regenera
  `public/x/x86_64` (DB, `history.json`, `SHA256SUMS`), servido en
  `https://equislinux.github.io/x-repo/x/x86_64/` y consumido por `xpm`.

`xpkg` **no está pendiente ni fuera de alcance**: es el constructor del canal
nativo y su paridad con makepkg se cerró al revivir el endpoint (sources
locales, `nombre::URL`, expansión de variables de receta, `CARCH`/`CHOST` en
fases, reset de `$srcdir`).

## Lo que ya funciona de extremo a extremo

- `xpkg build` produce `.xp` reales; `xpkg info` y `xpkg lint` los validan.
- `xpkg repo-add` crea y mantiene bases ALPM (`.db.tar.gz` + copia `.db`,
  `.files`) en layouts estáticos, incluido el de GitHub Pages.
- `xpm` sincroniza, resuelve dependencias, descarga, verifica e instala tanto
  desde el endpoint nativo como desde bases Arch sin `FILENAME` (deriva el
  nombre convencional `name-version-arch.pkg.tar.zst`).
- La suite de ciclo de vida xpkg↔xpm (`tests/e2e_xpm_lifecycle.rs`) construye
  con xpkg e instala/actualiza/elimina con xpm en un root aislado.

## Pendientes

De la `ROADMAP.md` de este repositorio (la Fase 9 está cerrada; queda la
Fase 10):

- Objetivos post-v1.0: paquetes split desde un XBUILD, compilación cruzada,
  builds en chroot limpio con namespaces, builds por lotes en orden de
  dependencias, integración tipo helper de AUR, soporte de paquetes VCS y
  traducciones.

## Notas

- **Sin duplicar responsabilidades**: `xpm` y `xpkg` son binarios
  independientes que comparten un formato de paquete; mantenerlos como las dos
  herramientas complementarias (builder vs gestor) en lugar de fusionarlos.
- **Compatibilidad**: mantener la vía PKGBUILD para que cualquier receta
  estilo Arch siga construyéndose sin cambios con `xpkg build --pkgbuild`.
- **Formato en lockstep**: `.xp` == estructura ALPM (`tar.zst` con `.PKGINFO`,
  `.BUILDINFO`, `.MTREE`, `.INSTALL` opcional); cualquier cambio de formato
  debe aterrizar a la vez en ambos repos.
