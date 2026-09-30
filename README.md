# DevHQ

Centro de operaciones **nativo de escritorio** para un ecosistema personal de software.
Su prioridad es responder **«¿Qué necesita mi atención?»**: observar aplicaciones,
recordar eventos, relacionar infraestructura y verificar resultados de automatizaciones.

> Si una función solo ahorra escribir un comando sencillo, probablemente no pertenece
> a DevHQ. Si observa, recuerda, relaciona, detecta problemas o automatiza un proceso
> completo, sí puede pertenecer.

DevHQ no reemplaza VS Code, una terminal, GitHub Desktop, Docker Desktop, Codex ni
Claude Code. Conserva inventario local, la evidencia que registras a mano y la de
comprobaciones HTTP(S) que lanzas tú. **No supervisa en segundo plano ni opera sistemas.**

## Qué incluye

- Overview centrado en atención, incidencias, actividad y contexto operativo.
- Applications con búsqueda, filtros, identidad visual y detalle con seis tabs.
  Solo Overview tiene contenido completo; las restantes explican su futura función.
- Infrastructure compartida con enlaces a las aplicaciones que dependen de ella,
  incidencias del recurso y enlaces desde los componentes de cada aplicación.
- Incidents con severidad, estado, contexto desplegable y filtros; Activity global.
- En Mi inventario: observaciones, incidencias y eventos manuales persistentes, con
  procedencia, hora y vigencia visibles.
- Una comprobación HTTP(S) de solo lectura por aplicación o recurso, que se ejecuta solo
  a petición y registra una observación automática con su evidencia técnica.
- Previews de Automations (Trigger → Condition → Action → Verification), Security
  y Settings informativos.

El modo **Demo** utiliza un escenario local fijo del **24/09/2026, 16:40 Europe/Madrid**.
Los tiempos relativos se calculan frente a ese instante, no frente al reloj real.
Los estados unknown/no observado no implican salud ni seguridad. Lifecycle y health
son independientes. Una incidencia de un recurso también aparece en las aplicaciones
que lo utilizan; su actividad también aparece en el contexto de esas aplicaciones.
La copia de ejemplo ocurre a las 08:40 y se verifica a las 08:45; los resultados
de automatización son parte del escenario, no ejecuciones de DevHQ. Los logos y
thumbnails pueden sustituirse mediante campos del modelo. En Tauri se abre por defecto
**Mi inventario**, que usa SQLite; la demo se elige explícitamente en la barra lateral.
La preview web solo muestra la demo porque no dispone de IPC nativo. Los mocks nunca
se importan a SQLite. En el inventario real, aplicaciones, componentes y recursos
aparecen como **no observados**; lifecycle no expresa health.

Desde Mi inventario se crean y editan recursos y aplicaciones. Cada aplicación
contiene componentes que pueden apuntar a un recurso existente. Al guardar una
aplicación se reemplaza su lista de componentes de forma atómica: quitar un
componente también quita su vínculo. La eliminación de aplicaciones o recursos
completos queda fuera de este incremento. Un recurso inexistente se rechaza antes
de escribir la aplicación. Un fallo de SQLite/IPC muestra un error y permite reintentar.

### Observaciones, incidencias y eventos

Todo registro real apunta por ID a **una** aplicación o **un** recurso y declara su
origen (`source`), siempre visible:

| `source` | Etiqueta               | Quién lo produce                                   |
| -------- | ---------------------- | -------------------------------------------------- |
| `manual` | Manual entry           | Tú. Incidencias y eventos registrados a mano       |
| `http`   | Automated · HTTP check | DevHQ, al ejecutar la comprobación que tú lanzaste |

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

### Comprobación HTTP(S) a petición

Cada aplicación o recurso admite **como máximo un** endpoint: URL http/https, estado
esperado (200–599, nunca 3xx porque los redirects se siguen), tiempo máximo (1–10 s) y
vigencia del resultado (1 min–30 días). **Run check** lo ejecuta una vez; no hay
programación, reintentos, supervisión en segundo plano ni credenciales.

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
  Si algún día se borra un sujeto, su configuración se va con él (`CASCADE`), pero su
  evidencia sigue impidiendo el borrado (`RESTRICT`).
- La URL se guarda en texto plano: no pongas secretos en ella. Se rechazan URLs con
  usuario o contraseña.

## Ejecutar

