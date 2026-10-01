# DevHQ

Centro de operaciones **nativo de escritorio** para un ecosistema personal de software.
Su prioridad es responder **«¿Qué necesita mi atención?»**: observar aplicaciones,
recordar eventos, relacionar infraestructura y verificar resultados de automatizaciones.

> Si una función solo ahorra escribir un comando sencillo, probablemente no pertenece
> a DevHQ. Si observa, recuerda, relaciona, detecta problemas o automatiza un proceso
> completo, sí puede pertenecer.

DevHQ no reemplaza VS Code, una terminal, GitHub Desktop, Docker Desktop, Codex ni
Claude Code. Conserva inventario local, la evidencia que registras a mano y la de
comprobaciones HTTP(S) que lanzas tú o que supervisa **solo mientras su ventana está
abierta**. **No funciona en segundo plano ni opera sistemas.**

## Qué incluye

- Overview centrado en atención, incidencias, actividad y contexto operativo.
- Applications con búsqueda, filtros, identidad visual y detalle con seis tabs. En la
  demo solo Overview tiene contenido completo; en Mi inventario también Health (historial
  de observaciones), Infrastructure (cada recurso en que corre, con su evidencia, su
  historial, su comprobación, sus incidencias y las demás aplicaciones que lo comparten) y
  Activity. Las restantes explican su futura función.
- Infrastructure compartida con enlaces a las aplicaciones que dependen de ella,
  incidencias del recurso y enlaces desde los componentes de cada aplicación. En Mi
  inventario, cada recurso observado despliega su historial de observaciones completo, con
  los mismos avisos de ventana y la misma carga bajo demanda que el Health tab de una app.
- Incidents con severidad, estado, contexto desplegable y filtros; Activity global.
- En Mi inventario: observaciones, incidencias y eventos manuales persistentes, con
  procedencia, hora y vigencia visibles.
- Una comprobación HTTP(S) de solo lectura por aplicación o recurso, que se ejecuta a
  petición o, si activas su supervisión, cada 5–60 min mientras DevHQ está abierto, y
  registra una observación automática con su evidencia técnica.
- Un panel de Supervision en Overview: qué comprueba DevHQ por su cuenta, cuándo toca la
  siguiente ejecución, qué falló dentro de DevHQ y qué no está supervisado.
- Previews de Automations (Trigger → Condition → Action → Verification) y Security
  informativos. En Mi inventario, Settings muestra la ruta del archivo SQLite y cómo
  respaldarlo y restaurarlo.

