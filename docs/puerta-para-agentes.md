# Puerta para agentes (diseño)

**Estado: en construcción.** Hechos: los pasos 1 (esquema v7 y procedencia `agent` en la UI), 2 (la
atención calculada en Rust, atada a la de la ventana) y 3 (`devhq --mcp` con las herramientas de
lectura; ver «Puerta para agentes» en el README). Del paso 4, hecho el interruptor de acceso
(esquema v8 y Settings → Agent gate).
Cuando una parte se implementa, su comportamiento pasa al README (que es la especificación) en el
mismo commit, y este documento se recorta.

## Qué resuelve

Mantener el inventario a mano cuesta tiempo: dar de alta aplicaciones con sus componentes y
recursos, configurar comprobaciones, revisar el estado de todo, anotar despliegues o abrir
incidencias. La puerta deja que un agente (Claude Code, Codex u otro cliente MCP) haga lo mismo que
haces tú en la ventana, **cuando se lo pides**: «revisa el estado de todas mis apps», «agrega la
app X que corre en el servidor Y y compruébala cada 15 min», «anota que desplegué la 1.4.0».

## Lo que no cambia

- **DevHQ sigue observando, no operando.** La puerta solo lee y escribe el inventario de DevHQ y,
  como la ventana, lanza comprobaciones HTTP(S) de solo lectura. No ejecuta comandos ni toca
  servidores, repositorios ni nada fuera de su archivo.
- **No hay segundo plano.** La puerta no es un servicio: es un proceso que el cliente MCP arranca al
  empezar una sesión y termina al cerrarla. Sin sesión de agente, no existe y no gasta nada.
- **No hay credenciales ni puertos.** Habla por stdin/stdout con el proceso que la lanzó; no abre
  ningún puerto ni acepta conexiones. Cualquier programa local que pueda lanzarla también puede
  abrir el archivo SQLite directamente, así que no añade superficie de ataque.
- **La procedencia siempre se ve.** El agente actúa con tus mismos permisos, pero lo que registra
  queda marcado como suyo y nunca se confunde con lo que viste tú.
- **La historia no se borra ni se reescribe.** La puerta pasa por los mismos casos de uso de Rust que
  la ventana, con las mismas validaciones, límites y transacciones. Lo que la ventana rechaza, la
  puerta también.

## Forma: un servidor MCP por stdio dentro del mismo ejecutable

`devhq --mcp` arranca el modo puerta del mismo ejecutable que instala el instalador, sin ventana ni
WebView. Así no hay un segundo programa que instalar y la puerta siempre tiene la misma versión que
la app: nunca abre un esquema más nuevo que el suyo (ese caso ya se rechaza) ni migra con reglas
distintas. Se conecta a Claude Code **una sola vez**, con alcance de usuario:

```sh
claude mcp add --scope user devhq -- "C:\Program Files\DevHQ\devhq.exe" --mcp
```

Claude Code lo guarda en su configuración de usuario, así que desde entonces cualquier sesión local
nueva, en cualquier carpeta, tiene las herramientas de DevHQ sin repetir nada: abres la sesión, pides
y la ventana muestra el resultado. Settings mostrará ese comando con la ruta real del ejecutable
instalado (`std::env::current_exe`) para copiarlo. DevHQ no lo ejecuta ni toca la configuración de
Claude Code: escribir en la configuración de otro programa es operar, no observar.

El protocolo es JSON-RPC 2.0 sobre stdio (MCP): `initialize`, `notifications/initialized`,
`ping`, `tools/list` y `tools/call`. Se implementa a mano sobre `serde_json`, que ya es una
dependencia, igual que el cliente WebDriver de `e2e/native/` evita las suyas: son cinco métodos. La
versión del protocolo se negocia en `initialize` (se responde con la del cliente si se soporta). Los
errores de los casos de uso (`invalid_input`, `database_error`, `check_error`…) vuelven como
resultado de herramienta con `isError: true` y el mismo mensaje que ve la ventana, para que el
agente sepa por qué no se hizo.

## Qué inventario abre

El mismo `inventory.sqlite3` que la app: `<directorio de datos>/com.devhq.desktop/`, calculado igual
que el `app_data_dir()` de Tauri (`%APPDATA%` en Windows, `$XDG_DATA_HOME` o `~/.local/share` en
Linux). Un test compara las dos rutas para que nunca se separen. `--database <ruta>` permite apuntar
a otra base, por ejemplo una copia de prueba.

Riesgo conocido: un proceso lanzado desde una app empaquetada (MSIX), como las apps de escritorio de
Claude o Codex, ve `AppData` redirigido a la carpeta del paquete (ver Arquitectura en el README).
Una puerta lanzada desde ahí podría leer o crear **otro** inventario sin que nadie lo note. Por eso,
antes de cada operación la puerta pregunta al sistema dónde está el archivo (`inventory::stored_at`,
lo mismo que usa Settings) y, si no es la ruta pedida, **se niega a escribir** y lo explica en el
error y en `status`. Leer sigue permitido porque `status` dice qué archivo se leyó. Claude Code
desde una terminal normal no está empaquetado y no tiene este problema.

