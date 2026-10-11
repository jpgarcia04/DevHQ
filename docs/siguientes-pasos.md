# Siguientes pasos

Nota de trabajo, no especificación: dice qué está en marcha y qué toca después para acercar DevHQ a
[docs/vision.md](vision.md), de modo que una sesión nueva (de Claude Code, de Codex o de quien sea)
pueda seguir sin la conversación que la escribió. El [README](../README.md) sigue mandando. Cuando un
paso se termina, sale de aquí en el mismo commit; cuando no quede ninguno, esta nota se borra.

Escrita el 6 oct 2026 desde una sesión local en la laptop con Windows, puesta al día esa noche, al
fusionar el registro de cambios y proponer el diseño de las referencias, y el 7 oct, al implementar
las referencias con las decisiones que la persona aprobó y al ver pasar su CI; y el 11 oct, al probar
cómo se puede leer una copia local sin que leerla ejecute nada, proponer el diseño de la etapa 3,
ponerlo al día sobre `main` tras fusionarse las referencias, revisarlo con las cuestiones de la
persona y, al final del día, fusionarlo con la auditoría de Codex resuelta.

## El plan, en corto

Revisado `main` contra la visión, el siguiente paso estructural era anclar cada sistema a lo que es
en el mundo real. La visión, la puerta para agentes (esquema v9), el registro de cambios común (v10),
las referencias (v11) y la primera fuente observada sobre ellas, la actividad de las copias locales
(etapa 3, v12), están en `main`: cada aplicación y recurso dice dónde está su repositorio, sus copias
locales, su documentación, sus logs y su panel, y DevHQ lee esas copias cuando se le pide, sin que
leerlas ejecute nada de lo que se lee.

**La próxima prioridad no está decidida.** Se elegirá después de evaluar el producto tal como está y
contrastarlo con la visión y sus preguntas. Hasta entonces, nada de lo que esta nota o la visión
mencionan es un compromiso: bases de datos compartidas y dominios como tipos de recurso, relaciones
nuevas, despliegues con versión, commit y destino, credenciales, operaciones y supervisión con la
ventana cerrada («Líneas de evolución posibles» de la visión) son posibilidades, y la que se haga
saldrá de una pregunta concreta.

No conviene hacer todavía: una tabla genérica de entidades, un motor de grafos, una entidad
«proyecto», una capa de «integraciones» antes de tener una segunda fuente real, event sourcing,
permisos por agente ni descubrir repositorios por su cuenta.

## Estado