El modo **Demo** utiliza un escenario local fijo del **24/09/2026, 16:40 Europe/Madrid**.
Los tiempos relativos se calculan frente a ese instante, no frente al reloj real.
Los estados unknown/no observado no implican salud ni seguridad. Lifecycle y health
son independientes. Una incidencia de un recurso también aparece en las aplicaciones
que lo utilizan; su actividad también aparece en el contexto de esas aplicaciones.
La copia de ejemplo ocurre a las 08:40 y se verifica a las 08:45; los resultados
de automatización son parte del escenario, no ejecuciones de DevHQ. Los logos y
thumbnails pueden sustituirse mediante campos del modelo. En Tauri se abre por defecto
**Mi inventario**, que usa SQLite; la demo se elige explícitamente en la barra lateral.
La preview web solo muestra la demo porque no dispone de IPC nativo; para desarrollo,
`preview.html` simula ese IPC con datos de ejemplo (ver [Ejecutar](#ejecutar)). Los mocks nunca
se importan a SQLite. En el inventario real, aplicaciones, componentes y recursos
aparecen como **no observados**; lifecycle no expresa health.

Desde Mi inventario se crean y editan recursos y aplicaciones. Cada aplicación
contiene componentes que pueden apuntar a un recurso existente. Al guardar una
aplicación se reemplaza su lista de componentes de forma atómica: quitar un
componente también quita su vínculo. Desde su formulario de edición, y tras confirmarlo,
se puede borrar una aplicación o un recurso **sin historial**: con sus componentes y su
comprobación HTTP. Lo que tiene observaciones, incidencias o eventos se conserva (una
aplicación puede pasar a Archived), y un recurso en el que corre algún componente también,
hasta desenlazarlo; la UI explica el motivo y Rust lo vuelve a comprobar, dentro de la
misma transacción, antes de borrar nada. Un recurso inexistente se rechaza antes
de escribir la aplicación, igual que un componente repetido o que pertenece a otra
aplicación. Rust guarda los textos sin espacios alrededor y rechaza (nunca recorta) los que
superan sus límites, los mismos que impone la UI y contados igual (unidades UTF-16): nombres
120, títulos 160, resúmenes y notas 500, descripciones y detalles 1000, versión 60; una
versión en blanco es ninguna. El texto ya guardado se acepta sin cambios aunque supere un
límite posterior, así que siempre se puede volver a guardar (o archivar) lo existente. Un fallo de SQLite/IPC muestra un error y permite reintentar.

### Observaciones, incidencias y eventos

Todo registro real apunta por ID a **una** aplicación o **un** recurso y declara su
origen (`source`), siempre visible:

| `source` | Etiqueta               | Quién lo produce                                                |
| -------- | ---------------------- | --------------------------------------------------------------- |
| `manual` | Manual entry           | Tú. Incidencias y eventos registrados a mano                    |
| `http`   | Automated · HTTP check | DevHQ, al ejecutar la comprobación que tú lanzaste o supervisas |

Solo las observaciones (y el evento que las acompaña) pueden ser `http`; el esquema lo
impone con CHECK. Una observación automática sigue exactamente la misma semántica de
vigencia, sustitución y atención que una manual.

- **Observación**: resultado (healthy/degraded/unhealthy), qué se vio, momento observado,
  momento registrado y vigencia (1 h–30 días). Es inmutable; se corrige registrando otra.
- **Incidencia**: título, severidad y estado open → investigating → resolved (reabrible).
  Cada cambio guarda una nota opcional en su historial.
- **Evento**: la línea temporal. Registrar una observación o cambiar una incidencia escribe
  su evento en la misma transacción; despliegues, backups, mantenimiento y notas se
  registran a mano. Si se anota mucho después de ocurrir, se muestran ambos momentos.

Estados de evidencia de un sujeto, a partir de su observación **más reciente** (sustituye
a las anteriores aunque sigan vigentes):

| Estado                                   | Cuándo                           | Presentación                       |
| ---------------------------------------- | -------------------------------- | ---------------------------------- |
| Not observed                             | Nunca hubo observación           | Círculo discontinuo; no es salud   |
| Observation expired                      | La más reciente pasó su vigencia | Reloj; muestra el último resultado |
| Healthy / Attention required / Unhealthy | La más reciente sigue vigente    | Punto de color                     |

**Needs attention** ordena: incidencias críticas, fallos observados, avisos, resultados
degradados, problemas caducados, incidencias informativas y resultados sanos caducados.
Lo nunca observado no entra en esa lista, pero Overview lo cuenta como hueco de cobertura:
una lista vacía solo tranquiliza si todo tiene evidencia vigente. Una incidencia o evento
de un recurso aparece también en las aplicaciones que lo usan. Lifecycle sigue siendo
independiente de health. La vigencia se reevalúa al cargar y cada 30 s con la ventana
abierta; ese reloj no lee datos ni ejecuta comprobaciones.

### Comprobación HTTP(S)

Cada aplicación o recurso admite **como máximo un** endpoint: URL http/https, estado
esperado (200–599, nunca 3xx porque los redirects se siguen), tiempo máximo (1–10 s),
vigencia del resultado (1 min–30 días) y, opcionalmente, un intervalo de supervisión.
**Run check** lo ejecuta una vez; no hay reintentos, credenciales ni ejecución con la
app cerrada.

- La petición sale del proceso Rust (reqwest con el TLS y el almacén de certificados del
  sistema), nunca del WebView: no se abre la CSP ni se expone un plugin HTTP. Es un `GET`
  directo, sin proxy, cookies, cabeceras propias ni cuerpo. La verificación TLS nunca se
  desactiva. Un único plazo cubre DNS, conexión, TLS y redirects, así que no puede quedar
  colgada; el comando es asíncrono y la ventana sigue utilizable.
- Se siguen hasta 5 redirects a mano y cada salto (estado y destino) queda registrado.
- Evidencia guardada: URL solicitada, estado esperado, último estado recibido, latencia
  (hasta la respuesta o el fallo), error y redirects. **Nunca** cuerpo ni cabeceras.
  Healthy exactamente cuando llega el estado esperado sin error; si no, Unhealthy.
- **Fallo del sistema comprobado** (DNS, conexión rechazada, TLS, tiempo agotado, estado
  inesperado, demasiados redirects) = evidencia: observación Unhealthy con su detalle.
- **Fallo de DevHQ** (URL o configuración inválida o ausente, SQLite, IPC) = error visible
  y recuperable. Nunca produce observación: la observación y su evento se escriben en una
  sola transacción, y solo entonces responde el comando.
- La configuración es contexto, no historial. Guardarla reemplaza la anterior del mismo
  sujeto; su sujeto no cambia (para moverla, quítala y créala en el otro). Cada
  observación copia la URL y el estado esperado, así que editar o quitar la configuración
  nunca reescribe evidencia; si la URL cambió desde la última ejecución, la UI lo avisa.
  Al borrar un sujeto sin historial, su configuración se va con él (`CASCADE`) y la
  supervisión lo olvida al instante; una ejecución en vuelo ya no puede registrar nada.
  Su evidencia, si la hubiera, impediría el borrado (`RESTRICT`).
- La URL se guarda en texto plano: no pongas secretos en ella. Se rechazan URLs con
  usuario o contraseña.

### Supervisión mientras DevHQ está abierto

Una comprobación puede estar **off** (solo a petición), **active** (además, cada 5–60 min)
o **paused** (conserva su intervalo pero no arranca sola). Activarla siempre es una
decisión explícita: la migración v5 deja todas las comprobaciones existentes en off.
Supervisar dice que DevHQ mira, nunca qué encontró; eso lo decide la evidencia de cada
ejecución, que es idéntica a la de Run check y sigue la misma semántica de vigencia.

- La vigencia del resultado debe superar el intervalo (lo validan Rust y un CHECK), así que
  la evidencia supervisada no caduca entre ejecuciones.
- Un único bucle en el runtime asíncrono de Tauri planifica todas las comprobaciones; vive
  lo que vive el proceso. Cerrar la ventana cancela las ejecuciones en vuelo, que no
  registran nada, y termina el bucle. No hay servicio, tarea del sistema ni arranque solo.
- La siguiente ejecución toca **un intervalo después de que terminó la anterior**, la
  lanzara quien la lanzara: un Run check también reinicia la espera. Las ejecuciones
  perdidas no se recuperan: tras una pausa, la app cerrada o una ejecución lenta, una
  comprobación atrasada corre **una** vez.
- Al abrir DevHQ, el ritmo continúa desde la última observación `http` guardada. Las
  atrasadas arrancan escalonadas (10 s tras el arranque y 10 s entre ellas): primero las
  que nunca corrieron y después las que más llevan esperando.
- Una comprobación nunca corre dos veces a la vez: pedir Run check durante una ejecución
  en vuelo se une a ella y recibe la misma observación.
- Sin reintentos: tras cualquier fallo, del sistema comprobado o de DevHQ, se espera un
  intervalo completo. Un fallo de DevHQ no registra nada, queda visible como último error
  hasta la siguiente ejecución correcta y nunca detiene las demás comprobaciones.
- Pausar deja terminar (y registrar) la ejecución en vuelo, acotada por su tiempo máximo;
  reanudar ejecuta una vez si está atrasada. Editar una comprobación pausada no la reanuda.
  Quitarla cancela su ejecución en vuelo: la escritura comprueba en la misma transacción
  que la configuración sigue existiendo, así que un resultado tardío nunca se registra.
- El bucle vuelve a planificar al menos cada 30 s, para notar un cambio de reloj o la
  vuelta de una suspensión sin fiarse de un temporizador largo.
- Cada lectura del inventario lleva **todos** los registros manuales, pero solo los últimos
  200 resultados automáticos de cada aplicación o recurso (con sus eventos): la supervisión
  puede añadir uno cada 5 min durante meses. Los anteriores siguen en SQLite, intactos, y la
  UI lo dice: una nota indica desde cuándo los resultados automáticos están completos y una
  línea marca dónde una lista pasa a tener solo registros manuales. En el historial de
  observaciones de un sistema, los anteriores se cargan bajo demanda, de 200 en 200, anclados
  al más antiguo ya mostrado para que los resultados nuevos no causen saltos ni duplicados. Una vista que mezcla
  sistemas (Activity) solo muestra resultados automáticos desde el momento en que los de
  todos están completos, para no sugerir que uno no tuvo resultados. Activity y la Activity
  de una aplicación también cargan los anteriores bajo demanda: Rust lee, por cada sistema
  recortado y siguiendo su índice, los que preceden al evento del corte, los mezcla y
  devuelve los 200 siguientes en el orden de la línea de tiempo. Así la vista queda completa
  hasta el más antiguo de la página sin saltarse ningún sistema, y lo que ya mostraba se
  conserva aunque un snapshot más nuevo lo deje fuera de su ventana. En las líneas de
  tiempo, tres o más resultados automáticos seguidos de un mismo sistema que dijeron lo mismo
  (el mismo estado o el mismo fallo, sin importar la latencia) se pliegan en una fila
  desplegable, marcada «N+» si llega al punto donde la vista deja de estar completa; cualquier otro registro entre medias corta el pliegue,
  así que el orden se conserva y lo que cambió no queda enterrado. Cada consulta sigue el
  índice de cada sistema, así que su coste depende de lo que se carga y no del historial
  acumulado. Con 5 comprobaciones cada 5 min durante 90 días (129 600 resultados, 91 MB en
  disco), antes de la ventana una recarga movía 86 MB en 0.5 s; ahora tarda 8 ms, manda
  855 KB a la ventana y lee 0.7 MB del archivo (0.65 MB con 10 días). Una página de Activity
  global tarda 2 ms, manda 79 KB y lee 0.4 MB, igual al principio, a la mitad o al final del
  historial. Los eventos de la ventana se buscan por el índice de su sistema; cuando se
  buscaban por el ID aleatorio de su observación, cada búsqueda caía en una página distinta de
  un índice tan grande como el historial y la recarga leía 3.9 MB a los 90 días. Las cifras
  son de Linux, en release y con la caché del sistema caliente (`history_costs`, ver
  [Ejecutar](#ejecutar)); en otros sistemas esa prueba no mide lo leído del archivo.
- El proceso nativo empuja el estado a la ventana por un `Channel` de Tauri: ejecución en
  curso, última y siguiente ejecución y último error de DevHQ. Cuando una ejecución
  registra evidencia, la ventana relee el inventario sin estado de carga, así que los
  formularios abiertos y los registros desplegados se conservan.

## Ejecutar

Requisitos: Node.js 24 LTS con npm 11 (el que trae), Rust estable y los
[prerrequisitos de Tauri 2](https://v2.tauri.app/start/prerequisites/).
En Windows: Visual Studio Build Tools con C++ y Windows SDK, además de WebView2. Las
comprobaciones HTTP usan el TLS del sistema: SChannel en Windows, Security.framework en
macOS y OpenSSL (con sus cabeceras de desarrollo) en Linux. Las pruebas de red usan un
servidor en `127.0.0.1` y el dominio reservado `.invalid`; no contactan servidores externos.

```sh
npm ci
npm run tauri dev          # escritorio nativo
npm run dev               # preview web: http://127.0.0.1:1420
                          # Mi inventario simulado: http://127.0.0.1:1420/preview.html
npm run check             # ESLint, tests, TypeScript, Vite y cargo check
npm run e2e               # Playwright sobre preview.html (Chrome del sistema)
cargo test --manifest-path src-tauri/Cargo.toml --lib # persistencia y migraciones
cargo test --release --manifest-path src-tauri/Cargo.toml --lib history_costs -- --ignored --nocapture
                          # mide recarga y Activity con 90 días de supervisión
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
npm run tauri build       # ejecutable e instalador NSIS para Windows
```

`preview.html` existe solo en desarrollo: carga la ventana real sobre un proceso nativo
simulado con `@tauri-apps/api/mocks` y datos de ejemplo relativos al reloj, con un recurso
supervisado que tiene más resultados de los que lleva un snapshot. Los comandos de lectura
siguen las mismas reglas de ventana y paginación que Rust (lo prueba
`src/preview/backend.test.ts`), toda escritura se rechaza sin guardar nada y un aviso fijo en
pantalla dice que son datos de ejemplo. `?scenario=empty` y `?scenario=unavailable` muestran
un inventario vacío o ilegible. El bundle de producción se construye desde `index.html` y no
contiene nada de esto. `npm run e2e` usa el Chrome instalado (en CI, el del runner) o el
ejecutable que indique `DEVHQ_CHROMIUM`; nunca descarga navegadores, y reutiliza un `npm run dev`
en marcha. Además de recorrer el historial, audita con axe-core (reglas WCAG A/AA y buenas
prácticas) cada página real de la preview, algunos formularios abiertos y la demo: landmarks,
orden de encabezados, nombres accesibles y contraste. Solo se excluye el rótulo decorativo de
las miniaturas de la demo, oculto a tecnologías de asistencia y exento de contraste.

Si DevHQ no termina tras cerrar su ventana, antes de matar el proceso ejecuta
`powershell -NoProfile -File scripts/dump-stacks.ps1 -ProcessId <pid>`: vuelca la pila de
cada hilo con `dbghelp.dll` de Windows y el PDB de release, sin instalar nada.

El instalador se genera en `src-tauri/target/release/bundle/nsis/`. El target de
empaquetado está configurado para Windows; otros sistemas necesitan sus prerrequisitos
y seleccionar un target de bundle apropiado. Los lockfiles de npm y Cargo se versionan. El
de npm lo escribe npm 11: npm 10 borra los campos `libc` de los paquetes de cada plataforma
cada vez que lo reescribe. En las sesiones de Claude Code en la web,
`.claude/hooks/session-start.sh` prepara el contenedor (WebKitGTK, npm 11, dependencias,
crates y `DEVHQ_CHROMIUM`) y `CLAUDE.md` resume las reglas de trabajo del repositorio.

## Arquitectura

```text
src/
  app/             Shell, rutas y navegación
  features/        Vistas y componentes por área del producto
  components/ui/   Primitivas shadcn/ui basadas en Radix
  components/      Presentación compartida: estados, logos, enlaces, encabezados
  domain/          Modelos, vocabulario y selectores puros
  data/mock/       Escenario tipado y relaciones por ID
  shared/          Utilidades de estilos y tiempo
  test/            Configuración de pruebas
src-tauri/         Host Rust/Tauri 2, SQLite, migraciones, IPC, ventana y CSP
```

React + TypeScript estricto + Vite + Tailwind CSS 4. El estado de filtros es local;
las rutas y tabs usan React Router con hash, compatible con el protocolo empaquetado
de Tauri. No hay servidor HTTP de aplicación, store global ni capa de repositorios
artificial. Los selectores de dominio no dependen de React ni de los mocks. Las vistas
consumen explícitamente el escenario durante esta etapa.

Rust expone `list_inventory` (una lectura consistente de inventario, registros y
`httpChecks`), `data_location`, `save_application`, `save_resource`, `delete_application`,
`delete_resource`, `record_observation`, `older_observations`, `older_events`, `open_incident`,
`update_incident_status`, `record_event`, `save_http_check`, `set_http_check_paused`,
`remove_http_check`, `run_http_check` y `watch_http_monitor`. Los comandos de
comprobaciones y de borrado son asíncronos y avisan a la supervisión antes de responder, así
que pausar, reanudar, quitar o borrar surten efecto de inmediato; `run_http_check` pasa por la supervisión para
no duplicar una ejecución en vuelo y `watch_http_monitor` abre el `Channel` de estado. Abre
`inventory.sqlite3` en el directorio de datos de la aplicación que proporciona Tauri; las
migraciones transaccionales v1→v6 usan `PRAGMA user_version` y una versión más nueva se
rechaza en vez de adivinarse. Cada apertura solo lee esa versión: únicamente una base antigua
toma el lock de escritura para migrar, y dentro vuelve a leerla por si otra conexión ya lo
hizo. La base usa write-ahead logging (WAL): un lector trabaja sobre el último estado
confirmado sin esperar a nadie y un escritor no espera a los lectores; solo los escritores se
turnan. Importa porque la supervisión escribe desde el runtime asíncrono mientras la ventana
lee, y los comandos síncronos corren en el hilo principal: con el journal de rollback, una
lectura podía congelar la ventana detrás de un commit en curso. El modo queda guardado en el
archivo y `synchronous` sigue en FULL, así que cada evidencia llega al disco antes de que el
comando responda. Al cerrarse la última conexión SQLite vuelca el log en `inventory.sqlite3`,
que queda completo por sí solo. Una base anterior se convierte en la primera apertura que la
tiene para sí; si otra conexión la está usando en ese momento (SQLite no espera para esto),
sigue con su journal de rollback, igual de correcta, hasta la siguiente. Settings muestra la
ruta exacta del archivo (`data_location` no abre la base, así que responde aunque no se pueda
leer): para respaldarlo basta copiarlo con DevHQ cerrado; para restaurar una copia, con DevHQ
cerrado se reemplaza el archivo y se borran `-wal` y `-shm` si quedaron, para que ningún log del
archivo anterior se aplique a la copia. La v4 reconstruye `observations` y `events` (SQLite no cambia
un CHECK con ALTER) conservando filas, IDs, `rowid` (desempate de orden), claves foráneas,
índices y los vínculos evento → observación. La v5 solo reconstruye `http_checks` (nada la
referencia) para añadir el estado y el intervalo de supervisión con sus CHECK, conservando
el `rowid` que ordena la lista. Ambas se ejecutan con las claves foráneas desactivadas y
solo se confirman si `PRAGMA foreign_key_check` no encuentra relaciones rotas. La v6 solo
añade dos índices parciales sobre los registros manuales; ninguna fila cambia. Los
registros usan dos columnas con clave foránea (`application_id`/`resource_id`) y un CHECK
que exige exactamente una; las referencias de historial son `ON DELETE RESTRICT`, así que
nada puede borrar un sujeto y dejar evidencia huérfana; los casos de uso de borrado lo
comprueban antes y explican qué lo impide. Las referencias inexistentes se rechazan antes de escribir y dentro de la misma
transacción. Los tiempos se guardan en milisegundos UTC y viajan como RFC 3339; no se
aceptan momentos futuros. La UI invoca esos casos de uso con contratos tipados. Se conserva la barra nativa de ventana. La CSP limita recursos y las
capabilities no conceden permisos adicionales. No se implementan autenticación,
cuentas, cloud, sync, telemetría, scanners, automation engine, Runner ni background service.

## Evolución prevista

1. Hecho: una comprobación HTTP(S) a petición con su propio `source`, sin cambiar la
   semántica de vigencia.
2. Hecho: supervisión cancelable de esas comprobaciones, solo con DevHQ abierto.
3. Después, automatizaciones con resultado e historial, y seguridad con almacén nativo de
   credenciales y evidencias contextualizadas.

Estos son límites conceptuales, **no módulos vacíos ni integraciones implementadas**.
La dirección es UI → IPC/casos de uso → dominio. SQLite conserva contexto; las credenciales no deben
vivir en componentes React, localStorage ni texto plano en SQLite.

Las pruebas cubren navegación, tabs/teclado, filtros, rutas desconocidas, relaciones
compartidas, cronología de la demo, salud desconocida, persistencia tras reapertura,
rechazo de referencias inválidas sin escrituras parciales, restricciones del esquema,
migraciones v1/v2→v3, v3→v4, v4→v5 y v5→v6 (incluida la negativa a confirmar relaciones rotas),
vigencia y sustitución de observaciones de ambos orígenes, orden de atención, errores de
carga visibles y recuperables, comprobaciones contra un servidor local (estado esperado,
500, redirect, bucle de redirects, tiempo agotado, conexión rechazada, dominio `.invalid`
y fallo TLS), configuraciones inválidas sin escrituras parciales, errores de DevHQ que
no producen observación, la distinción de origen en cada vista real y el borrado (qué lo
impide, la confirmación y que un resultado tardío de un sujeto borrado no se registra). La supervisión se
prueba sobre un reloj simulado de Tokio, sin esperas reales: ritmo por intervalo,
continuidad tras reabrir, una sola ejecución por atraso, sin solapes, Run check que se une
a la ejecución en vuelo, pausa, cancelación al quitar o cerrar, fallos de DevHQ visibles
que no detienen al resto y configuración ilegible que se reintenta; y, con peticiones
reales a un servidor local, que la evidencia programada es idéntica a la manual. La ventana
de historial se prueba por su límite exacto, los eventos que acompañan a cada resultado, el
aviso de lo omitido en cada vista, la paginación de observaciones y de eventos de varios
sistemas a la vez (sin huecos ni duplicados, con empates al mismo milisegundo y resultados
nuevos entre páginas) y los planes de consulta, que nunca recorren el historial automático
completo ni buscan un evento automático por su observación, tampoco con estadísticas que otra
herramienta haya calculado sobre el archivo. La concurrencia de SQLite se prueba sin esperas medidas: una lectura no espera a una
escritura en curso, una escritura no espera a una lectura abierta (que conserva su estado), dos
primeras aperturas simultáneas migran una sola vez, una base antigua ocupada se abre sin
convertirse y se convierte en la siguiente apertura, y al cerrar queda un único archivo
completo.
