# Siguientes pasos

Nota de trabajo, no especificación: dice qué está en marcha y qué toca después para acercar DevHQ a
[docs/vision.md](vision.md), de modo que una sesión nueva (de Claude Code, de Codex o de quien sea)
pueda seguir sin la conversación que la escribió. El [README](../README.md) sigue mandando. Cuando un
paso se termina, sale de aquí en el mismo commit; cuando no quede ninguno, esta nota se borra.

Escrita el 6 oct 2026 desde una sesión local en la laptop con Windows, puesta al día esa noche, al
fusionar el registro de cambios y proponer el diseño de las referencias, y el 7 oct, al implementar
las referencias con las decisiones que la persona aprobó.

## El plan, en corto

Revisado `main` contra la visión, el siguiente paso estructural era anclar cada sistema a lo que es
en el mundo real. La visión, la puerta para agentes (esquema v9) y el registro de cambios común (v10)
están en `main`, y las referencias (v11), en `claude/references-design`: cada aplicación y recurso
dice dónde está su repositorio, sus copias locales, su documentación, sus logs y su panel. Ahora:

1. **Primera fuente observada sobre esas anclas** (etapa 3): la actividad de los repositorios
   locales, leída cuando se pide.

Después, según lo que pidan preguntas concretas: bases de datos compartidas y dominios como tipos de
recurso, relaciones nuevas, despliegues con versión, commit y destino, credenciales, operaciones y
supervisión con la ventana cerrada, en el orden de «Líneas de evolución posibles» de la visión.

No conviene hacer todavía: una tabla genérica de entidades, un motor de grafos, una entidad
«proyecto», una capa de «integraciones» antes de tener una segunda fuente real, event sourcing,
permisos por agente ni descubrir repositorios por su cuenta.

## Estado

