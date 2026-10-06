# Siguientes pasos

Nota de trabajo, no especificación: dice qué está en marcha y qué toca después para acercar DevHQ a
[docs/vision.md](vision.md), de modo que una sesión nueva (de Claude Code, de Codex o de quien sea)
pueda seguir sin la conversación que la escribió. El [README](../README.md) sigue mandando. Cuando un
paso se termina, sale de aquí en el mismo commit; cuando no quede ninguno, esta nota se borra.

Escrita el 6 oct 2026 desde una sesión local en la laptop con Windows.

## El plan, en corto

Revisado `main` contra la visión, el siguiente paso estructural es que el inventario tenga tiempo y
autor, como ya los tiene la evidencia: hoy una observación dice quién, cuándo y hasta cuándo, pero
editar una aplicación pisa el valor anterior sin rastro. Por eso, en orden:

0. **Consolidar**: la visión y la puerta para agentes en `main`. Cualquier migración nueva en `main`
   chocaría con las v7–v9 de la puerta.
1. **Registro de cambios común** (hecho en una rama, ver abajo): cada cambio de configuración, tuyo
   o de un agente, en un solo registro con el autor y cada campo antes y después.
2. **Referencias**: anclar cada sistema a lo que es en el mundo real (repositorio, copia local en
   una estación de trabajo, documentación, logs, panel).
3. **Primera fuente observada sobre esas anclas**: la actividad de los repositorios locales, leída
   cuando se pide.

Después, según lo que pidan preguntas concretas: bases de datos compartidas y dominios como tipos de
recurso, relaciones nuevas, despliegues con versión, commit y destino, credenciales, operaciones y
supervisión con la ventana cerrada, en el orden de «Líneas de evolución posibles» de la visión.

No conviene hacer todavía: una tabla genérica de entidades, un motor de grafos, una entidad
«proyecto», una capa de «integraciones» antes de tener una segunda fuente real, event sourcing,
permisos por agente ni descubrir repositorios por su cuenta.

## Estado