## Acceso: apagado, solo lectura, lectura y escritura

Una preferencia nueva, guardada en SQLite y cambiada solo desde Settings: **Off** (por defecto),
**Read only** o **Read and write**. Activarla es siempre una decisión explícita, igual que la
supervisión. La puerta la lee en cada llamada, así que un cambio en Settings se aplica de inmediato
sin reiniciar nada. Con Off, toda herramienta responde que la puerta está apagada y dónde se
enciende. No es una medida de seguridad (el archivo es tuyo y está en tu disco), sino de intención:
que ningún agente escriba en tu inventario sin que lo hayas decidido.

Además, cada cliente MCP pide permiso por herramienta. Las de lectura llevan `readOnlyHint` y las
de borrado `destructiveHint`. Se recomienda permitir siempre las de lectura y dejar que el cliente
pregunte en las de escritura.

## Herramientas

Los nombres son en inglés como el código. Claude Code los muestra como `mcp__devhq__<nombre>`.

| Herramienta                                      | Qué hace                                                                                                                                                               | Caso de uso que reutiliza                   |
| ------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------- |
| `status`                                         | Archivo abierto y dónde lo guarda el sistema, versión de esquema, modo de acceso, si la ventana de DevHQ está abierta y qué no puede saber la puerta                   | `DataLocation::of`                          |
| `attention`                                      | «¿Qué necesita mi atención?»: incidencias activas y evidencia por urgencia, cobertura (vigente, caducada, nunca observada), qué no está supervisado y el estado global | nuevo, atado a `attentionItems` (ver abajo) |
| `list_systems`                                   | Aplicaciones y recursos con lifecycle, estado de evidencia, comprobación y número de incidencias activas                                                               | `inventory::list`                           |
| `get_system`                                     | Detalle de una aplicación o recurso: componentes y recursos, observaciones, incidencias, eventos y comprobación, con el aviso de ventana de historial                  | `inventory::list`                           |
| `history`                                        | Observaciones o eventos anteriores, de 200 en 200, anclados como en la ventana                                                                                         | `older_observations`, `older_events`        |
| `save_application`                               | Alta o edición de una aplicación con sus componentes                                                                                                                   | `save_application`                          |
| `save_resource`                                  | Alta o edición de un recurso                                                                                                                                           | `save_resource`                             |
| `record_observation`                             | Observación con resultado, resumen y vigencia                                                                                                                          | `record_observation`                        |
| `open_incident`, `update_incident`               | Abrir una incidencia o cambiar su estado                                                                                                                               | `open_incident`, `update_incident_status`   |
| `record_event`                                   | Despliegue, respaldo, mantenimiento o nota                                                                                                                             | `record_event`                              |
| `save_check`, `set_check_paused`, `remove_check` | Configurar, pausar, reanudar o quitar una comprobación HTTP(S) y su supervisión                                                                                        | `http_check::*`                             |
| `run_check`                                      | Ejecutar una comprobación ahora y devolver su evidencia                                                                                                                | `http_check::run_http_check`                |
| `delete_application`, `delete_resource`          | Borrar lo que no tiene historial (lo demás se rechaza y se explica)                                                                                                    | `delete_application`, `delete_resource`     |
| `backup`                                         | Back up now, con la misma verificación                                                                                                                                 | `backup::back_up`                           |

Las ediciones de la puerta son **parciales**: un campo que no se manda se conserva, y la lista de
componentes solo se reemplaza si se manda. La ventana reemplaza la aplicación entera porque tiene el
formulario completo delante; un agente que solo quiere cambiar la versión no debe poder borrar los
componentes por omisión. Leer lo guardado y escribir el resultado ocurre en una sola transacción,
así que nada se pisa a medias con un guardado de la ventana.

Cada respuesta de escritura devuelve lo que quedó guardado. Cada lectura incluye los mismos avisos
que la UI: desde cuándo los resultados automáticos están completos y qué no está observado. Una
lista vacía o un sistema sin evidencia nunca se describe como sano.

## Procedencia

- **Registros.** Observaciones, incidencias y eventos escritos por la puerta llevan `source =
'agent'` y, en una columna `agent`, el nombre que el cliente declaró en `initialize`
  (`clientInfo.name`, por ejemplo `claude-code`). Ese nombre lo **declara** el cliente y no se
  verifica, y la UI lo presenta así: «Agent · claude-code». Una observación del agente tiene la
  misma semántica de vigencia que una manual, y ninguna vista la presenta como medida por DevHQ.
- **Configuración.** Aplicaciones, recursos, componentes y comprobaciones no tienen `source`
  porque no son evidencia. Lo que la puerta cambia en ellos queda en una tabla nueva, `agent_log`:
  cuándo, qué cliente, qué herramienta, qué sistema (tipo, ID y nombre en ese momento) y un
  resumen («Created application «Tienda» with 3 components»). Nunca se modifica ni se borra. No
  tiene claves foráneas, así que no impide borrar un sistema sin historial, y su entrada sobrevive
  al borrado. Aparece en Activity, global y de cada sistema, marcada como del agente.
