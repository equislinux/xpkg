# xpkg - Notas de integración futura

Contexto y notas para cuando el tooling Rust de xpkg se re-active dentro del
*reboot* de equislinux. Este documento es deliberadamente prospectivo y se basa en
`DECISIONS.md` y el `ROADMAP.md` del workspace (ambos en la raíz del
workspace, fuera de este repo), más los ítems abiertos del `ROADMAP.md` propio
de este repositorio.

Documentos relacionados en esta carpeta:

- [01 - Resumen y estado](01-overview-and-status.md)
- [02 - Uso](02-usage.md)
- [03 - Arquitectura](03-architecture.md)

---

## Dónde está el empaquetado hoy

Durante el reboot, el payload de la distribución se empaqueta con el toolchain
clásico de Arch:

- Las recetas son **PKGBUILD** construidas con **makepkg** (el paquete
  `x-scripts`, construido en el área de packaging del repo `scripts`).
- Los paquetes resultantes se publican en el repositorio binario **`x-repo`**
  (el mismo layout que xpkg mantendría con `repo-add`).
- El roadmap del workspace mantiene la fase de tooling Rust (**Fase 4 -
  Tooling Rust xpm/xpkg**) **pospuesta**: xpm/xpkg no forman parte del flujo
  de entrega actual.

Por tanto, **el reboot flow no consume xpkg hoy**. Es el constructor futuro
previsto, en estado REBOOT PENDING.

## La decisión guía: ADR-0004

La ADR-0004 (en `DECISIONS.md`) mantiene xpkg/xpm como la base de tooling
propia y actualizada de la distribución (Rust + ALPM + firmado Sequoia) y
declara la consecuencia de que **los paquetes propios se construirán con xpkg
y se servirán desde `x-repo`**. Eso solo ocurre tras cumplir la condición
previa:

> Conectar el resolver SAT de xpm a `install` (deps transitivas) y soportar la
> instalación local de ficheros `.xp`; terminar los stubs de consulta
> pendientes. Hasta entonces, el bootstrap del sistema no debe depender de
> xpm.

Implicación para xpkg: un cambio completo a paquetes construidos con xpkg está
bloqueado en el camino de instalación de `xpm`, no en xpkg mismo. xpkg ya
produce artefactos que el `xpm sync` actual entiende (ver abajo).

## Qué funciona ya de punta a punta

`docs/realexample.md` de este repo documenta una ejecución real que prueba el
camino:

- `xpkg build` produjo un `.xp` real para `xfetch`; `xpkg info` y `xpkg lint`
  lo validaron.
- `xpkg repo-add` creó y actualizó bases de datos ALPM (`.db.tar.gz` y una
  copia `.db`) en layouts estáticos locales, incluido el layout de Pages de
  `x-repo`.
- `xpm sync` contra mirrors `file://` descargó y parseó esas bases de datos.
- La salvedad registrada (primera ejecución) de que la descarga/instalación de
  paquetes era una fase posterior de xpm ya no aplica: `xpm install`/`upgrade`
  resuelven, descargan, verifican e instalan de punta a punta, y las bases
  estilo Arch sin `FILENAME` derivan el nombre convencional
  `name-version-arch.pkg.tar.zst`. Los repos solo-metadatos con artefactos
  hospedados en otro sitio (p. ej. GitHub Releases) siguen requiriendo la
  composición de la URL de fetch en `xpm`.

## Ítems abiertos en este repositorio

Del `ROADMAP.md` de este repositorio (la Fase 9 está completa; sólo queda
abierta la Fase 10):

- ~~Tests de integración con xpm (#56).~~ Hecho: la suite de ciclo xpkg↔xpm
  construye con xpkg e instala/actualiza/elimina con xpm.
- ~~Benchmarks comparativos frente a makepkg (#57).~~ Hecho:
  `cargo test -p xpkg --test bench_makepkg`, resultados en
  `docs/BENCHMARKS.md`.
- Objetivos futuros de la Fase 10 (post-v1.0): split packages desde un solo
  XBUILD, cross-compilación, builds chroot limpios vía namespaces, builds en
  lote en orden de dependencias, integración de helper tipo AUR, soporte de
  paquetes VCS, traducciones.

## Notas para la re-activación

- **Disparador**: retomar cuando vuelva a necesitarse el tooling Rust - los
  disparadores documentados son conectar el resolver SAT de xpm a `install` y
  que un repositorio `.xp` sea el formato servido.
- **Orden**: primero aterrizar la condición previa del lado de xpm
  (ADR-0004), después cambiar el empaquetado de los paquetes propios de
  makepkg/PKGBUILD a xpkg/XBUILD y servir el resultado desde `x-repo`. Reusar
  la infraestructura existente de `x-repo` y el layout estático validado en
  `realexample.md`.
- **Método**: el reboot trabaja en ramas `x/reboot` con commits locales (sin
  push por parte de asistentes); cada fase cierra con tests locales por
  sección antes de pasar a la siguiente; las decisiones se documentan en el
  `DECISIONS.md` del workspace.
- **Evitar duplicación**: xpkg y xpm son binarios independientes que comparten
  un formato de paquete; mantenerlos como las dos herramientas
  complementarias (constructor vs gestor) en lugar de fusionar
  responsabilidades.
- **Compat**: mantener el parsing de PKGBUILD mientras dure la migración para
  poder construir una receta estilo Arch existente sin cambios con
  `xpkg build --pkgbuild`.