- [#2](https://github.com/jpgarcia04/DevHQ/pull/2), la visión y `AGENTS.md`: **fusionado** en
  `main`.
- [#3](https://github.com/jpgarcia04/DevHQ/pull/3), la puerta para agentes
  (`claude/practical-hamilton-2tkqys`, esquema v9): **abierto**. Su primera ejecución del workflow
  de Windows falló por dos cosas que ya tienen arreglo empujado, y el CI volvió a correr:
  - `f1a46d3`: en el runner `TMP` es un nombre corto 8.3 (`C:\Users\RUNNER~1\…`), y una prueba de la
    puerta veía el inventario «en otro sitio» (`storedAt`). Las pruebas de la puerta usan ahora
    `resolved_temp_dir()`, como las de respaldo. Además una prueba dejaba su carpeta en `TMP`, que la
    segunda pasada rechaza. Reproducido y comprobado en la laptop con un `TMP` corto, con espacios y
    acentos: 93 pruebas en verde, nada dejado atrás.
  - `ea99642`: al final del recorrido nativo, justo después de cerrarse DevHQ, la puerta aún veía el
    lock de la ventana. Windows suelta los locks de un proceso muerto «según sus recursos»; el
    recorrido ahora espera hasta 10 s, como ya hacía la prueba del lock, y anota cuánto tardó. **Es
    una hipótesis**: si vuelve a fallar en ese paso tras 10 s, algo retiene el lock (un proceso hijo
    de WebView2, por ejemplo) y hay que investigarlo en el log del job «Installer, native walk…».
- `claude/common-change-log`: **la etapa 1, sin PR todavía**. Un commit (`21a4df5`) sobre la puerta
  con las dos correcciones: esquema v10, `change_log` en lugar de `agent_log`, cada caso de uso
  registra su cambio en su transacción (la ventana como `manual`, la puerta como `agent`), Activity
  lo muestra y dice desde cuándo está completo. El README («Registro de cambios») lo describe. En la
  laptop pasaron Prettier, ESLint, `tsc -b`, Vitest (166), build, `npm run e2e` (30), `cargo fmt`,
  clippy y las pruebas de Rust (97). No se corrió `npm run e2e:native`, que en la laptop se niega a
  correr porque `%APPDATA%` ya tiene datos de DevHQ; lo correrán el CI de Linux y el de Windows.

## Qué hacer, en orden

1. **Fusionar la puerta.** `gh pr checks 3`. Si todo está en verde, `gh pr merge 3 --merge`. Si falla,
   leer el job que falló (`gh run view <id> --log-failed`, o la API de jobs si la ejecución sigue en
   curso), arreglarlo en `claude/practical-hamilton-2tkqys` con las comprobaciones de `CLAUDE.md`, y
   empujar una sola vez: cada push relanza Windows (unos 22 min con caché, 48 sin ella).
2. **Llevar el registro de cambios a `main`.** Con la puerta ya fusionada: en
   `claude/common-change-log`, `git fetch` y `git rebase origin/main` (o fusionar `main`). Después:
   - Actualizar [docs/vision.md](vision.md), que se escribió antes de este paso: el principio 12 dice
     que tus cambios de configuración todavía no dejan rastro; «Estado actual e historia» habla de
     `agent_log` en la rama de la puerta; en «Preguntas que debe poder responder», la fila «¿Cómo
     llegó esto a su estado actual?» dice que solo hay cambios de agentes. Añadir una línea en su
     «Historial».
   - Pasar todas las comprobaciones de `CLAUDE.md`, empujar y abrir el PR contra `main`. Fusionar
     cuando el CI de Linux y el de Windows estén en verde.
   - Quitar de esta nota lo que ya esté hecho, en ese mismo PR.
3. **Etapa 2, referencias.** Antes de escribir código, proponer el diseño a la persona y esperar su
   visto bueno. El punto de partida:
   - Una tabla que cuelga de una aplicación o un recurso con las dos columnas de siempre
     (`application_id`/`resource_id` y el CHECK de exactamente una), configuración como
     `http_checks`: se va con su sujeto (`CASCADE`) y no es evidencia. Migración v11 con su prueba
     de actualización fila por fila.
   - Tipos que son conceptos, no proveedores: repositorio, copia local, documentación, logs y panel.
     URL o ruta; las URL con usuario o contraseña se rechazan, como en la comprobación HTTP.
   - La copia local apunta además a un recurso `workstation`, porque el mismo proyecto puede estar
     clonado en varias máquinas. Saber en qué estación corre DevHQ no hace falta hasta la etapa 3.
   - Un monorepo es la misma URL de repositorio desde varias aplicaciones, sin entidad nueva.
   - Cada cambio de referencias se registra en `change_log` como cualquier otro cambio de
     configuración (añadir sus campos y acciones).
   - La puerta las muestra en `get_system` y las deja editar, por los mismos casos de uso que la
     ventana: es el primer contexto real para un agente que entra a un proyecto.
   - Abrir la carpeta o el enlace solo como subproducto, nunca como motivo.
4. **Etapa 3, actividad local**, después de las referencias. Dos decisiones que tomar con la persona
   antes: la actividad (último commit, cambios sin confirmar, commits sin publicar) no es salud, así
   que no cabe en una observación healthy/degraded/unhealthy y necesita su propia forma de evidencia;
   y choca con la regla «no se implementan scanners», que habría que reescribir de forma explícita
   (leer a petición una ruta que el inventario ya conoce no es escanear). Solo refs locales, sin red
   ni credenciales.

## Lo que solo se puede hacer en la laptop

- Lo pendiente de la puerta en [docs/puerta-para-agentes.md](puerta-para-agentes.md): un Claude Code
  real contra el `devhq.exe` instalado y una puerta lanzada desde la app de escritorio de Claude
  (MSIX). La persona prefiere no instalar DevHQ hasta que sea usable.
- `npm run e2e:native` en Windows, que necesita apartar antes los datos de `%APPDATA%` y
  `%LOCALAPPDATA%`.

Una sesión en la nube corre todo lo demás, incluido `npm run e2e:native` sobre WebKitGTK en Linux
(ver «Cloud sessions» en `CLAUDE.md`), y el workflow de Windows prueba lo suyo en cada PR.