- **Comprobaciones lanzadas por la puerta.** La evidencia la produce DevHQ, así que su observación es
  `source = 'http'`, idéntica a la de Run check; quién la pidió queda en `agent_log`.

## Comprobaciones con la ventana abierta

Hoy la supervisión garantiza que una comprobación nunca corre dos veces a la vez **dentro de un
proceso**. La puerta es otro proceso y corre la suya por su cuenta. Coordinarlos exigiría que la app
escuchara peticiones de otros procesos, que es justo lo que no queremos. Así que, si el agente pide
una comprobación en el mismo instante en que la supervisión la está corriendo, quedan dos
observaciones honestas; ninguna se pierde ni se inventa. El README lo dirá así.

Para que la ventana abierta no muestre datos viejos, la app vigila `PRAGMA data_version` en una
conexión propia cada 2 s mientras está abierta. Cuando otra conexión confirma algo, la supervisión
relee su configuración (así el ritmo continúa desde la ejecución que lanzó el agente y una pausa
pedida por él surte efecto) y la ventana relee el inventario sin estado de carga, como ya hace tras
una ejecución supervisada. Una recarga cuesta unos 8 ms. Con la ventana cerrada no hay nada que
vigilar.

Que la ventana esté abierta lo sabe la puerta por un lock del sistema que la app toma al arrancar
sobre un archivo junto a la base (`File::try_lock`) y que el sistema suelta si la app muere. `status`
lo informa. El estado en vivo de la supervisión (próxima ejecución, último error de DevHQ) solo
existe en ese proceso, y la puerta dice que no lo conoce en vez de suponerlo.

## La atención, calculada en Rust y atada a la de la ventana

Hoy el orden de atención, la cobertura y el estado global se calculan en TypeScript
(`src/domain/evidence.ts`). La puerta los necesita en Rust. Para que las dos copias nunca discrepen,
se atan como el contrato IPC: fixtures en `contract/attention/` con un inventario, un instante y el
resultado esperado. Vitest comprueba `attentionItems`, `evidenceCoverage` y `workspaceStatus` contra
ellos, y Rust su versión. Cambiar la regla en un lado sin el otro rompe un test.

## Coste

Cero mientras no la usas: no hay proceso. Durante una sesión, cada llamada abre una conexión como un
comando de la ventana y sigue los mismos índices, así que su coste depende de lo que lee y no del
historial acumulado. La vigilancia de `data_version` en la app es una consulta trivial cada 2 s, solo
con la ventana abierta.

## Esquema

- v7 (hecha, ver el README): `observations`, `incidents` y `events` admiten `source = 'agent'` con
  su columna `agent`, y los índices parciales cubren los registros manuales y de agentes.
- v8 (hecha): `agent_access`, el modo de la puerta (`off` sin fila).
- v9, con las herramientas de escritura: `agent_log`, solo de inserción, con índices por sistema y
  por tiempo para que Activity lo lea por índice como el resto de la historia. Lleva su test de
  actualización que compara volcados fila por fila y tests de plan de consulta para `agent_log`.

## Pruebas

- Unitarias en Rust de cada herramienta contra una base sembrada: acceso Off y solo lectura que
  rechazan sin escribir, ediciones parciales, rechazo de escritura cuando el sistema guarda el
  archivo en otra ruta, procedencia en cada registro y en `agent_log`.
- Protocolo: el binario real en `--mcp`, manejado por stdio desde un test con una base temporal
  (handshake, `tools/list`, errores JSON-RPC, una herramienta por tipo).
- Fixtures de atención en ambos lados.
- `npm run e2e:native`: con la ventana abierta, la puerta da de alta una aplicación y la ventana la
  muestra sin recargar a mano.

## Plan por commits

1. Hecho: esquema v7 y procedencia `agent` visible en la UI.
2. Hecho: atención en Rust atada a la de TypeScript por fixtures.
3. Hecho: modo `--mcp` con `status` y las herramientas de lectura.
4. Herramientas de escritura y `agent_log` (esquema v9), y el modo de acceso (esquema v8, hecho).
5. La app nota los cambios de otros procesos (`data_version`) y el lock de «ventana abierta».
6. Settings: la sección de la puerta y el comando para conectarla; README y recorrido nativo.

Todo se construye y se prueba en Linux. En Windows queda por comprobar, en la laptop o en el
workflow de Windows: la ruta de datos real, que un ejecutable con subsistema `windows` atiende bien
stdio cuando lo lanza Claude Code, y el comportamiento lanzado desde una app MSIX.

## Decisiones tomadas

- La puerta puede hacer todo lo que se hace desde la ventana, borrar incluido (el cliente pide
  confirmación en cada llamada y Rust sigue rechazando borrar lo que tiene historial).
- Lo que hace el agente aparece mezclado con lo tuyo en Activity, global y de cada sistema, con una
  etiqueta breve de procedencia: es la línea de comunicación clara entre los dos, no una vista
  aparte.
- El enfermero (supervisión con la ventana cerrada) no será un agente: si llega, lo hará DevHQ
  mismo, sin gastar uso de ningún modelo.
