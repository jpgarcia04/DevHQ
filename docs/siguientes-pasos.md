# Siguientes pasos

Nota de trabajo, no especificación: dice qué está en marcha y qué toca después para acercar DevHQ a
[docs/vision.md](vision.md), de modo que una sesión nueva (de Claude Code, de Codex o de quien sea)
pueda seguir sin la conversación que la escribió. El [README](../README.md) sigue mandando. Cuando un
paso se termina, sale de aquí en el mismo commit; cuando no quede ninguno, esta nota se borra.

Escrita el 6 oct 2026 desde una sesión local en la laptop con Windows, y puesta al día esa noche, al
fusionar la puerta y abrir el PR del registro de cambios.

## El plan, en corto

Revisado `main` contra la visión, el siguiente paso estructural es que el inventario tenga tiempo y
autor, como ya los tiene la evidencia: hoy una observación dice quién, cuándo y hasta cuándo, pero
editar una aplicación pisa el valor anterior sin rastro. Por eso, en orden:

0. **Consolidar** (hecho): la visión y la puerta para agentes (esquema v9) están en `main`.
1. **Registro de cambios común** (en revisión, ver abajo): cada cambio de configuración, tuyo o de
   un agente, en un solo registro con el autor y cada campo antes y después.
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

- [#2](https://github.com/jpgarcia04/DevHQ/pull/2) (la visión y `AGENTS.md`) y
  [#3](https://github.com/jpgarcia04/DevHQ/pull/3) (la puerta para agentes, esquema v9) están
  **fusionados** en `main`; #3 entró (`2e99c21`) con todo el CI de Linux y de Windows en verde. En
  Windows se confirmaron los dos arreglos de su primera ejecución: las pruebas de Rust con un `TMP`
  corto 8.3 (`f1a46d3`) y la espera al lock de la ventana (`ea99642`). La hipótesis del lock era
  cierta: la puerta dejó de ver la ventana 109 ms después de que DevHQ terminara, y antes miraba en
  el acto.
- **Un fallo intermitente sin explicar**, en el primer intento del job «Installer, native walk…» de
  esa ejecución: el primer paso de la caminata («Mi inventario opens on the v6 inventory») esperó
  15 s a «Nothing is registered yet.» y no lo vio. No era `findAll`: la captura `failure.png` muestra
  «Loading local inventory...», así que la primera lectura del inventario (`list_inventory`, la carga
  en la que DevHQ actualiza el archivo de v6 y lo pasa a WAL) no había respondido; el volcado de
  texto, un instante después, ya mostraba el inventario vacío. Además pasaron 74 s entre el arranque
  de msedgedriver y el fallo, cuando en una ejecución normal todo el paso cabe en 3 o 4 s: no solo
  esperó la carga, también algo de WebDriver. Al relanzar el job, el paso pasó en 901 ms. Hipótesis
  sin comprobar: `list_inventory` es síncrono y corre en el hilo principal, que con WebView2 también
  atiende la ventana, y en esa primera carga compite con la supervisión y con `notice_changes`, que
  abren el mismo archivo a la vez; cada espera de lock está acotada a 5 s, así que eso solo no
  explica un minuto, y el antivirus en tiempo real puede pesar.
- `claude/common-change-log`, [#4](https://github.com/jpgarcia04/DevHQ/pull/4): **la etapa 1, en
  revisión**, rebasada sobre `main`. Esquema v10, `change_log` en lugar de `agent_log`, cada caso de
  uso registra su cambio en su transacción (la ventana como `manual`, la puerta como `agent`),
  Activity lo muestra y dice desde cuándo está completo; el README («Registro de cambios») lo
  describe y [docs/vision.md](vision.md) ya no lo trata como pendiente. Trae también un diagnóstico
  para el fallo de arriba: la caminata dice cuánto tarda la ventana en poder manejarse tras arrancar
  DevHQ y, en el primer paso y en cualquier paso que falle, si la página está visible, cuánto lleva
  cargada y cuándo se pidió y se respondió cada llamada a DevHQ. En la laptop, sobre la rama
  rebasada, pasaron Prettier, ESLint, `tsc -b`, Vitest (166), build, `npm run e2e` (30),
  `cargo fmt`, clippy y las pruebas de Rust (97). `npm run e2e:native` se negó a correr, como debe,
  porque `%APPDATA%` ya tiene datos de DevHQ: la caminata la corren el CI de Linux y el de Windows.

## Qué hacer, en orden

1. **Fusionar el registro de cambios** (#4) cuando el CI de Linux y el de Windows estén en verde
   (`gh pr checks 4`), con el visto bueno de la persona. Antes, leer en los logs de las dos caminatas
   (WebKitGTK y «Installer, native walk…») las líneas nuevas del primer paso: si dicen «none
   recorded», ese WebView no registra las llamadas a DevHQ y el diagnóstico hay que cambiarlo; si no,
   anotar aquí cuánto tardó `list_inventory`. Si un job falla, `gh run view <id> --log-failed`,
   arreglarlo en la rama con las comprobaciones de `CLAUDE.md` y empujar una sola vez: cada push
   relanza Windows (unos 22 min con caché, 48 sin ella).
   - Si vuelve el fallo del primer paso, leer su «window state». Si `list_inventory` falta mientras
     otras llamadas sí aparecen, o se respondió tarde, lo retiene Rust: buscar qué (las tres
     conexiones de la primera carga, la conversión a WAL, el antivirus); hacer `list_inventory`
     asíncrono, como ya son los comandos que esperan, al menos dejaría la ventana atendida mientras
     tanto. Si se respondió pronto y la página no lo mostró, el retraso está en la página o en el
     driver: `visibility hidden` apuntaría a que WebView2 no pinta una ventana que nadie ve.
2. **Etapa 2, referencias.** Antes de escribir código, proponer el diseño a la persona y esperar su
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
3. **Etapa 3, actividad local**, después de las referencias. Dos decisiones que tomar con la persona
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
