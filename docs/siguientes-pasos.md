# Siguientes pasos

Nota de trabajo, no especificación: dice qué está en marcha y qué toca después para acercar DevHQ a
[docs/vision.md](vision.md), de modo que una sesión nueva (de Claude Code, de Codex o de quien sea)
pueda seguir sin la conversación que la escribió. El [README](../README.md) sigue mandando. Cuando un
paso se termina, sale de aquí en el mismo commit; cuando no quede ninguno, esta nota se borra.

Escrita el 6 oct 2026 desde una sesión local en la laptop con Windows, y puesta al día esa noche, al
fusionar el registro de cambios y proponer el diseño de las referencias.

## El plan, en corto

Revisado `main` contra la visión, el siguiente paso estructural es anclar cada sistema a lo que es en
el mundo real. La visión, la puerta para agentes (esquema v9) y el registro de cambios común (v10) ya
están en `main`: el inventario tiene autor y tiempo, como la evidencia. Ahora, en orden:

1. **Referencias** (diseño propuesto, ver abajo): anclar cada sistema a lo que es en el mundo real
   (repositorio, copia local en una estación de trabajo, documentación, logs, panel).
2. **Primera fuente observada sobre esas anclas**: la actividad de los repositorios locales, leída
   cuando se pide.

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
    cargada hacía 516 ms; el paso tardó 1679 ms. **Las llamadas salen como «none recorded»**, y no es
    un fallo de la caminata: en Linux, Tauri 2.11 llama a DevHQ por `ipc://localhost/<comando>`, y los
    navegadores solo registran los tiempos de peticiones HTTP(S) (Resource Timing); en Windows llama
    por `http://ipc.localhost/<comando>`. El diagnóstico sirve en el WebView donde salió el fallo, y
    en WebKitGTK no puede servir con esa técnica.
- **Pendiente pequeño**: que la caminata diga en WebKitGTK por qué no hay tiempos, en vez de «none
  recorded», o que los mida igual en los dos WebView con `performance.mark` en `inventoryApi`.
  Tocar `e2e/native/` relanza los jobs de Windows, así que va con el próximo cambio que ya los
  relance (la etapa 2 toca `src-tauri/`), no en un push propio.
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

1. **Etapa 2, referencias: diseño propuesto, esperando el visto bueno de la persona.** No se escribe
   código hasta que lo dé. Responde «¿Dónde están sus logs, su documentación, su repositorio?» (hoy,
   solo la descripción libre) y, para un agente que entra en una carpeta, «¿qué sistema es este?».
   Es inventario, no evidencia: cambia poco y lo puede declarar un agente que ya está en el proyecto.
   - **Tabla** `system_references` (`references` es palabra reservada de SQL): `id`, el sujeto con
     las dos columnas de siempre (`application_id`/`resource_id`, CHECK de exactamente una,
     `ON DELETE CASCADE` como `http_checks`), `kind`, `target`, `label` opcional (120) y
     `machine_id` → `resources(id) ON DELETE RESTRICT`. Índices por sujeto y por máquina. La v11 solo
     crea la tabla; ninguna fila cambia; prueba de actualización fila por fila.
   - **Tipos**, conceptos y no proveedores: `repository`, `local_copy`, `documentation`, `logs` y
     `dashboard`.
   - **Destino**: una URL http(s) sin usuario ni contraseña (2048, como la comprobación) o una ruta
     absoluta (`C:\…`, `\\servidor\…` o `/…`, aceptadas en cualquier sistema, porque la ruta puede ser
     de otra máquina). Una regla para todas: **toda ruta dice en qué máquina está** (`machine_id`), y
     una URL no. `repository` y `dashboard`, solo URL; `local_copy`, solo ruta en un recurso
     `workstation` (el mismo proyecto en la laptop y en el sobremesa son dos); `documentation` y
     `logs`, cualquiera de las dos (un log en `/var/log/…` de un servidor). La misma referencia (tipo,
     destino y máquina) dos veces en un sujeto se rechaza.
   - **Borrar la máquina de una ruta se rechaza** y dice de quién es la ruta, como un recurso en el que
     corre un componente: con `CASCADE` cambiaría la configuración de otra aplicación sin su entrada
     en el registro de cambios.
   - **Monorepo**: la misma URL de repositorio desde varias aplicaciones, sin entidad nueva; la
     ventana y la puerta dicen «same repository as …», derivado al leer por igualdad exacta.
   - **Se editan como los componentes**, dentro de `save_application` y `save_resource`: la lista
     enviada es la completa, conserva los ID y se reemplaza en la misma transacción. Así el registro de
     cambios las cubre con un campo más, `references`, en las acciones que ya existen (`created`,
     `updated`, `deleted`): **no hacen falta acciones nuevas**, que obligarían a reconstruir
     `change_log` para cambiar el CHECK de `action`.
   - **Ventana**: los formularios de aplicación y de recurso ganan la lista; el Overview de la
     aplicación y la tarjeta del recurso en Infrastructure las muestran con su máquina y un botón para
     copiar el destino, como la ruta en Settings. Sin referencias dice «No references yet», sin
     presentarlo como problema: no es atención ni cobertura.
   - **Abrir**, solo como subproducto: la v11 solo copia. Después, si la persona lo quiere, un comando
     Rust `open_reference(id)` que lee la referencia de SQLite y abre en el navegador solo URL
     http(s): la ventana nunca pasa una URL y el WebView no gana plugin ni permiso. Una ruta no se abre
     hasta que DevHQ sepa en qué estación corre (etapa 3), y entonces solo se muestra la carpeta en el
     explorador, nunca se abre un archivo: abrir una ruta que escribió un agente podría ejecutar un
     programa.
   - **Puerta**: `get_system` y `list_systems` devuelven las referencias de cada sistema (en
     `list_systems`, para que un agente encuentre su sistema por su carpeta o su remoto);
     `save_application` y `save_resource` aceptan `references`, lista completa cuando se manda y
     conservada cuando no, con la máquina por ID o por nombre exacto, como el recurso de un componente.
   - **Contratos y pruebas**: fixtures de `list_inventory`, `save_application` y `save_resource`; las
     herramientas de la puerta; datos de ejemplo en la preview; validación sin escrituras parciales;
     el rechazo al borrar la máquina; y el registro de cambios con el campo nuevo.
   - **Fuera**: comprobar que la ruta existe o que la URL responde (sería evidencia), descubrir
     repositorios, credenciales y tipos con nombre de proveedor.
   - **Decisiones de la persona**: (a) editar dentro del formulario del sistema (propuesto) o con
     comandos propios; (b) la v11 solo copia (propuesto) o trae ya `open_reference` para URL;
     (c) `repository` solo como URL web http(s) (propuesto; los remotos `git@host:…` llevan usuario y
     la etapa 3 los leerá de la copia local) o también ssh; (d) los logs de una unidad de systemd o de
     un contenedor, por ahora en la etiqueta (propuesto) o con un tercer tipo de destino.
2. **Etapa 3, actividad local**, después de las referencias. Dos decisiones que tomar con la persona
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