- [#2](https://github.com/jpgarcia04/DevHQ/pull/2) (la visión),
  [#3](https://github.com/jpgarcia04/DevHQ/pull/3) (la puerta, v9),
  [#4](https://github.com/jpgarcia04/DevHQ/pull/4) (el registro de cambios, v10) y
  [#5](https://github.com/jpgarcia04/DevHQ/pull/5) (las referencias, etapa 2, v11) están
  **fusionados** en `main`. #4 entró (`1a40caf`) con todo el CI de Linux y de Windows en verde al
  primer intento, y #5 también (`aff9dd7`, 11 oct): los 5 checks de Linux y los 3 jobs de Windows (el
  previo; «Rust tests · NTFS, SChannel…», 13 min; «Installer, native walk…», 18 min). Lo que la
  persona fijó al aprobar las referencias y queda en el README («Referencias»): se editan con su
  sistema, como los componentes, y el registro de cambios las cubre con un campo `references` en las
  acciones de siempre; la v11 solo copia el destino; `repository` es solo una URL web http(s),
  comparada por igualdad exacta; un destino es una URL http(s) o una ruta absoluta, y la etiqueta es
  un nombre para personas, nunca un localizador. Son inventario declarado, no evidencia: no llevan
  `source`.
- **La etapa 3**, la actividad de las copias locales (esquema v12), está **fusionada** en `main` con
  [#6](https://github.com/jpgarcia04/DevHQ/pull/6) (`db82470`, 11 oct), con los 4 jobs del CI de
  Linux y los 3 de Windows en verde sobre su código final y los dos defectos que encontró la
  auditoría independiente de Codex corregidos. Su diseño, las decisiones y lo comprobado que las
  justifican, y cómo se implementó y verificó, están en
  [etapa-3-copias-locales.md](etapa-3-copias-locales.md), como contexto histórico.
- **Fuera de alcance, para después**: el bundle de la ventana pasó de unos 497 a 517 kB y Vite avisa
  de que supera 500 kB; es un aviso, en una app que carga desde disco, y no se cambió la configuración
  por él. Una aplicación que ya no declara ninguna copia local no ofrece «Show every reading» en la
  ventana; sus lecturas siguen guardadas y la puerta las da (`history`).
- **La primera carga, medida en las dos caminatas de #5.** La caminata dice cuánto tarda la ventana
  en poder manejarse tras arrancar DevHQ y, en el primer paso (y en cualquiera que falle), si la
  página está visible, cuánto lleva cargada y cuándo se pidió y se respondió cada llamada a DevHQ, en
  ms desde que cargó la página:
  - WebView2 («Installer, native walk…», bajo la protección en tiempo real): la ventana se pudo
    manejar 5077 ms después de arrancar DevHQ; visible, con foco y cargada hacía 1413 ms;
    **`list_inventory` 1231 → 1255 ms (24 ms)**, con la actualización desde v6 y el paso a WAL
    incluidos, y `watch_http_monitor` 1230 → 1259 ms. El paso tardó 726 ms. La puerta dejó de ver la
    ventana 319 ms después de que DevHQ terminara. En #4 fueron 4760 ms, 686 ms, `list_inventory`
    532 → 560 ms (28 ms), 407 ms y 322 ms: la llamada sigue tardando lo mismo.
  - WebKitGTK: la ventana se pudo manejar 3036 ms después de arrancar DevHQ; visible, con foco y
    cargada hacía 394 ms; el paso tardó 1373 ms (en #4, 2426, 516 y 1679 ms). Ahí no hay tiempos de
    llamadas: Tauri llama a DevHQ por `ipc://localhost/<comando>`, que no es HTTP, y la página solo
    mide peticiones HTTP(S) (Resource Timing); en Windows llama por `http://ipc.localhost/<comando>`.
    Desde #5 la caminata pregunta a Tauri adónde llama (`convertFileSrc('', 'ipc')`) y lo dice en vez
    de «none recorded»: «calls into DevHQ: not timed here, since Tauri sends them to
    ipc://localhost/, which is not HTTP(S), and the page times only HTTP(S) requests».
- **Un fallo intermitente sin explicar**, visto una vez en el primer intento del job «Installer,
  native walk…» de #3: el primer paso esperó 15 s a «Nothing is registered yet.» y la captura mostraba
  «Loading local inventory...», así que `list_inventory` no había respondido; pasaron 74 s entre el
  arranque de msedgedriver y el fallo, y al relanzarlo el paso pasó en 901 ms. En #4 y en #5 no
  volvió. Si vuelve, leer su «window state»:
  - Si `list_inventory` falta mientras otras llamadas sí aparecen, o se respondió tarde, lo retiene
    Rust: buscar qué (las tres conexiones de la primera carga, la conversión a WAL, el antivirus);
    hacer `list_inventory` asíncrono, como ya son los comandos que esperan, al menos dejaría la
    ventana atendida mientras tanto.
  - Si se respondió pronto (unos 30 ms, como arriba) y la página no lo mostró, el retraso está en la
    página o en el driver: `visibility hidden` apuntaría a que WebView2 no pinta una ventana que nadie
    ve.

## Qué hacer, en orden

1. **Decidir la próxima prioridad**: evaluar DevHQ tal como está, con la etapa 3 ya en `main`,
   y contrastarlo con [docs/vision.md](vision.md) y sus preguntas antes de elegir. Ninguna etapa ni
   funcionalidad nueva está elegida todavía.
2. **Una propuesta, no decidida: abrir una referencia, si la persona lo quiere**, solo como
   subproducto: un comando Rust `open_reference(id)` que lee la referencia de SQLite y abre en el
   navegador solo URL http(s); la ventana nunca pasa una URL y el WebView no gana plugin ni permiso.
   Una ruta no se abre hasta que DevHQ sepa en qué estación corre, y entonces solo se muestra la
   carpeta en el explorador, nunca se abre un archivo: abrir una ruta que escribió un agente podría
   ejecutar un programa.

## Lo que solo se puede hacer en la laptop

- Lo pendiente de la puerta en [docs/puerta-para-agentes.md](puerta-para-agentes.md): un Claude Code
  real contra el `devhq.exe` instalado y una puerta lanzada desde la app de escritorio de Claude
  (MSIX). La persona prefiere no instalar DevHQ hasta que sea usable.
- `npm run e2e:native` en Windows, que necesita apartar antes los datos de `%APPDATA%` y
  `%LOCALAPPDATA%`.

Una sesión en la nube corre todo lo demás, incluido `npm run e2e:native` sobre WebKitGTK en Linux
(ver «Cloud sessions» en `CLAUDE.md`), y el workflow de Windows prueba lo suyo en cada PR.