- [#2](https://github.com/jpgarcia04/DevHQ/pull/2) (la visión),
  [#3](https://github.com/jpgarcia04/DevHQ/pull/3) (la puerta, v9) y
  [#4](https://github.com/jpgarcia04/DevHQ/pull/4) (el registro de cambios, v10) están **fusionados**
  en `main`. #4 entró (`1a40caf`) con todo el CI de Linux y de Windows en verde al primer intento.
- **Las referencias (etapa 2, esquema v11)** están hechas en `claude/references-design`, con su
  pull request abierto; la fusión la decide la persona. Lo que la persona fijó al aprobar el diseño
  y queda en el README («Referencias»): se editan con su sistema, como los componentes, y el registro
  de cambios las cubre con un campo `references` en las acciones de siempre; la v11 solo copia el
  destino; `repository` es solo una URL web http(s), comparada por igualdad exacta; un destino es una
  URL http(s) o una ruta absoluta, y la etiqueta es un nombre para personas, nunca un localizador.
  Son inventario declarado, no evidencia: no llevan `source`.
- **La primera carga, medida en las dos caminatas de #4.** La caminata dice cuánto tarda la ventana
  en poder manejarse tras arrancar DevHQ y, en el primer paso (y en cualquiera que falle), si la
  página está visible, cuánto lleva cargada y cuándo se pidió y se respondió cada llamada a DevHQ, en
  ms desde que cargó la página:
  - WebView2 («Installer, native walk…», bajo la protección en tiempo real): la ventana se pudo
    manejar 4760 ms después de arrancar DevHQ; visible, con foco y cargada hacía 686 ms;
    **`list_inventory` 532 → 560 ms (28 ms)**, con la actualización desde v6 y el paso a WAL
    incluidos, y `watch_http_monitor` 531 → 563 ms. El paso tardó 407 ms. La puerta dejó de ver la
    ventana 322 ms después de que DevHQ terminara.
  - WebKitGTK: la ventana se pudo manejar 2426 ms después de arrancar DevHQ; visible, con foco y
    cargada hacía 516 ms; el paso tardó 1679 ms. Ahí no hay tiempos de llamadas: Tauri llama a DevHQ
    por `ipc://localhost/<comando>`, que no es HTTP, y la página solo mide peticiones HTTP(S)
    (Resource Timing); en Windows llama por `http://ipc.localhost/<comando>`. Desde la etapa 2 la
    caminata pregunta a Tauri adónde llama (`convertFileSrc('', 'ipc')`) y, en WebKitGTK, dice eso
    en vez de «none recorded».
- **Un fallo intermitente sin explicar**, visto una vez en el primer intento del job «Installer,
  native walk…» de #3: el primer paso esperó 15 s a «Nothing is registered yet.» y la captura mostraba
  «Loading local inventory...», así que `list_inventory` no había respondido; pasaron 74 s entre el
  arranque de msedgedriver y el fallo, y al relanzarlo el paso pasó en 901 ms. En #4 no volvió. Si
  vuelve, leer su «window state»:
  - Si `list_inventory` falta mientras otras llamadas sí aparecen, o se respondió tarde, lo retiene
    Rust: buscar qué (las tres conexiones de la primera carga, la conversión a WAL, el antivirus);
    hacer `list_inventory` asíncrono, como ya son los comandos que esperan, al menos dejaría la
    ventana atendida mientras tanto.
  - Si se respondió pronto (unos 30 ms, como arriba) y la página no lo mostró, el retraso está en la
    página o en el driver: `visibility hidden` apuntaría a que WebView2 no pinta una ventana que nadie
    ve.

## Qué hacer, en orden

1. **Etapa 3, actividad local.** Responde «¿qué lleva meses sin actividad?» y prepara «¿qué tiene
   cambios sin desplegar?», sobre las copias locales que la etapa 2 ya ancla a su máquina. Tres
   decisiones que tomar con la persona antes de escribir código:
   - La actividad (último commit, branch, HEAD, cambios sin confirmar, el remoto real) no es salud,
     así que no cabe en una observación healthy/degraded/unhealthy y necesita su propia forma de
     evidencia, con su `source`, su momento observado y su vigencia. «Commits sin publicar» y «sin
     actividad desde…» serían estado derivado, no guardado.
   - Choca con la regla «no se implementan scanners», que habría que reescribir de forma explícita,
     en el README y en `CLAUDE.md` en el mismo commit: leer a petición una ruta que el inventario ya
     conoce no es escanear ni descubrir.
   - DevHQ no sabe todavía en qué máquina corre, y una copia local solo se puede leer en la suya:
     hace falta decir cuál de las estaciones de trabajo del inventario es «esta» (y cómo se recuerda
     sin guardar nada que cambie de equipo a equipo en el inventario compartido).
     Solo refs locales, sin red ni credenciales. El remoto real de una copia (también SSH) se guarda
     como evidencia, aparte de la referencia `repository`; compararlos exige normalizar URL, que la
     etapa 2 dejó fuera a propósito.
2. **Abrir una referencia, si la persona lo quiere**, solo como subproducto: un comando Rust
   `open_reference(id)` que lee la referencia de SQLite y abre en el navegador solo URL http(s); la
   ventana nunca pasa una URL y el WebView no gana plugin ni permiso. Una ruta no se abre hasta que
   DevHQ sepa en qué estación corre, y entonces solo se muestra la carpeta en el explorador, nunca se
   abre un archivo: abrir una ruta que escribió un agente podría ejecutar un programa.

## Lo que solo se puede hacer en la laptop

- Lo pendiente de la puerta en [docs/puerta-para-agentes.md](puerta-para-agentes.md): un Claude Code
  real contra el `devhq.exe` instalado y una puerta lanzada desde la app de escritorio de Claude
  (MSIX). La persona prefiere no instalar DevHQ hasta que sea usable.
- `npm run e2e:native` en Windows, que necesita apartar antes los datos de `%APPDATA%` y
  `%LOCALAPPDATA%`.

Una sesión en la nube corre todo lo demás, incluido `npm run e2e:native` sobre WebKitGTK en Linux
(ver «Cloud sessions» en `CLAUDE.md`), y el workflow de Windows prueba lo suyo en cada PR.
