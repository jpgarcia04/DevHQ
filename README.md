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
  informativos. En Mi inventario, Settings muestra la ruta del archivo SQLite, cómo
  respaldarlo y restaurarlo, y **Back up now**, que lo copia y verifica con DevHQ abierto.

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
límite posterior, así que siempre se puede volver a guardar (o archivar) lo existente. Un fallo
de SQLite/IPC muestra un error y permite reintentar; mientras carga o si falla, el detalle de una
aplicación, que aún no sabe cuál es, se titula «Application» y conserva el enlace de vuelta.

### Observaciones, incidencias y eventos

Todo registro real apunta por ID a **una** aplicación o **un** recurso y declara su
origen (`source`), siempre visible:

| `source` | Etiqueta               | Quién lo produce                                                |
| -------- | ---------------------- | --------------------------------------------------------------- |
| `manual` | Manual entry           | Tú. Incidencias y eventos registrados a mano                    |
| `http`   | Automated · HTTP check | DevHQ, al ejecutar la comprobación que tú lanzaste o supervisas |
| `agent`  | Agent · _nombre_       | Un agente al que le pediste actuar por la puerta para agentes   |

Solo las observaciones (y el evento que las acompaña) pueden ser `http`; el esquema lo
impone con CHECK. Una observación automática sigue exactamente la misma semántica de
vigencia, sustitución y atención que una manual. Un registro `agent` guarda además, en
`agent`, el nombre que el cliente del agente declaró (por ejemplo `claude-code`); DevHQ no
puede verificarlo y la etiqueta lo dice al pasar el cursor. Se presenta mezclado con lo tuyo en
cada vista, con su propia etiqueta, y sigue la misma semántica que un registro manual: es algo
reportado, no medido por DevHQ. Una incidencia conserva quién la abrió; cada cambio de estado es
un evento con la procedencia de quien lo hizo. La [puerta para agentes](#puerta-para-agentes) por
ahora solo lee, así que hoy nada escribe registros `agent`.

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
  lo que vive el proceso. Cerrar la ventana termina la supervisión: cancela las ejecuciones en
  vuelo y el bucle, y desde ese momento no se registra nada más. Una ejecución cuya petición ya
  terminó no se puede interrumpir mientras escribe, así que su escritura comprueba en su
  transacción, ya con el lock de escritura de SQLite, que la supervisión sigue viva: si el cierre
  llegó antes, no confirma nada; si ya estaba confirmando, el cierre la espera (lo que tarda ese
  commit en llegar al disco y su conexión en cerrarse, nunca a otro escritor). No hay servicio,
  tarea del sistema ni arranque solo.
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
- Cada lectura del inventario lleva **todos** los registros manuales y de agentes, pero solo los últimos
  200 resultados automáticos de cada aplicación o recurso (con sus eventos): la supervisión
  puede añadir uno cada 5 min durante meses. Los anteriores siguen en SQLite, intactos, y la
  UI lo dice: una nota indica desde cuándo los resultados automáticos están completos y una
  línea marca dónde una lista pasa a tener solo registros manuales y de agentes. En el historial de
  observaciones de un sistema, los anteriores se cargan bajo demanda, de 200 en 200, anclados
  al más antiguo ya mostrado para que los resultados nuevos no causen saltos ni duplicados. Una vista que mezcla
  sistemas (Activity) solo muestra resultados automáticos desde el momento en que los de
  todos están completos, para no sugerir que uno no tuvo resultados. Activity y la Activity
  de una aplicación también cargan los anteriores bajo demanda: Rust recorre, por el índice de
  cada sistema, los que preceden al evento del corte y los mezcla fila a fila, tomando siempre el
  más reciente, hasta reunir los 200 siguientes en el orden de la línea de tiempo; de cada sistema
  lee solo lo que entra en la página y una fila más para comparar. Así la vista queda completa
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
  global tarda 1 ms, manda 79 KB y lee 0.15 MB, igual al principio, a la mitad o al final del
  historial; con 20 y 50 sistemas lee 0.25 y 0.45 MB y tarda 2 ms. Cuando leía una página de cada
  sistema (201 posiciones) para devolver 200 en total, y cada fila de un sistema cae en una página
  del archivo distinta entre las de los demás, leía 0.37 MB con 5 sistemas, 1.2 MB con 20 y
  14.5 MB con 50 (14 ms). Los eventos de la ventana se buscan por el índice de su sistema; cuando se
  buscaban por el ID aleatorio de su observación, cada búsqueda caía en una página distinta de
  un índice tan grande como el historial y la recarga leía 3.9 MB a los 90 días. Las cifras
  son de Linux, en release y con la caché del sistema caliente (`history_costs`, ver
  [Ejecutar](#ejecutar)); en otros sistemas esa prueba no mide lo leído del archivo. En una
  laptop con Windows 11 (i5-13420H), con la protección en tiempo real de Defender activa, dos
  corridas dieron 8.9 y 12.6 ms por recarga, 2.2 a 2.4 ms por página de Activity global al
  principio, a la mitad y al final del historial, y 2.2–3.2, 2.7–2.8 y 4.1–4.3 ms con 5, 20 y 50
  sistemas, con los mismos 855 y 79 KB hacia la ventana: más lento que en Linux, pero igual de
  independiente de lo lejos que esté la página.
- El proceso nativo empuja el estado a la ventana por un `Channel` de Tauri: ejecución en
  curso, última y siguiente ejecución y último error de DevHQ. Cuando una ejecución
  registra evidencia, la ventana relee el inventario sin estado de carga, así que los
  formularios abiertos y los registros desplegados se conservan.

### Respaldo

**Back up now**, en Settings › Database file, copia el inventario con DevHQ abierto. Es el
comando `backup_inventory`, sin argumentos, que corre fuera del hilo principal y de uno en uno:
mientras se hace un respaldo, la ventana desactiva el botón y Rust rechaza otro. No necesita
plugins ni permisos, no cambia el esquema y no se registra en la línea de tiempo: no es evidencia
de ningún sistema.

- SQLite copia con `VACUUM INTO`, que lee el último estado confirmado como cualquier lector en
  WAL: no espera a ningún escritor (la supervisión sigue registrando) y no incluye nada sin
  confirmar. La copia es un único archivo en modo journal de rollback, completo por sí solo.
- Se escribe en `backups/` junto a `inventory.sqlite3`, primero como
  `inventory-<UTC>.sqlite3.partial`, donde `<UTC>` es el momento en el formato básico de ISO 8601,
  con milisegundos y sin `:` (por ejemplo `inventory-20261001T120320.450Z.sqlite3`). El propio
  respaldo crea ese `.partial`; si ya existe, falla sin tocarlo.
- Después la abre aparte, en solo lectura: `PRAGMA quick_check` debe responder `ok` y
  `user_version` debe ser la versión del esquema. Luego la fuerza a disco y solo entonces le da
  su nombre final, con un enlace que el sistema de archivos rechaza si ese nombre ya existe, y
  borra el `.partial`. Así nunca reemplaza un archivo, y un nombre final siempre es una copia que
  estaba completa y verificada al hacerse. DevHQ nunca borra ni reescribe un respaldo terminado.
  La carpeta de datos suele estar en el disco del sistema (NTFS, APFS, ext4…), que admite enlaces; en un
  sistema de archivos que no los admita, como FAT32 o exFAT, el respaldo fallaría al nombrar la
  copia, con un error recuperable y sin dejar archivos. En Windows, el workflow hace el respaldo con
  la protección en tiempo real de Defender activa (ver [Ejecutar](#ejecutar)); otros antivirus siguen
  sin probarse.
- Si algo falla, borra el `.partial` y la ventana muestra un error recuperable; el inventario no
  cambia. Al terminar, muestra la hora, el tamaño, la verificación y cuántas filas de cada tabla
  tiene la copia.
- Debajo, `list_backups` lista los archivos de la carpeta tal como están, con su fecha de
  modificación y su tamaño, sin presentarlos como sanos: DevHQ verificó cada respaldo una vez, al
  hacerlo, y no ha vuelto a mirarlos. Un `.partial` se marca como incompleto, no como respaldo.
- Los respaldos están en el mismo disco que el inventario y dentro de la carpeta de datos de
  DevHQ, `%APPDATA%\com.devhq.desktop` en Windows; la ventana avisa de copiarlos a otro sitio. El
  desinstalador solo borra esa carpeta si marcas «Delete the application data» en su página de
  confirmación, una casilla que aparece desmarcada. Marcada, la borra entera, con el inventario y
  `backups/`, y también `%LOCALAPPDATA%\com.devhq.desktop`, el perfil de WebView2.
  Una desinstalación silenciosa (`/S`) o pasiva (`/P`) no muestra esa página y una actualización
  no la aplica, así que ninguna borra datos. Así lo dice la plantilla NSIS de tauri-bundler que
  trae `@tauri-apps/cli` 2.11.5, y el workflow de Windows lo comprueba en los dos sentidos: tras una
  desinstalación silenciosa siguen el inventario y sus respaldos, y con la casilla marcada desaparecen
  las dos carpetas.
- Se restauran como cualquier copia (ver [Arquitectura](#arquitectura)). La preview rechaza el
  respaldo como toda escritura y muestra una carpeta de ejemplo.

### Puerta para agentes

`devhq --mcp` arranca la puerta para agentes: el mismo ejecutable, sin ventana ni WebView, habla el
Model Context Protocol (JSON-RPC 2.0, un mensaje por línea en stdin y stdout) con un agente como
Claude Code, para que haga lo que tú haces en la ventana cuando se lo pides. Se registra una sola vez,
con alcance de usuario, y desde entonces cualquier sesión local de Claude Code la tiene:

```sh
claude mcp add --scope user devhq -- "<ruta del ejecutable de DevHQ>" --mcp
```

El instalador pone el ejecutable en `%LOCALAPPDATA%\DevHQ`. El cliente lanza la puerta al empezar una
sesión y la termina al cerrarla (al cerrarse stdin): no abre ningún puerto, no acepta conexiones, no
queda nada corriendo ni guarda credenciales. Abre el mismo `inventory.sqlite3` que la app, en el
directorio de datos que Tauri le da (`%APPDATA%\com.devhq.desktop` en Windows; un test lo compara con
el que resuelve Tauri), o el que se le indique con `--database <ruta>`. Si ahí no hay inventario, lo
dice y no crea uno vacío donde tu DevHQ no lo encontraría.

Hasta dónde llega un agente lo decides tú en Settings → **Agent gate**: **Off** (por defecto, también
tras actualizar), **Read only** o **Read and write**. La elección se guarda en el inventario
(`agent_access`), porque la ventana la cambia y la puerta, otro proceso, la lee en cada llamada: se
aplica de inmediato, también a una sesión ya abierta. No es una barrera de seguridad (el archivo es
tuyo y está en tu disco) sino de intención: ningún agente lee ni escribe en tu inventario sin que lo
hayas decidido. Con la puerta cerrada solo responde `status`, sin decir qué contiene el inventario,
para que el agente pueda explicarte dónde abrirla; las demás herramientas responden
`access_denied`. Settings también muestra el comando de registro con la ruta real del ejecutable,
para copiarlo; DevHQ nunca lo ejecuta ni toca la configuración de Claude Code. Por ahora la puerta
solo lee, así que Read and write funciona como Read only:

| Herramienta    | Qué responde                                                                                                                                                                                 |
| -------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `status`       | Qué archivo abrió, dónde lo guarda de verdad el sistema (`storedAt`, como Settings), su versión de esquema, cuántos sistemas tiene y qué no puede saber                                      |
| `attention`    | «¿Qué necesita mi atención?» con el mismo orden que Overview, la cobertura, lo nunca observado y lo que nada supervisa; el estado solo es `covered` si todo tiene evidencia vigente          |
| `list_systems` | Cada aplicación y recurso con su estado de evidencia y última observación, incidencias activas, comprobación HTTP y relaciones                                                               |
| `get_system`   | Una aplicación o recurso (por ID, o por nombre exacto sin distinguir mayúsculas) con sus observaciones, incidencias y eventos cargados, y desde cuándo su historial automático está completo |
| `history`      | Los resultados automáticos anteriores de un sistema, de 200 en 200, como la carga bajo demanda de la ventana                                                                                 |

Todas se declaran de solo lectura (`readOnlyHint`). Cada respuesta dice lo que la ventana diría: una
lista recortada lleva el aviso de su ventana de historial y nada sin evidencia vigente se presenta
como sano. La atención se calcula con la copia en Rust de las reglas de la ventana, atada a ellas por
`contract/attention/`. El estado en vivo de la supervisión (ejecución en curso, siguiente, último
error de DevHQ) solo existe en el proceso de la ventana, y `status` dice que la puerta no lo conoce.
Un fallo de un caso de uso vuelve como resultado con `isError` y el mismo mensaje que vería la
ventana; un método o una herramienta desconocidos, como error de JSON-RPC.

En Windows, una puerta lanzada desde una app empaquetada (MSIX), como la app de escritorio de Claude,
ve `AppData` redirigido (ver [Arquitectura](#arquitectura)) y podría leer otro inventario: `status`
lo dice con `storedAt`. Claude Code desde una terminal normal no está empaquetado. El workflow de
Windows instala DevHQ y habla con la puerta del ejecutable instalado como lo hace un cliente MCP:
comprueba que responde por stdin y stdout aunque no tenga consola, que abre el inventario de
`%APPDATA%` y que termina al cerrarse su entrada.

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
npm run e2e:native        # la app real (Rust, SQLite y WebView) con tauri-driver
cargo test --manifest-path src-tauri/Cargo.toml --lib # persistencia y migraciones
cargo test --release --manifest-path src-tauri/Cargo.toml --lib history_costs -- --ignored --nocapture
                          # mide recarga y Activity con 90 días de supervisión,
                          # y una página de Activity con 5, 20 y 50 sistemas
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
npm run tauri build       # ejecutable e instalador NSIS para Windows
```

`preview.html` existe solo en desarrollo: carga la ventana real sobre un proceso nativo
simulado con `@tauri-apps/api/mocks` y datos de ejemplo relativos al reloj, con un recurso
supervisado que tiene más resultados de los que lleva un snapshot. Los comandos de lectura
siguen las mismas reglas de ventana y paginación que Rust (lo prueba
`src/preview/backend.test.ts`), toda escritura se rechaza sin guardar nada y una franja propia
sobre la ventana dice que son datos de ejemplo; la ventana ocupa el resto, así que el aviso no
tapa nada al desplazarse. `?scenario=empty` y `?scenario=unavailable` muestran un inventario
vacío o ilegible. El bundle de producción se construye desde `index.html` y no contiene nada de
esto. `npm run e2e` usa el Chrome instalado (en CI, el del runner) o el ejecutable que indique
`DEVHQ_CHROMIUM`; nunca descarga navegadores, y reutiliza un `npm run dev` en marcha. Antes de
la primera prueba carga la preview una vez (`e2e/warm-up.ts`): Vite transforma cada módulo la
primera vez que se pide y no guarda nada entre arranques. En una laptop con Windows (i5-13420H),
con seis pestañas a la vez, la primera ventana tardaba unos 3.5 s de los 5 s que una prueba le
da para aparecer, y a veces más: fallaban las seis primeras pruebas, o una con 12 workers. Además de
recorrer el historial y comprobar que el aviso no tapa la ventana, audita con axe-core (reglas
WCAG A/AA y buenas prácticas) cada página real de la preview, algunos formularios abiertos y la
demo: landmarks, orden de encabezados, nombres accesibles y contraste. Solo se excluye el rótulo decorativo de
las miniaturas de la demo, oculto a tecnologías de asistencia y exento de contraste.

`npm run e2e:native` recorre la app real, no la preview. `e2e/native/build.ts` compila DevHQ en
modo debug con el frontend embebido y el recorrido lo maneja por WebDriver. En Linux lo lanza
`tauri-driver` 2.1.0 a través de `WebKitWebDriver` (paquete `webkit2gtk-driver`). En Windows
`tauri-driver` ya no puede: desde WebView2 150, `msedgedriver` le pasa a la app su puerto de
depuración y su carpeta de datos por variables de entorno, pero Tauri fija los argumentos y la
carpeta de WebView2 por su API, que manda, así que el puerto nunca se abre (`DevToolsActivePort
file doesn't exist`). Allí la build de prueba, y solo ella, abre el puerto de DevTools de WebView2
en `127.0.0.1` con una configuración que se fusiona al compilarla; el recorrido arranca DevHQ y un
`msedgedriver` de la misma versión que WebView2 se engancha a ese puerto. El instalador se compila
sin ella. El cliente WebDriver es un archivo propio sobre `fetch` (`e2e/native/webdriver.ts`): el
protocolo es JSON sobre HTTP, así que no hace falta ninguna dependencia, y cada petición que hace
está a la vista. El recorrido parte de una base v6 en modo journal de rollback, como las anteriores
a WAL. Abre Mi inventario, comprueba la ruta que muestra Settings y «Copy path», crea un recurso y
una aplicación, registra una observación, supervisa una comprobación HTTP contra un servidor propio
en `127.0.0.1` (esa ejecución y la recarga de su evidencia solo llegan a la ventana por el
`Channel` de supervisión), la ejecuta a petición, lee el panel Supervision y pulsa Back up now: el
archivo debe estar en `backups/`, sin `.partial`, con `quick_check` correcto, la versión del
esquema y las filas del inventario, y la ventana debe listarlo sin ninguna copia incompleta (un
antivirus que aún retuviera el `.partial` borrado mientras lo analiza lo dejaría a la vista en la
lista). Por último cierra la ventana como
lo hace su botón, nunca por WebDriver: con `WM_DELETE_WINDOW` en X11 (un script de Python que llama
a libX11, porque bajo Xvfb no hay gestor de ventanas que lo envíe) o con `WM_CLOSE` en Windows. El
proceso debe terminar, sin dejar `inventory.sqlite3-wal`, y el archivo debe haber pasado a WAL.
Antes de hacer clic espera a que el elemento deje de moverse, porque el contenido que llega tarde
empuja la página, y guarda una captura por paso en `test-results/native/`. `DEVHQ_APP` lo apunta a
otro ejecutable.

Nunca toca un inventario real. En Linux, todas las carpetas XDG apuntan a una carpeta temporal y,
si no hay pantalla, arranca su propio Xvfb. Windows resuelve sus carpetas con la API de carpetas
conocidas, que ninguna variable de entorno redirige: allí solo corre si aún no existen las
carpetas de DevHQ en `%APPDATA%` y `%LOCALAPPDATA%` (en un equipo con datos se niega) y al
terminar borra las que creó. Con la versión síncrona de `watch_http_monitor`, este recorrido falla
en su primer paso: en Linux el pánico no puede atravesar la llamada de WebKitGTK que entrega la
petición, así que el proceso abortaba y la ventana se cerraba en cuanto abría Mi inventario.

El workflow `Windows` (`.github/workflows/windows.yml`) corre en `windows-latest` lo que solo
Windows puede mostrar. Pasa los tests de Rust sobre NTFS y SChannel tres veces: la segunda con `TMP`
y `TEMP` en una carpeta con espacios y acentos (`temp_dir()` lee `TMP`), de modo que cada base de
prueba, `VACUUM INTO`, el enlace duro del respaldo y el WAL viven en esa ruta; un
`FileSystemWatcher` registra lo que los tests crean allí, y el paso falla si no ve nada o si los
tests dejan algo. Cambiar `TMP` hace que Cargo vuelva a compilar el paquete, y el compilador y el
enlazador escriben allí archivos propios, así que compila primero, anota lo que deja la compilación y
ejecuta después el binario de tests directamente, desde la carpeta del paquete como Cargo. La tercera,
con la protección en tiempo real de Microsoft Defender activa, como el recorrido. Construye
la app de release y su instalador NSIS con `npm run tauri build` y guarda el instalador como
artefacto. Compila después la misma app con el puerto de DevTools de la build de prueba y la
recorre sobre WebView2. Instala el instalador en silencio para el usuario actual, abre la app
instalada sobre el inventario del recorrido (debe mostrar su ventana, arrancar WebView2 y terminar
al cerrarla sin dejar `-wal`) y la desinstala en silencio: deben quedar el inventario y sus
respaldos. Después la instala otra vez y la desinstala como una persona
(`.github/windows/uninstall-deleting-data.ps1`): lanza `uninstall.exe` sin `/S`, comprueba que
«Delete the application data» aparece desmarcada, la marca con `BM_SETCHECK` (la plantilla lee la
casilla con `BM_GETCHECK` al dejar la página, así que es lo mismo que un clic) y pulsa Uninstall y,
al terminar, Close con `WM_COMMAND`. Deben desaparecer `%APPDATA%\com.devhq.desktop`, con el
inventario y `backups/`, `%LOCALAPPDATA%\com.devhq.desktop`, el ejecutable y la entrada de
desinstalación.

El runner trae Defender sin protección en tiempo real y con `C:\` y `D:\` excluidas del análisis.
Antes de la tercera pasada de los tests de Rust y del recorrido, `.github/windows/defender.ps1` la
activa, junto con la supervisión de comportamiento y el análisis de descargas, como en un escritorio;
quita todas las exclusiones y comprueba con `Get-MpComputerStatus` que quedó activa. Que lo diga no
prueba que analice: Defender tarda unos segundos en aplicar la nueva configuración, y la primera
versión de la sonda, que escribía un solo archivo nada más activarla, no vio ninguna detección. Por
eso escribe archivos de prueba EICAR (la cadena inofensiva que todo antivirus detecta a propósito) en
la carpeta temporal o junto a la carpeta de datos, uno nuevo cada pocos segundos, y los vuelve a abrir
mientras espera, porque Defender analiza un archivo al cerrarlo tras escribirlo y otra vez al abrirlo.
El paso falla si en un minuto no borra ni bloquea ninguno, y antes imprime las versiones del motor y
de las firmas y le pide que analice la última prueba. En el runner bloquea la tercera al abrirla,
unos 12 s después de activarse. Con la protección activa pasan todos los tests de Rust (en unos 33 s
en lugar de 4) y el recorrido: Back up now crea el `.partial`, lo enlaza con su nombre final y lo
borra mientras Defender analiza cada archivo que se escribe, y la instalación y las desinstalaciones
también corren bajo su análisis. Tras el recorrido comprueba que la protección siguió activa, y al
final de cada job la apaga, para que guardar las cachés no analice cada archivo.

En el recorrido de Windows el servidor de la comprobación habla HTTPS: el workflow crea un
certificado para `127.0.0.1`, lo añade a las raíces de confianza del equipo y se lo pasa al recorrido
con `DEVHQ_E2E_TLS_PFX`, así que DevHQ tiene que completar un handshake con SChannel y el almacén de
certificados de Windows (los tests de Rust solo ven fallar uno). La primera ejecución del workflow
destapó que `cargo test` no arrancaba en Windows: tao, muda y los diálogos de Tauri importan `TaskDialogIndirect`, que solo existe en Common Controls
v6, y un proceso solo obtiene esa versión por su manifiesto. tauri-build lo incrusta como recurso
que Cargo enlaza solo en los binarios, así que el ejecutable de tests no lo llevaba y Windows se
negaba a iniciarlo (`STATUS_ENTRYPOINT_NOT_FOUND`) antes del primer test. Con el linker de MSVC,
`src-tauri/build.rs` incrusta ahora ese mismo manifiesto (`src-tauri/windows-app-manifest.xml`) en
cada ejecutable del paquete, la app incluida, y tauri-build ya no pone el suyo, así que hay uno
solo.

Los minutos de Windows cuentan doble en un repositorio privado, así que el workflow corre entero a
mano (`workflow_dispatch`, que GitHub solo ofrece cuando el workflow está en la rama principal) y, en
un pull request, solo cuando un push cambió algo que prueba. No usa un filtro `paths`: en
`pull_request` compara todo el pull request con su base, así que, en cuanto uno tocaba `src-tauri/`,
Windows corría en cada push. En su lugar, un job previo en Linux (`.github/windows/needs-windows.sh`)
busca el commit más reciente de la rama en el que pasaron todos los jobs de Windows y mira qué cambió
desde él: Windows corre si cambió `src-tauri/`, `contract/`, `e2e/native/`, el propio workflow, sus
scripts (`.github/windows/`) o `package-lock.json`, que fija la CLI de Tauri que construye el
instalador. Si no, sus jobs quedan como omitidos y el job previo dice por qué. Una ejecución fallida o
cancelada no cuenta, así que la siguiente vuelve a probar lo que esa no terminó (un push nuevo cancela
la ejecución en curso de la misma rama). Si ningún commit de la rama pasó todavía, o GitHub no lista
las ejecuciones anteriores, compara con el punto en que la rama salió de su base, como hacía el
filtro: un job omitido nunca esconde un cambio del pull request sin probar.

Lo que el workflow no hace como una persona se comprobó a mano el 2 oct 2026 en una laptop con
Windows 11 (i5-13420H), WebView2 154 y Defender en tiempo real, con `npm run tauri dev`, con el
instalador y con el `devhq.exe` de release. Una base v5 en journal de rollback se migró a v6 al
abrir, con todas sus filas y `rowid` intactos, y pasó a WAL; `-wal` y `-shm` aparecen al abrir cada
conexión y desaparecen al cerrarla, y al cerrar la ventana el proceso terminó en menos de 200 ms
sin dejar `-wal`. La comprobación atrasada corrió 10.0 s después de abrir y las siguientes cada 5
min, también durante los respaldos. SChannel contra internet: `https://example.org` dio Healthy
(HTTP 200 en ~100 ms), `expired.badssl.com` y `self-signed.badssl.com` dieron Unhealthy con el error
de Windows como evidencia (`CERT_E_EXPIRED`, os error -2146762495, y `CERT_E_UNTRUSTEDROOT`, os error
-2146762487) y un dominio `.invalid`, Unhealthy por DNS (os error 11001). «Copy path» copió la ruta
exacta. Cinco Back up now seguidos tardaron 10–16 ms cada uno (`.partial`, nombre final y borrado,
sin que Defender retuviera nada) y dieron copias idénticas byte a byte; un `.partial` puesto a mano
salió como incompleto, y restaurar una copia como dice «By hand» mostró exactamente su contenido,
más la ejecución atrasada que la supervisión hizo al abrir.
El instalador pide la carpeta (`%LOCALAPPDATA%\DevHQ`, sin permisos de administrador) y al final
ofrece abrir DevHQ y crear un acceso directo en el escritorio. Desinstalar con «Delete the
application data» desmarcada conservó el inventario, los respaldos y el perfil de WebView2, y quitó
el ejecutable, los accesos directos y la entrada de desinstalación; marcada, borró también
`%APPDATA%\com.devhq.desktop` y `%LOCALAPPDATA%\com.devhq.desktop`. Lanzado desde la terminal de la
app de escritorio de Claude, DevHQ guardaba sus datos en la carpeta del paquete de esa app (ver
[Arquitectura](#arquitectura)). Con Build Tools en español, el enlazador escribe «Creando
biblioteca…» y rustc lo muestra como aviso `linker_messages`, porque solo reconoce el texto en
inglés; no afecta a nada.

Si DevHQ no termina tras cerrar su ventana, antes de matar el proceso ejecuta
`powershell -NoProfile -File scripts/dump-stacks.ps1 -ProcessId <pid>`: vuelca la pila de
cada hilo con `dbghelp.dll` de Windows y el PDB de release, sin instalar nada.

El instalador se genera en `src-tauri/target/release/bundle/nsis/`. El target de
empaquetado está configurado para Windows; otros sistemas necesitan sus prerrequisitos
y seleccionar un target de bundle apropiado. Los lockfiles de npm y Cargo se versionan. El
de npm lo escribe npm 11: npm 10 borra los campos `libc` de los paquetes de cada plataforma
cada vez que lo reescribe. Los archivos de texto llevan LF en el repositorio y en cada copia de
trabajo (`.gitattributes`): Git for Windows se instala con `core.autocrlf=true` y, sin eso, los
escribiría con CRLF, que `prettier --check` rechaza aunque el código no cambie. En las sesiones
de Claude Code en la web,
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
contract/          Argumentos y respuestas del IPC, y casos de atención, que comprueban los tests de ambos lados
```

React + TypeScript estricto + Vite + Tailwind CSS 4. El estado de filtros es local;
las rutas y tabs usan React Router con hash, compatible con el protocolo empaquetado
de Tauri. No hay servidor HTTP de aplicación, store global ni capa de repositorios
artificial. Los selectores de dominio no dependen de React ni de los mocks. Las vistas
consumen explícitamente el escenario durante esta etapa.

Rust expone `list_inventory` (una lectura consistente de inventario, registros y
`httpChecks`), `data_location`, `backup_inventory`, `list_backups`, `save_application`, `save_resource`, `delete_application`,
`delete_resource`, `record_observation`, `older_observations`, `older_events`, `open_incident`,
`update_incident_status`, `record_event`, `save_http_check`, `set_http_check_paused`,
`remove_http_check`, `run_http_check` y `watch_http_monitor`. Los comandos de
comprobaciones y de borrado son asíncronos y avisan a la supervisión antes de responder, así
que pausar, reanudar, quitar o borrar surten efecto de inmediato; `run_http_check` pasa por la supervisión para
no duplicar una ejecución en vuelo y `watch_http_monitor` abre el `Channel` de estado. Ese último también es
asíncrono porque lanza la tarea que alimenta el `Channel`: Tauri ejecuta un comando síncrono en el hilo
que recibe la petición (el principal), que no tiene runtime de Tokio, y lanzarla desde ahí provocaba un
pánico que en Linux abortaba el proceso al abrir Mi inventario (lo comprueba `npm run e2e:native`). Abre
`inventory.sqlite3` en el directorio de datos de la aplicación que proporciona Tauri; las
migraciones transaccionales v1→v8 usan `PRAGMA user_version` y una versión más nueva se
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
leer): con DevHQ abierto se respalda con Back up now (ver [Respaldo](#respaldo)); con DevHQ
cerrado también basta copiarlo. Para restaurar un respaldo o una copia, con DevHQ cerrado se pone
en lugar del archivo, con el nombre `inventory.sqlite3`, y se borran `-wal` y `-shm` si quedaron,
para que ningún log del archivo anterior se aplique a la copia.

Esa ruta es la que DevHQ pide, y el sistema puede guardar el archivo en otra. Un enlace simbólico o un
junction llevan a otra carpeta, y Windows redirige `AppData` para un DevHQ lanzado desde una app
empaquetada (MSIX), como las apps de escritorio de Claude o Codex: lo que ese proceso crea bajo
`AppData` va a la carpeta del paquete
(`%LOCALAPPDATA%\Packages\<paquete>\LocalCache\Roaming\com.devhq.desktop`) y el proceso ve una mezcla
de las dos. El Explorador, el DevHQ instalado y cualquier otro programa ven el `%APPDATA%` real, donde
ese inventario no está o hay otro, así que un respaldo puesto allí no lo restaura. Por eso
`data_location`, `backup_inventory` y `list_backups` preguntan al sistema dónde está cada archivo
(`std::fs::canonicalize`, que en Windows llama a `GetFinalPathNameByHandleW`) y, si no es la ruta
pedida, lo devuelven en `storedAt`: Settings muestra y copia esa ruta, dice cuál pidió DevHQ y por
qué difieren, y señala cada respaldo que el sistema guarda fuera de la carpeta listada. Se pregunta
archivo por archivo porque Windows resuelve así: una carpeta que existe en los dos lados se resuelve
a la real, aunque guarde archivos de las dos. Un junction en Windows y un enlace simbólico en Linux
lo prueban, porque llegan a la carpeta de datos por otra ruta igual que la redirección, que un test
no puede montar.

La v4 reconstruye `observations` y `events` (SQLite no cambia
un CHECK con ALTER) conservando filas, IDs, `rowid` (desempate de orden), claves foráneas,
índices y los vínculos evento → observación. La v5 solo reconstruye `http_checks` (nada la
referencia) para añadir el estado y el intervalo de supervisión con sus CHECK, conservando
el `rowid` que ordena la lista. Ambas se ejecutan con las claves foráneas desactivadas y
solo se confirman si `PRAGMA foreign_key_check` no encuentra relaciones rotas. La v6 solo
añade dos índices parciales sobre los registros manuales; ninguna fila cambia. La v7 reconstruye
`observations`, `incidents` y `events` como la v4 para admitir el origen `agent` y la columna
`agent`, que un CHECK exige exactamente con ese origen; las filas existentes conservan su origen
sin agente. Sus índices parciales pasan a cubrir los registros manuales y de agentes, que se cargan
siempre completos. La v8 solo añade `agent_access`, una fila como mucho con el modo de la puerta
para agentes; ninguna fila cambia y, sin fila, la puerta está cerrada. Los
registros usan dos columnas con clave foránea (`application_id`/`resource_id`) y un CHECK
que exige exactamente una; las referencias de historial son `ON DELETE RESTRICT`, así que
nada puede borrar un sujeto y dejar evidencia huérfana; los casos de uso de borrado lo
comprueban antes y explican qué lo impide. Las referencias inexistentes se rechazan antes de escribir y dentro de la misma
transacción. Los tiempos se guardan en milisegundos UTC y viajan como RFC 3339; no se
aceptan momentos futuros. La UI invoca esos casos de uso con contratos tipados, y unos fixtures JSON
compartidos los atan a Rust: `contract/commands/` guarda, por comando, los argumentos exactos que
envía `inventoryApi`, y `contract/responses/` lo que Rust responde, incluidos el mensaje del `Channel`
de supervisión y un rechazo. Vitest comprueba con `mockIPC` que `inventoryApi` envía exactamente esos
argumentos, con valores que el compilador ata a los tipos de la ventana, y que un valor de cada tipo
que espera la ventana es igual a su respuesta. Rust pasa cada fixture por el manejador real de
comandos, sobre el runtime simulado de Tauri y contra una base sembrada: el comando debe existir,
aceptar esos nombres de argumentos y esos valores (los de los enums pasan su validación) y terminar
bien, y cada argumento debe leerse entero, porque serde ignora una clave que no conoce y leería como
ausente un campo opcional renombrado (una edición se volvería un alta). Su serialización de cada
respuesta debe ser igual al fixture. Renombrar un campo, un argumento, un comando o un valor en
cualquiera de los dos lados rompe un test. La respuesta a «¿Qué necesita mi atención?» (orden de
urgencia, cobertura y estado global) también se calcula en Rust (`attention.rs`) para la puerta para
agentes, que no tiene ventana a la que preguntar; `contract/attention/` guarda casos con un
inventario, un instante y el resultado esperado, y Vitest y Rust comprueban contra ellos sus dos
versiones, empates y fin exacto de la vigencia incluidos, así que la puerta nunca responde distinto
que la ventana. Se conserva la barra nativa de ventana. La CSP limita recursos y las
capabilities no conceden permisos adicionales. No se implementan autenticación,
cuentas, cloud, sync, telemetría, scanners, automation engine, Runner ni background service.

## Evolución prevista

1. Hecho: una comprobación HTTP(S) a petición con su propio `source`, sin cambiar la
   semántica de vigencia.
2. Hecho: supervisión cancelable de esas comprobaciones, solo con DevHQ abierto.
3. En curso: la [puerta para agentes](#puerta-para-agentes), con la que un agente lee y mantiene el
   inventario cuando se lo pides, con su procedencia siempre visible. Ya lee, con el acceso que
   eliges en Settings; escribir y que la ventana abierta note sus cambios siguen en diseño:
   [docs/puerta-para-agentes.md](docs/puerta-para-agentes.md).
4. Después, automatizaciones con resultado e historial, y seguridad con almacén nativo de
   credenciales y evidencias contextualizadas.
5. A largo plazo, una supervisión que siga con la ventana cerrada y avise de lo que necesita
   atención, hecha por DevHQ mismo y no por un agente. Contradice que DevHQ no funcione en segundo plano, así que exige revisar esa regla y
   cuidar lo que consume.

Estos son límites conceptuales, **no módulos vacíos ni integraciones implementadas**.
La dirección es UI → IPC/casos de uso → dominio. SQLite conserva contexto; las credenciales no deben
vivir en componentes React, localStorage ni texto plano en SQLite.

Las pruebas cubren navegación, tabs/teclado, filtros, rutas desconocidas, relaciones
compartidas, cronología de la demo, salud desconocida, persistencia tras reapertura,
rechazo de referencias inválidas sin escrituras parciales, restricciones del esquema,
migraciones v1/v2→v3, v3→v4, v4→v5, v5→v6, v6→v7 y v7→v8 (incluida la negativa a confirmar relaciones rotas),
vigencia y sustitución de observaciones de ambos orígenes, orden de atención, errores de
carga visibles y recuperables, comprobaciones contra un servidor local (estado esperado,
500, redirect, bucle de redirects, tiempo agotado, conexión rechazada, dominio `.invalid`
y fallo TLS), configuraciones inválidas sin escrituras parciales, errores de DevHQ que
no producen observación, la distinción de origen en cada vista real, el contrato IPC de cada comando, el contrato de atención, el respaldo
(las mismas filas que el origen comparadas una a una, sin lo no confirmado y sin esperar a un escritor,
sin reemplazar ni borrar un archivo ajeno, uno a la vez, y una copia dañada que se descarta sin nombre
final) y el borrado (qué lo
impide, la confirmación y que un resultado tardío de un sujeto borrado no se registra). La supervisión se
prueba sobre un reloj simulado de Tokio, sin esperas reales: ritmo por intervalo,
continuidad tras reabrir, una sola ejecución por atraso, sin solapes, Run check que se une
a la ejecución en vuelo, pausa, cancelación al quitar o cerrar (también de una ejecución que
ya estaba escribiendo su resultado al cerrar), fallos de DevHQ visibles
que no detienen al resto y configuración ilegible que se reintenta; y, con peticiones
reales a un servidor local, que la evidencia programada es idéntica a la manual; y, a través del manejador real de comandos sobre el
runtime simulado de Tauri y desde un hilo sin runtime asíncrono como el principal, que la ventana puede
abrir el `Channel` de estado. La ventana
de historial se prueba por su límite exacto, los eventos que acompañan a cada resultado, el
aviso de lo omitido en cada vista, la paginación de observaciones y de eventos de varios
sistemas a la vez (sin huecos ni duplicados, con empates al mismo milisegundo, resultados
nuevos entre páginas y doce sistemas de historiales desiguales), que la mezcla toma de cada
sistema solo lo que devuelve de él y una fila más, y los planes de consulta, que nunca recorren el historial automático
completo ni buscan un evento automático por su observación, tampoco con estadísticas que otra
herramienta haya calculado sobre el archivo. La concurrencia de SQLite se prueba sin esperas medidas: una lectura no espera a una
escritura en curso, una escritura no espera a una lectura abierta (que conserva su estado), dos
primeras aperturas simultáneas migran una sola vez, una base antigua ocupada se abre sin
convertirse y se convierte en la siguiente apertura, y al cerrar queda un único archivo
completo. Por encima de todo eso, el recorrido nativo de `npm run e2e:native` prueba la app real,
con su ventana, de punta a punta (ver [Ejecutar](#ejecutar)).