Requisitos: Node.js 24 LTS, npm, Rust estable y los
[prerrequisitos de Tauri 2](https://v2.tauri.app/start/prerequisites/).
En Windows: Visual Studio Build Tools con C++ y Windows SDK, además de WebView2. Las
comprobaciones HTTP usan el TLS del sistema: SChannel en Windows, Security.framework en
macOS y OpenSSL (con sus cabeceras de desarrollo) en Linux. Las pruebas de red usan un
servidor en `127.0.0.1` y el dominio reservado `.invalid`; no contactan servidores externos.

```sh
npm ci
npm run tauri dev          # escritorio nativo
npm run dev               # preview web: http://127.0.0.1:1420
npm run check             # ESLint, tests, TypeScript, Vite y cargo check
cargo test --manifest-path src-tauri/Cargo.toml --lib # persistencia y migraciones
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
npm run tauri build       # ejecutable e instalador NSIS para Windows
```

Si DevHQ no termina tras cerrar su ventana, antes de matar el proceso ejecuta
`powershell -NoProfile -File scripts/dump-stacks.ps1 -ProcessId <pid>`: vuelca la pila de
cada hilo con `dbghelp.dll` de Windows y el PDB de release, sin instalar nada.

El instalador se genera en `src-tauri/target/release/bundle/nsis/`. El target de
empaquetado está configurado para Windows; otros sistemas necesitan sus prerrequisitos
y seleccionar un target de bundle apropiado. Los lockfiles de npm y Cargo se versionan.

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
`httpChecks`), `save_application`, `save_resource`, `record_observation`, `open_incident`,
`update_incident_status`, `record_event`, `save_http_check`, `remove_http_check` y
`run_http_check` (asíncrono). Abre `inventory.sqlite3` en el directorio de datos de la
aplicación que proporciona Tauri; las migraciones transaccionales v1→v4 usan
`PRAGMA user_version` y una versión más nueva se rechaza en vez de adivinarse. La v4
reconstruye `observations` y `events` (SQLite no cambia un CHECK con ALTER) conservando
filas, IDs, `rowid` (desempate de orden), claves foráneas, índices y los vínculos
evento → observación; se ejecuta con las claves foráneas desactivadas y solo se confirma
si `PRAGMA foreign_key_check` no encuentra relaciones rotas. Los
registros usan dos columnas con clave foránea (`application_id`/`resource_id`) y un CHECK
que exige exactamente una; las referencias de historial son `ON DELETE RESTRICT`, así que
nada puede borrar un sujeto y dejar evidencia huérfana (no existe aún ningún caso de uso de
borrado). Las referencias inexistentes se rechazan antes de escribir y dentro de la misma
transacción. Los tiempos se guardan en milisegundos UTC y viajan como RFC 3339; no se
aceptan momentos futuros. La UI invoca esos casos de uso con contratos tipados. Se conserva la barra nativa de ventana. La CSP limita recursos y las
capabilities no conceden permisos adicionales. No se implementan autenticación,
cuentas, cloud, sync, telemetría, scanners, automation engine, Runner ni background service.

## Evolución prevista

1. Hecho: una comprobación HTTP(S) a petición con su propio `source`, sin cambiar la
   semántica de vigencia.
2. Después, supervisión cancelable, automatizaciones con resultado e historial, y
   seguridad con almacén nativo de credenciales y evidencias contextualizadas.

Estos son límites conceptuales, **no módulos vacíos ni integraciones implementadas**.
La dirección es UI → IPC/casos de uso → dominio. SQLite conserva contexto; las credenciales no deben
vivir en componentes React, localStorage ni texto plano en SQLite.

Las pruebas cubren navegación, tabs/teclado, filtros, rutas desconocidas, relaciones
compartidas, cronología de la demo, salud desconocida, persistencia tras reapertura,
rechazo de referencias inválidas sin escrituras parciales, restricciones del esquema,
migraciones v1/v2→v3 y v3→v4 (incluida la negativa a confirmar relaciones rotas),
vigencia y sustitución de observaciones de ambos orígenes, orden de atención, errores de
carga visibles y recuperables, comprobaciones contra un servidor local (estado esperado,
500, redirect, bucle de redirects, tiempo agotado, conexión rechazada, dominio `.invalid`
y fallo TLS), configuraciones inválidas sin escrituras parciales, errores de DevHQ que
no producen observación y la distinción de origen en cada vista real.
