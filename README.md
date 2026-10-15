# DevHQ

Centro de operaciones **nativo de escritorio** para un ecosistema personal de software.
Su prioridad es responder **«¿Qué necesita mi atención?»**: observar aplicaciones,
recordar eventos, relacionar infraestructura y verificar resultados de automatizaciones.

> Si una función solo ahorra escribir un comando sencillo, probablemente no pertenece
> a DevHQ. Si observa, recuerda, relaciona, detecta problemas o automatiza un proceso
> completo, sí puede pertenecer.

DevHQ no reemplaza VS Code, una terminal, GitHub Desktop, Docker Desktop, Codex ni
Claude Code. Conserva inventario local, la evidencia que registras a mano, la de
comprobaciones HTTP(S) que lanzas tú o que supervisa **solo mientras su ventana está
abierta**, y lo que lee, cuando se lo pides, en las copias locales de esta estación de trabajo,
sin ejecutar nada de lo que nombran. **No funciona en segundo plano ni opera sistemas.**

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
- En Mi inventario, un registro de cambios de la configuración: cada alta, edición o borrado de
  aplicaciones, recursos, componentes, referencias y comprobaciones, tuyo o de un agente, con quién,
  cuándo y cada campo antes y después, en Activity junto a lo demás.
- En Mi inventario, las referencias de cada aplicación y recurso: dónde está su repositorio, sus
  copias locales en cada estación de trabajo, su documentación, sus logs y su panel, para copiarlas.
- En Mi inventario, las copias locales de esta estación de trabajo, leídas cuando pulsas Read o
  Read all on this computer, sin ejecutar nada de lo que nombran: cada copia con su última lectura,
  sus lecturas anteriores bajo demanda, un panel Local copies en Overview y la última actividad de
  cada aplicación. Settings › This computer dice qué estación de trabajo es este equipo.
- Una comprobación HTTP(S) de solo lectura por aplicación o recurso, que se ejecuta a
  petición o, si activas su supervisión, cada 5–60 min mientras DevHQ está abierto, y
  registra una observación automática con su evidencia técnica.
- Un panel de Supervision en Overview: qué comprueba DevHQ por su cuenta, cuándo toca la
  siguiente ejecución, qué falló dentro de DevHQ y qué no está supervisado.
- Previews de Automations (Trigger → Condition → Action → Verification) y Security
  informativos. En Mi inventario, Settings muestra la ruta del archivo SQLite, cómo
  respaldarlo y restaurarlo, y **Back up now**, que lo copia y verifica con DevHQ abierto.
- Un inventario de un esquema anterior se actualiza solo cuando lo pides, con **Back up and
  upgrade**, que antes lo respalda y verifica la copia (ver
  [Actualizar un inventario antiguo](#actualizar-un-inventario-antiguo)).

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
componente también quita su vínculo. Las [referencias](#referencias) de una aplicación o un
recurso se guardan igual, con él. Desde su formulario de edición, y tras confirmarlo,
se puede borrar una aplicación o un recurso **sin historial**: con sus componentes, sus
referencias y su comprobación HTTP. Lo que tiene observaciones, incidencias o eventos se conserva
(una aplicación puede pasar a Archived), y un recurso en el que corre algún componente, o que
guarda rutas de otros sistemas, también, hasta desenlazarlo; la UI explica el motivo y Rust lo vuelve a comprobar, dentro de la
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
impone con CHECK. Lo que DevHQ lee en una copia local no es ninguno de estos registros: es una
lectura, con su propio `source` (`read`), que nunca dice nada de la salud (ver
[Copias locales](#copias-locales)). Una observación automática sigue exactamente la misma semántica de
vigencia, sustitución y atención que una manual. Un registro `agent` guarda además, en
`agent`, el nombre que el cliente del agente declaró (por ejemplo `claude-code`); DevHQ no
puede verificarlo y la etiqueta lo dice al pasar el cursor. Se presenta mezclado con lo tuyo en
cada vista, con su propia etiqueta, y sigue la misma semántica que un registro manual: es algo
reportado, no medido por DevHQ. Una incidencia conserva quién la abrió; cada cambio de estado es
un evento con la procedencia de quien lo hizo. Solo la [puerta para agentes](#puerta-para-agentes)
escribe registros `agent`.

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

### Registro de cambios

Las observaciones, incidencias y eventos ya son historia: nunca se reescriben y dicen quién los
registró. La configuración no: aplicaciones, recursos, componentes, referencias y comprobaciones
HTTP(S) guardan solo cómo están ahora, y editarlas pisa el valor anterior. Su historia es el **registro de
cambios** (`change_log`), uno solo para ti y para los agentes, con el autor como un dato más: tú en
la ventana (`manual`, «Manual entry») o un agente por la [puerta](#puerta-para-agentes) (`agent`, con
el nombre que declaró su cliente, «Agent · _nombre_»).

- Cada caso de uso que cambia la configuración escribe su entrada **en la misma transacción** que el
  cambio: dar de alta, editar o borrar una aplicación o un recurso (con sus componentes y sus
  referencias), y
  configurar, cambiar, pausar, reanudar o quitar una comprobación. Nunca hay un cambio sin su
  entrada ni una entrada sin su cambio. Un rechazo no deja nada, y guardar sin cambiar nada, pausar
  lo que ya estaba pausado o reanudar lo que ya estaba activo no es un cambio y tampoco.
- Cada entrada guarda un resumen para leer («Updated application «Tienda»: version «1.3.0» →
  «1.4.0»») y, campo por campo, el valor anterior y el posterior, con los nombres de campo que usa
  la ventana (`version`, `components`, `references`, `url`…): un alta, los valores con que nació; un borrado,
  todos los que tenía. Así el registro dice cómo llegó algo a su estado actual, no solo quién lo
  tocó.
- Ejecutar una comprobación no cambia la configuración, y su observación es evidencia de DevHQ
  (`http`), que no dice quién la pidió: solo la puerta deja una entrada (`check_run`) cuando la pide
  un agente, para que una petición que un agente hizo salir de tu computadora siempre lleve su
  nombre.
- Las entradas nunca se modifican ni se borran. No tienen claves foráneas, así que no impiden borrar
  un sistema sin historial y le sobreviven con el nombre que tenía: Activity las sigue mostrando,
  con «no longer in the inventory» en vez de un enlace que no lleva a nada.
- Activity (global y de cada sistema) y la actividad reciente de Overview las muestran **mezcladas
  con los demás registros** en orden de tiempo, como entradas de tipo Change con la etiqueta de su
  autor. Crecen solo cuando alguien cambia algo, como los registros manuales, así que se cargan
  siempre completas.
- Hasta la v10 tus cambios no dejaban rastro (los de agentes sí, desde la v9). Si la actualización
  encuentra un inventario que ya tenía aplicaciones o recursos, anota ese momento y Activity dice que
  los cambios están completos desde entonces y que los anteriores hechos en la ventana no dejaron
  rastro; un inventario vacío, o uno nuevo, no tiene nada anterior que echar en falta y no lo dice.

### Referencias

Una aplicación o un recurso dice dónde está en el mundo con sus **referencias**: su repositorio, sus
copias locales en cada estación de trabajo, su documentación, sus logs y su panel. Responden «¿dónde
están sus logs, su documentación, su repositorio?» y, para un agente que trabaja en una carpeta,
«¿qué sistema es este?». Son inventario que declaras tú o un agente, no evidencia: no llevan
`source`, su rastro es el [registro de cambios](#registro-de-cambios) y DevHQ no las abre ni
comprueba que existan o respondan, salvo la copia local que le pidas leer (ver
[Copias locales](#copias-locales)). La ventana solo las copia.

| Tipo            | En la ventana | Destino                                                                   |
| --------------- | ------------- | ------------------------------------------------------------------------- |
| `repository`    | Repository    | URL http(s): la ubicación canónica del código, no la copia de una máquina |
| `local_copy`    | Local copy    | Ruta absoluta en un recurso `workstation`                                 |
| `documentation` | Documentation | URL http(s) o ruta absoluta                                               |
| `logs`          | Logs          | URL http(s) o ruta absoluta                                               |
| `dashboard`     | Dashboard     | URL http(s)                                                               |

- El destino es una URL http(s) o una ruta absoluta, y se guarda tal como se escribió, sin espacios
  alrededor. La URL sigue las reglas de la comprobación HTTP(S): con host, sin usuario ni contraseña
  y de 2048 caracteres como mucho; se guarda en texto plano, así que no lleva secretos. La ruta se
  acepta en la forma de cualquier sistema, sea cual sea el que ejecuta DevHQ, porque puede ser de
  otra máquina: `C:\…` (o `C:/…`), `\\servidor\recurso\…` o `/…`, de 2048 caracteres como mucho.
  Nada más es un destino: ni rutas relativas, ni `~`, ni caracteres de control, ni remotos SSH. El
  remoto real de una copia Git es evidencia aparte, que DevHQ lee cuando se lo pides (ver
  [Copias locales](#copias-locales)), y los localizadores de unidades de systemd o de contenedores
  llegarán con una integración que los necesite.
- **Toda ruta dice en qué máquina está** (`machine_id`, un recurso), porque el mismo proyecto vive en
  una carpeta distinta en cada equipo, y una URL no está en ninguna. La máquina de una copia local
  es una estación de trabajo. Un recurso puede ser la máquina de sus propias rutas, como los logs de
  un servidor en `/var/log/…`; en la ventana se elige como «This resource» una vez guardado.
- La **etiqueta**, opcional y de hasta 120 caracteres, es un nombre para personas que distingue dos
  referencias del mismo tipo (dos paneles, la documentación de la API y la de usuario). Nunca es un
  localizador: no se copia ni se interpreta.
- La misma referencia (tipo, destino y máquina) dos veces en un sistema se rechaza.
- Se editan **dentro del formulario de su aplicación o recurso**, como los componentes: la lista que
  se guarda es la completa, conserva el ID de cada referencia que sigue en ella y reemplaza la
  anterior en la misma transacción que el resto del sistema, después de validarla entera; un rechazo
  no cambia nada. El registro de cambios las cubre con un campo más, `references`, en las mismas
  entradas de alta, edición y borrado.
- **Monorepo**: varias aplicaciones, o un recurso, pueden declarar el mismo repositorio sin ninguna
  entidad nueva. Cada lectura del inventario deriva en Rust qué repositorios declara más de un
  sistema (`sharedRepositories`), por igualdad exacta de la URL, sin normalizarla, y la ventana y
  la puerta dicen «Same repository as …». No se guarda.
- Borrar una aplicación o un recurso borra sus referencias (`CASCADE`). Borrar un recurso que es la
  máquina de rutas de **otros** sistemas se rechaza y dice de cuáles son, como un recurso en el que
  corre un componente: quitarlas cambiaría la configuración de otro sistema sin su entrada en el
  registro de cambios, y el esquema también lo impide (`RESTRICT`). Sus propias rutas no lo impiden:
  el borrado las quita primero, porque SQLite aplica el `RESTRICT` de la máquina en el acto, antes
  de la cascada del sujeto. Por la misma razón, un recurso que guarda copias locales de otros
  sistemas no puede dejar de ser estación de trabajo, y el rechazo dice de qué sistemas son.
- En la ventana, el Overview de cada aplicación y la tarjeta de cada recurso en Infrastructure
  muestran sus referencias, con la máquina de cada ruta y un botón **Copy** para el destino, como la
  ruta en Settings. Sin referencias dicen «No references yet»: no es atención ni hueco de cobertura,
  porque no dice nada de cómo está el sistema.

### Copias locales

Una referencia `local_copy` dice dónde está, en una estación de trabajo, una copia de trabajo del
repositorio de un sistema. Cuando se lo pides, DevHQ la **lee** y guarda lo que encontró como una
**lectura**: evidencia propia, nunca salud. Responde «¿qué lleva meses sin actividad?» y «¿qué
trabajo existe solo en una máquina?», y guarda el commit de HEAD para el día en que DevHQ sepa qué
está desplegado.

**Observar no da poder a lo observado.** Un repositorio puede nombrar programas en su config, en sus
atributos y en sus hooks, y el programa `git` los ejecuta: drivers de filtro, fsmonitor, programas de
firma, helpers de remotos. Por eso DevHQ nunca ejecuta `git` y trata la ruta declarada, y lo que
contiene, como datos no confiables, los haya declarado quien los haya declarado. Lee con las APIs de
lectura de libgit2 (`git2`, compilada con el proyecto y sin ningún transporte de red), que no lanzan
programas ni escriben, y cada copia en un proceso propio, `devhq --read-copy`, que antes de leer se
impide a sí mismo arrancar programas: en Windows entra en un Job Object que solo admite un proceso,
él mismo; en Linux se pone un filtro seccomp que responde `EPERM` a `execve` y `execveat`, también
como root. Donde no sabe hacerlo (otros sistemas), no lee y lo dice. El proceso tiene 30 s; si no
termina, DevHQ lo mata, y lo que devuelve se valida como dato no confiable antes de guardarlo. Así un
cuelgue o un fallo de libgit2 ante datos hostiles termina con el lector, nunca con la ventana ni con
la puerta.

Una **batería de repositorios hostiles**, en las pruebas de Rust de Linux y de Windows, nombra un
programa marcador en cada clave de config que Git ejecuta (una lista que una prueba compara con la de
`git help --config`), en `.gitattributes`, en los hooks y en `core.hooksPath`, en un `include.path`, un
`includeIf` y un `config.worktree`, en un submódulo, un repositorio anidado y un worktree enlazado, en
un commit firmado y en remotos `ext::`, y pone copias del marcador al principio del `PATH` como `git`,
`sh`, `gpg`, `ssh` o `git-lfs`. Leer, con el lector encerrado y sin encerrar, no debe dejar ninguna
marca ni cambiar o crear un solo archivo (contenido y fecha de modificación), y debe leer bien cada
dato; la misma copia leída por el `git status` instalado sí deja marcas. La batería encontró que la API
de reflog de libgit2 crea un log vacío cuando una ref no lo tiene, así que DevHQ lee esos logs como
archivos. Un lector o una biblioteca nuevos, y cada versión nueva de libgit2, la pasan antes.

**Lo que no se garantiza.** Si unos datos hostiles lograran ejecutar código dentro del lector, ese
código tendría tus permisos: podría leer y escribir donde tú puedes, conectarse a la red o pedirle a
otro proceso (WMI o el Programador de tareas en Windows; systemd o D-Bus en Linux) que arranque un
programa; ni el Job Object ni seccomp lo impiden. Lo que sí impiden es que el lector, libgit2 incluido,
lance un programa por sí mismo. Nada del sistema impide que el lector escriba: lo garantizan libgit2 y
la batería. `git2` se mantiene al día y solo se llaman las APIs que la batería cubre.

Qué dice una lectura:

| Dato                                        | Qué es                                                                                                                                                                                                                                                                                                                                          |
| ------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `outcome`, `detail`                         | `read`; `missing` (nada en esa ruta); `not_a_copy` (una carpeta donde no empieza ningún repositorio, aunque esté dentro de uno, porque DevHQ no busca en las carpetas de arriba; un repositorio bare; un archivo); `unreadable` (el sistema niega el acceso, o la copia es de otro usuario y Git tampoco la usaría). `detail` dice por qué      |
| `branch`, `head`                            | La rama de HEAD (nula si está suelto) y su commit (nulo si aún no hay commits)                                                                                                                                                                                                                                                                  |
| `lastCommitAt`                              | La fecha de commit más nueva entre HEAD y las ramas locales, según los propios commits                                                                                                                                                                                                                                                          |
| `unpublishedOnHead`, `unpublishedElsewhere` | Commits de HEAD, y de las demás ramas locales, que ninguna rama remota de la copia contiene; hasta 1000 («1000+»). Separados porque las ramas viejas de un PR fusionado con squash cuentan como sin publicar                                                                                                                                    |
| `stashes`                                   | Cuántos stashes guarda: las entradas del log de `refs/stash` que Git lee, como en `git stash list`; ninguna si no hay log                                                                                                                                                                                                                       |
| `syncedAt`                                  | La última vez que la copia habló con un remoto según sus archivos (`FETCH_HEAD` y la entrada más nueva del log de cada rama remota): DevHQ nunca hace fetch, así que «sin publicar» es según lo que la copia sabía entonces                                                                                                                     |
| `remotes`                                   | Nombre y URL de cada remoto (hasta 20), la URL solo en formas conocidas y sin credenciales: http(s) sin usuario, contraseña, query ni fragmento; `ssh://` sin contraseña; `usuario@host:ruta`; `git://` y `file://` sin usuario ni contraseña; y rutas absolutas. Cualquier otra, como `ext::`, que es una línea de comandos, se guarda sin URL |
| `operation`                                 | Un merge, rebase, cherry-pick, revert, bisect o am a medias                                                                                                                                                                                                                                                                                     |
| `changed`, `untracked`, `unverifiable`      | Archivos seguidos con cambios, en el índice o en el working tree (conflictos incluidos); entradas nuevas no ignoradas, una carpeta nueva una vez, como en `git status`; y archivos que DevHQ no puede verificar: parecen cambiados bajo un driver de filtro que la config define y DevHQ no ejecuta (LFS, por ejemplo), o no se pudieron leer   |
| `worktreeUnknown`                           | En lugar de esos tres números, por qué no se conoce el working tree: un índice que libgit2 no sabe leer, o el plazo                                                                                                                                                                                                                             |

Nunca autores, correos, mensajes ni nombres de archivo, tamaños, listas de ramas o tags, ni el estado de
los submódulos (cada uno es su propia copia, si se declara). La URL de un remoto se guarda en texto
plano, ya limpia. Un formato que libgit2 aún no conoce (`extensions.refStorage = reftable`,
`objectFormat = sha256`) es un fallo de DevHQ, que se muestra y no se guarda; uno que solo afecta al
índice deja la lectura con el working tree desconocido. Leer mientras un `git` pesado escribe en la
copia puede fallar una vez: se vuelve a pedir. libgit2 toma lo que el sistema no le deja mirar por
algo que no está, así que una carpeta sin permiso le parece una carpeta sin repositorio; antes de
decir `not_a_copy`, DevHQ vuelve a mirar lo mismo que miró libgit2 (`.git`, el archivo que nombra otra
carpeta, `HEAD`, `commondir`, `objects` y `refs`, y la propia carpeta como repositorio bare) y, si el
sistema le negó alguna de esas miradas, la lectura es `unreadable` con el motivo del sistema.

Los logs de las ramas remotas y del stash se leen como los lee Git: una línea que Git no aceptaría,
como una que aún se está escribiendo, no es una entrada. Un número que parece exacto nunca sale de un
log leído a medias. La entrada más nueva es la última, así que de cada log de rama remota solo se lee
el final (1 MiB), y `syncedAt` es exacto por largo que sea el log; si en ese final no hay ninguna
entrada, el momento es desconocido. Contar stashes exige el log entero, así que uno de más de 32 MiB
(más de cien mil stashes) no se cuenta. Ninguno de los dos casos se rellena con una cuenta menor, un
momento anterior o un cero, y como la lectura no tiene dónde decir «desconocido» para esos datos, es un
fallo de DevHQ, que se muestra y no se guarda. Un log que el sistema no deja leer deja la lectura
`unreadable`, con el motivo del sistema, en vez de sin stashes o sin contacto. Un log que no existe no
dice nada: sin él, Git tampoco lista ningún stash.

- **Evidencia sin vigencia.** Cada lectura se guarda en `copy_readings`, inmutable, con `source =
read` («Automated · Local read»), el lector y su versión (`reader`, «libgit2 1.9.7»), el momento
  observado y el registrado, y el agente que la pidió por la puerta, o nada si la pediste tú. No es
  healthy ni unhealthy, no entra en la atención ni en la cobertura de salud, no escribe eventos ni sale
  en Activity, y no caduca, porque nunca afirmó seguir siendo cierta: dice lo que la copia tenía cuando
  se leyó, y su momento se muestra siempre. Nada la presenta como actual.
- **Copia lo que leyó**: la referencia, la máquina y la ruta, sin claves foráneas, como una observación
  HTTP copia su URL. Editar o quitar la referencia nunca reescribe una lectura, y una máquina se puede
  borrar cuando ya no guarda rutas. Si la referencia desaparece o cambia mientras se lee, no se guarda
  nada, como una comprobación quitada en vuelo.
- **Es historia**: un sistema con lecturas no se puede borrar, como uno con observaciones. Una aplicación
  que ya no se usa pasa a Archived: sus copias dejan de leerse al pedir todas, y sus lecturas se
  conservan. Una copia cuya carpeta ya no existe se quita de las referencias sin tocar sus lecturas.
- **Lo derivado**, en Rust y en cada lectura del inventario (`localCopies`): cada copia declarada con su
  última lectura, por un índice; si esa lectura se hizo en otra ruta o máquina de las que declara hoy la
  referencia; si la copia guardaba trabajo que solo existía allí (commits sin publicar, stashes,
  cambios, entradas nuevas o no verificables, una operación a medias); y si alguno de sus remotos
  apunta al repositorio que declara su sistema (`matches`), ninguno (`differs`), o el sistema no declara
  ninguno (`undeclared`). Esta comparación normaliza los dos lados (host en minúsculas sin usuario ni
  puerto, ruta sin `/` a los lados, sin `.git` y en minúsculas, para `https://`, `ssh://`, `git://` y
  `usuario@host:ruta`), porque el remoto real suele ser `git@github.com:me/shop.git` y el repositorio
  declarado `https://github.com/me/shop`; un host cuyo SSH y web difieren, como Azure DevOps, sale
  `differs`. `sharedRepositories` sigue comparando por igualdad exacta. Las lecturas anteriores de un
  sistema se cargan de 200 en 200 (`older_readings`), por el índice de su sistema.

**Esta estación de trabajo.** DevHQ solo lee copias de la estación de trabajo en la que corre, y eso no
lo dice el inventario, que puede viajar a otro equipo: lo guarda `this-workstation.json`, en la carpeta
local de DevHQ (`%LOCALAPPDATA%\com.devhq.desktop` en Windows, que no viaja con un perfil móvil;
en Linux, junto al inventario, en `~/.local/share/com.devhq.desktop.dev`; una build de desarrollo
usa `com.devhq.desktop.dev`, ver [Arquitectura](#arquitectura)). Back up now no lo copia. Guarda, por
equipo, el recurso elegido en cada inventario y cuándo; cada equipo se distingue por una huella, el
SHA-256 de una etiqueta de DevHQ y del identificador del sistema (`MachineGuid` en Windows,
`/etc/machine-id` en Linux), que nunca se guarda en bruto. Así, copiar el home a otro equipo, o
compartirlo entre dos, no hace que uno se crea el otro: el otro equipo simplemente aún no ha elegido. La
elección vale si el recurso existe en el inventario abierto y es una estación de trabajo; si no, DevHQ
dice por qué no sabe qué equipo es (sin elegir, ya no está en este inventario, ya no es estación de
trabajo, el sistema no da identificador o el archivo no se puede leer) y no lee ninguna copia. Solo la
ventana la elige (`set_this_workstation`); la puerta solo la lee. No es inventario, así que no va al
registro de cambios; cada lectura guarda en qué máquina se hizo. Las VMs clonadas sin sysprep comparten
`MachineGuid`, y reinstalar el sistema obliga a elegirla otra vez.

**Solo a petición.** `read_copies` lee las copias que se nombran o, sin nombrar ninguna, todas las de
esta estación salvo las de aplicaciones archivadas, una tras otra: nunca al abrir DevHQ, nunca con un
temporizador, nunca con la ventana cerrada. Las copias de otra máquina las lee DevHQ en esa máquina. DevHQ
no busca copias: lee solo las rutas que el inventario declara.

**En la ventana.** Settings › This computer dice qué estación de trabajo es este equipo, desde cuándo y
en qué archivo, o por qué DevHQ no lo sabe, y deja elegir entre los recursos de tipo Workstation (o
ninguno); si no hay ninguno, manda a Infrastructure a crearlo, y si el sistema no da identificador o el
archivo no se puede leer, no ofrece elegir. La ventana pregunta qué estación es este equipo aparte de
`list_inventory`, después de cada lectura del inventario, porque que la elección valga depende de él; si
esa pregunta falla, el inventario se sigue mostrando y el fallo se dice donde importa. En las
referencias de cada aplicación y recurso, cada copia local muestra su última lectura en pasado: el
resultado (Read, Not found, Not a copy, Not readable), el momento, la máquina, la etiqueta
«Automated · Local read» con el agente que la pidió, si fue uno, y lo que encontró (HEAD, último
commit, commits sin publicar con «1000+», stashes, working tree o por qué se desconoce, una operación a
medias, el último contacto con un remoto y los remotos, los que no se guardaron con su nombre y sin
URL). Dice si la lectura se hizo en otra ruta o máquina de las declaradas hoy y si sus remotos parecen
el repositorio declarado. Read aparece solo en las copias de esta estación y se desactiva mientras esa
copia se lee; en las de otra máquina, la ventana dice que las lee DevHQ allí, y sin estación conocida,
por qué no lee ninguna y dónde elegirla. Un fallo de DevHQ al leer se muestra y no guarda nada. «Show
every reading» carga, solo al abrirlo, todas las lecturas del sistema, las más nuevas primero y de
200 en 200, en una línea cada una, y dice cuándo están todas y qué lecturas son de rutas que el
sistema ya no declara; se ofrece en los sistemas que declaran alguna copia local, y las de un sistema
que ya no declara ninguna siguen guardadas y la puerta las da (`history`). Overview tiene un panel
**Local copies**, junto a Supervision y fuera de «Needs attention»: cuántas copias tiene esta estación y
Read all on this computer, que las lee en serie y dice cuántas leyó y cuáles no pudo; el trabajo que
solo existía en una copia, las copias que no se encontraron donde se declaran, las de esta estación
nunca leídas y cuántas de otros equipos nunca se leyeron, las mismas listas que da la puerta y sin las
aplicaciones archivadas. Applications, en cada tarjeta, y el Overview de cada aplicación dicen su
última actividad: el commit más nuevo entre las últimas lecturas de sus copias, con la máquina y el
momento de esa lectura, y cuántas copias nunca se leyeron; si ninguna se leyó, que la actividad se
desconoce. Nunca dicen «inactiva».

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
  lanzara quien la lanzara: un Run check también reinicia la espera, y también un `run_check` de
  la [puerta para agentes](#puerta-para-agentes), que la supervisión nota en unos 2 s. Las ejecuciones
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
  curso, última y siguiente ejecución y último error de DevHQ, y dos contadores desde que abrió:
  cuántas ejecuciones registraron evidencia (`recorded`) y cuántas veces notó que otra conexión
  confirmó algo (`changes`, ver [Puerta para agentes](#puerta-para-agentes)). Cuando uno crece,
  la ventana relee el inventario sin estado de carga, así que los formularios abiertos y los
  registros desplegados se conservan. Si dos lecturas se cruzan, aplica solo la que empezó
  después: nunca vuelve a un estado anterior al que ya muestra.

### Respaldo

**Back up now**, en Settings › Database file, copia el inventario con DevHQ abierto. Es el
comando `backup_inventory`, sin argumentos, que corre fuera del hilo principal y de uno en uno:
mientras se hace un respaldo, la ventana desactiva el botón y Rust rechaza otro. No necesita
plugins ni permisos, no cambia el esquema y no se registra en la línea de tiempo: no es evidencia
de ningún sistema.

- Solo respalda un inventario del esquema actual, tal como está: nunca lo actualiza para poder
  copiarlo. Uno más antiguo, más nuevo o que no es un inventario lo rechaza antes de escribir nada,
  sin crear siquiera la carpeta `backups/`. El respaldo que precede a una actualización lo hace la
  propia actualización, con la versión de origen en su nombre y un manifiesto (ver
  [Actualizar un inventario antiguo](#actualizar-un-inventario-antiguo)).

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
- **Hasta dónde llega «a disco».** El contenido de la copia se fuerza a disco antes de nombrarla
  (`fsync` en Linux, `FlushFileBuffers` en Windows) y, si el sistema responde con un error, el
  respaldo falla y el `.partial` se borra. El nombre se confirma aparte: en Linux, ya nombrada la
  copia, DevHQ fuerza a disco también la carpeta `backups/` y la carpeta de datos que la contiene, y
  si el sistema responde con un error el respaldo falla con `backup_error`, aunque la copia, completa
  y verificada, se queda con su nombre, porque no consta que ese nombre sobreviva a una caída del
  sistema o a un corte de luz. Windows no documenta ninguna forma de forzar a disco los nombres de una
  carpeta: NTFS anota cada nombre en su diario al darlo y escribe el diario a disco por su cuenta, así
  que allí DevHQ confirma el contenido, no el nombre. En macOS no se ha probado. Y todo vale lo que
  valga lo que confirman el sistema y el disco: uno que da por escrito lo que aún tiene en su caché, o
  que falla, puede perder lo confirmado; la verificación lee la copia tal como la devuelve el sistema,
  no la relee del disco.
- Si algo falla antes de nombrar la copia, borra el `.partial`; si lo único que falla es confirmar el
  nombre, la copia ya nombrada se queda y el mensaje lo dice. En los dos casos la ventana muestra un
  error recuperable, **The backup failed.** con el motivo, y el inventario no cambia. Al terminar,
  muestra la hora, el tamaño, la verificación y cuántas filas de cada tabla tiene la copia.
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

### Actualizar un inventario antiguo

Ninguna apertura actualiza un inventario (ver [Arquitectura](#arquitectura)). Uno de un esquema
anterior solo lo actualiza la persona, desde la ventana, con **Back up and upgrade**. Nada más lo
hace: ni una consulta, ni la supervisión, ni un respaldo, ni la [puerta para agentes](#puerta-para-agentes),
que no tiene ninguna herramienta para ello. Lo implementa `src-tauri/src/upgrade.rs`.

- **Ver antes de decidir.** Una ventana que arranca sobre un inventario anterior a la v12 no lo sirve:
  cada página de Mi inventario muestra **Inventory needs a schema upgrade** con lo que es, leído sin
  cambiar ningún archivo y como lee la puerta, con una conexión que no puede escribir
  (`inspect_upgrade`). Dice la ruta, la versión que declara y la de este DevHQ, y cuántas filas tiene
  cada tabla. Solo ofrece actualizarlo si el archivo tiene exactamente la estructura que las
  migraciones dan a su versión (cada tabla con sus columnas y declaraciones, cada índice, y ninguna
  tabla, índice, vista o trigger más), si `PRAGMA integrity_check` responde `ok` y si
  `PRAGMA foreign_key_check` no encuentra relaciones rotas. Si no, dice por qué, no ofrece nada y no
  cambia nada: DevHQ no actualiza lo que no puede garantizar, sea un archivo dañado, uno con una
  estructura que DevHQ no produjo o el de otro programa.
- **Consentimiento.** Hay un solo botón, **Back up and upgrade**, y ninguna forma de actualizar sin
  respaldo. Lo que la persona acepta es lo que vio: la versión y una huella del contenido (SHA-256 de
  cada fila de cada tabla, con su `rowid` y cada valor con su tipo), que la ventana devuelve con
  `upgrade_inventory`.
- **Orden.** `upgrade_inventory` hace, por este orden:
  1. Mira otra vez los archivos sin cambiarlos, y solo abre su conexión si ninguno necesita recuperación.
  2. Toma el lock de escritura de SQLite (`BEGIN IMMEDIATE`): ninguna otra conexión, de DevHQ o de otro
     programa, puede confirmar nada hasta que termine. Dentro comprueba que el inventario es el que la
     persona vio (misma versión, misma huella) y, de nuevo, su estructura, su integridad y sus
     relaciones. Si algo no coincide lo rechaza sin escribir nada, con `upgrade_changed` (ya no es lo
     que se vio) o `upgrade_refused`.
  3. Lo copia desde una segunda conexión, de solo lectura, con `VACUUM INTO`, que lee el último estado
     confirmado, el que el lock mantiene, en `backups/inventory-<UTC>-v<N>.sqlite3.partial`, donde `N`
     es su versión. Abre la copia aparte y comprueba que tiene la versión `N` y su estructura, que
     `integrity_check` responde `ok`, que no tiene relaciones rotas y que cada tabla tiene las mismas
     filas, con el mismo `rowid` y los mismos valores, que el inventario bajo el lock. La fuerza a disco
     y le da su nombre final con un enlace que nunca reemplaza un archivo, como Back up now. Después
     escribe su manifiesto, `inventory-<UTC>-v<N>.sqlite3.json`, también a través de un `.partial`,
     forzado a disco y nombrado sin reemplazar nada, y por último, en Linux, fuerza a disco los dos
     nombres (ver hasta dónde llega «a disco» en [Respaldo](#respaldo)). Sin un respaldo verificado
     y su manifiesto no se migra nada, ni cuando el sistema responde que no pudo hacer alguno de esos
     pasos.
  4. Aplica las migraciones en la misma transacción.
  5. Antes de confirmar, comprueba el resultado: la versión 12 y su estructura, `integrity_check`,
     `foreign_key_check`, y que cada fila de cada tabla sigue ahí con su `rowid` y los mismos valores
     en las columnas que ya tenía, comparados valor a valor y tipo a tipo. En cada columna nueva, cada
     fila debe tener el valor por defecto que declara la columna (`NULL` si no declara ninguno), que es
     lo que dan las migraciones que añaden columnas o reconstruyen tablas: así, por ejemplo, ninguna
     comprobación sale supervisada de una actualización. Las tablas nuevas deben estar vacías (la puerta
     queda cerrada si el inventario no tenía `agent_access`), con dos excepciones, ambas de la v10:
     `agent_log` pasa entera a `change_log`, cada fila como cambio de un agente sin campos
     estructurados, y `change_log_start` guarda el momento de la actualización si el inventario ya tenía
     aplicaciones o recursos. Contar filas no bastaría: un valor cambiado, o del mismo contenido pero
     de otro tipo, con el mismo número de filas, también hace fallar la actualización.
  6. Confirma. La ventana arranca entonces sobre el inventario como si DevHQ acabara de abrirse (lo
     pasa a WAL) y empieza la supervisión, sin reiniciar.
- **Lo que se ve.** Mientras tanto, la ventana muestra cada paso cuando Rust lo alcanza (comprobación
  con el lock, respaldo, respaldo verificado con su ruta, actualización y verificación), por un
  `Channel`. Al terminar, encima del inventario ya servido, **Inventory upgraded** dice las dos
  versiones, cuántas filas y tablas se comprobaron, y la ruta del respaldo, la de su manifiesto y su
  SHA-256, hasta que lo descartas.
- **El manifiesto** es un JSON pequeño junto al respaldo: qué es (`format`), el nombre del respaldo
  (`backup`), su `sha256` y su tamaño (`sizeBytes`), cuándo se tomó (`takenAt`), la ruta del inventario
  del que es copia (`inventory`), su versión (`schemaVersion`), para qué se hizo (`madeBefore`), qué
  DevHQ lo hizo (`devhq`), qué dijo `integrityCheck` y cuántas filas tiene cada tabla (`rows`). Se
  escribe una vez y nunca se reescribe, y sirve para comprobar sin DevHQ que el respaldo sigue siendo
  el archivo que se verificó.
- **Si algo falla**, lo dice y la ventana vuelve a mirar el inventario: si sigue pudiendo, ofrece
  intentarlo otra vez.
  - Si no se puede crear la carpeta o el `.partial` (permisos, un nombre ocupado), si `VACUUM INTO`
    falla (disco lleno), si la copia no pasa su verificación, si no se puede escribir el manifiesto o
    si, en Linux, el sistema no confirma que los nombres llegaron al disco, la respuesta es
    `backup_error`: el `.partial` se borra, el inventario no cambia y no se migra nada. Si lo que falla
    es el manifiesto, la copia, ya verificada y nombrada, se queda; si es la confirmación de los
    nombres, se quedan la copia y su manifiesto; el mensaje lo dice.
  - Si una migración falla, si el resultado no pasa sus comprobaciones o si el commit falla, la
    respuesta es `upgrade_failed`: SQLite deshace la transacción, el inventario sigue en su versión con
    lo que tenía y el respaldo verificado se queda; el mensaje dice dónde está.
  - Si DevHQ termina antes de publicar el respaldo, el inventario no se tocó (nada lo escribe antes) y
    puede quedar un `.partial`, que Settings lista como incompleto.
  - Si termina después de publicarlo y antes del commit (lo matan, se va la luz), lo confirmado sigue
    siendo el estado anterior, y ese respaldo lo tiene entero, porque con el lock nadie pudo confirmar
    nada después. Pero el archivo queda con un journal o un log que hay que recuperar, y DevHQ no
    recupera por su cuenta un inventario de un esquema anterior: al arrancar lo rechaza sin tocar sus
    archivos (ver «Un inventario antiguo que necesita recuperación», abajo).
  - Si termina después del commit, el inventario ya es v12: al arrancar, la ventana lo recupera como
    cualquier inventario actual y lo sirve.
  - Dos actualizaciones a la vez, desde dos ventanas, las ordena el lock. La segunda, al obtenerlo,
    encuentra el inventario ya en la v12, no hace otro respaldo y la ventana lo sirve (**Inventory
    already upgraded**); si espera más de 5 s se rechaza con `upgrade_refused` y basta con reintentar.
- **Los respaldos** van a la misma carpeta `backups/` que los de Back up now, en el mismo disco que el
  inventario: un disco que falla se los lleva, y desinstalar DevHQ con «Delete the application data»
  los borra. Copia el respaldo y su manifiesto a otro sitio. DevHQ no los borra ni los reescribe nunca.
- **Restaurar un respaldo** se hace a mano; DevHQ no restaura desde la ventana.
  1. Cierra DevHQ.
  2. Comprueba el respaldo: su manifiesto debe nombrar este inventario (`inventory`), su SHA-256 debe
     ser el del manifiesto (en PowerShell, `Get-FileHash -Algorithm SHA256 <archivo>`; en Linux,
     `sha256sum <archivo>`) y, con la CLI de SQLite, `PRAGMA integrity_check` debe responder `ok` y
     `PRAGMA user_version` la versión del manifiesto. El nombre del archivo no basta.
  3. Aparta juntos, a una carpeta nueva y vacía fuera de la carpeta de datos, sin borrar ni
     reemplazar nada, `inventory.sqlite3` y, si están, `inventory.sqlite3-wal`,
     `inventory.sqlite3-shm` e `inventory.sqlite3-journal`.
  4. Copia el respaldo a la carpeta de datos con el nombre `inventory.sqlite3`, dejando el original en
     `backups/`.
  5. Abre DevHQ: verá un inventario de esa versión y lo describirá antes de ofrecer actualizarlo otra
     vez.

  Lo que se escribió después del respaldo no está en él.

- **Un inventario antiguo que necesita recuperación** no se recupera, no se respalda y no se
  actualiza. Lo deja así una actualización que terminó después de publicar su respaldo y antes del
  commit, pero también un DevHQ anterior u otro programa que terminó a mitad de una escritura, o un
  `-journal`, `-wal` o `-shm` de origen desconocido, y nada en los archivos dice cuál fue.
  - **Lo comprobado.** Al arrancar, la ventana copia los archivos a una carpeta temporal, deja que
    SQLite recupere la copia, mira su versión y la borra. Si es de un esquema anterior, Mi inventario
    muestra **Inventory needs a schema upgrade** sin ofrecer Back up and upgrade (`inspect_upgrade`
    responde `needs_recovery`) y dice lo que sigue; Back up now y la puerta lo rechazan sin escribir
    nada. Ni la ventana ni la puerta recuperan, mueven, borran ni reemplazan ninguno de sus archivos.
  - **Lo desconocido.** Qué tienen exactamente hasta recuperarlos, y si algún respaldo tiene lo mismo:
    lo último que se confirmó puede estar solo en ellos. Sin su `-journal` el archivo puede quedar
    escrito a medias, y sin su `-wal` se pierden cambios confirmados. Solo de una actualización
    interrumpida antes del commit se sabe que su respaldo tiene todo lo confirmado, pero ni su nombre
    ni su manifiesto dicen que los archivos vengan de esa actualización: tras una actualización que
    falló, por ejemplo, un DevHQ anterior pudo seguir escribiendo en el inventario.
  - **Qué hacer**, a mano:
    1. Cierra DevHQ. No abras el inventario con otro programa, porque SQLite lo recuperaría en su
       sitio, ni con un DevHQ anterior, que además podría actualizarlo sin respaldo.
    2. Copia `inventory.sqlite3` y, si están, `inventory.sqlite3-journal`, `-wal` y `-shm`, juntos y con
       esos nombres, a una carpeta fuera de la carpeta de datos. No borres ni renombres ninguno.
    3. Busca en `backups/` un respaldo cuyo manifiesto nombre este inventario (`inventory`); si una
       actualización se interrumpió, el suyo es el de su hora (`takenAt`). Compruébalo como en el
       paso 2 de «Restaurar un respaldo».
    4. Comprueba que tiene lo mismo que los archivos: pon otra copia de ellos, con sus nombres, en otra
       carpeta y ábrela con la CLI de SQLite, que recupera esa copia y no el original. En esa carpeta,
       `sqlite3 inventory.sqlite3 ".output recuperado.sql" .dump`, y para el respaldo,
       `sqlite3 <respaldo> ".output respaldo.sql" .dump`, deben dar los mismos bytes
       (`fc.exe /b recuperado.sql respaldo.sql` en Windows, `cmp recuperado.sql respaldo.sql` en Linux),
       y `PRAGMA user_version`, que `.dump` no incluye, la misma versión en los dos. El archivo lo
       escribe la propia CLI, con `.output`, y no una redirección (`>`): Windows PowerShell 5.1
       decodifica lo que escribe un programa y lo vuelve a codificar, y con la consola en UTF-8 (página
       de códigos 65001) dos volcados que difieren en bytes que no son UTF-8 válido salen iguales.
    5. Solo si coinciden, restáuralo como dice «Restaurar un respaldo»: no se pierde nada confirmado.
       Si no coinciden, o no hay respaldo, no pongas ninguno en su lugar y conserva los archivos como
       están: esta versión de DevHQ no ofrece otro camino.

- **Volver a una versión anterior de DevHQ.** Un DevHQ anterior no lee la v12: volver es restaurar el
  respaldo previo a la actualización, de su versión, como arriba, y lo escrito después se pierde. Los
  DevHQ anteriores a que ninguna apertura actualizara un inventario actualizan sin respaldo lo que
  abren, y convierten a WAL uno más nuevo antes de rechazarlo: no abras con ellos un inventario que
  quieras conservar sin tener una copia.

La preview muestra el panel con `?scenario=outdated` y rechaza la actualización como toda escritura.

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
queda nada corriendo ni guarda credenciales. Abre el mismo `inventory.sqlite3` que la ventana del
mismo ejecutable, en el directorio de datos que Tauri le da (`%APPDATA%\com.devhq.desktop` para el
ejecutable instalado en Windows; un test lo compara con el que resuelve Tauri), o el que se le indique
con `--database <ruta>`. Registrada con un ejecutable que no empaquetó el instalador, como el de
`target/`, abre los datos de desarrollo (ver [Arquitectura](#arquitectura)), y `status` lo dice en
sus notas. Si ahí no hay inventario, lo
dice y no crea uno vacío donde tu DevHQ no lo encontraría.

**La puerta abre el inventario tal como lo encuentra, y al leerlo no escribe en él ni cambia lo que
contiene.** Ninguna consulta crea el archivo ni su carpeta, lo actualiza a otro esquema, cambia su modo
de journal, lo recupera ni escribe en él ni en los archivos que SQLite guarda a su lado (`-wal`,
`-shm`, `-journal`), y solo crea uno de ellos, un `-wal` vacío, en la carrera que se describe al final
de esta explicación. No basta con una conexión que no pueda escribir: SQLite cambia esos archivos con
solo abrir la base en algunos estados, sea cual sea el permiso de la conexión. La primera conexión a
un log (`-wal`) que dejó un proceso que ya terminó reconstruye su índice (`-shm`); la última en
cerrarse vuelca el log en el archivo y lo borra; la que encuentra un journal de rollback de una
escritura interrumpida lo deshace; y cualquiera, salvo una `immutable`, crea el log de un archivo en
WAL que no lo tiene. Por eso la puerta mira primero los archivos sin SQLite y solo abre una conexión
donde SQLite no va a escribir. La conexión es de solo lectura y, cuando una conexión mantiene abierto
el log (la ventana, por ejemplo), la puerta lo lee a través de su índice sin escribir en ninguno de los
dos (`readonly_shm`): ve lo que esa conexión confirmó aunque aún no esté en el archivo. Un archivo en
WAL sin log lo contiene todo y se lee como un archivo que no cambia (`immutable`), porque cualquier
otra conexión crearía el log y su índice. Como nada impide que un escritor lo abra mientras tanto, esa
lectura solo cuenta si después el archivo tiene los mismos bytes y no apareció ni desapareció un log,
un índice o un journal; si no, la puerta vuelve a mirar. Un archivo con journal de rollback y sin log,
como cada respaldo de DevHQ, se lee con los bloqueos de siempre.

Queda una carrera que este mecanismo no cierra. Si la última conexión al inventario (la ventana al
cerrarse, por ejemplo) se cierra en el instante entre que la puerta mira los archivos y SQLite abre la
base, esa conexión vuelca el log en el archivo y lo borra, y la conexión de la puerta, que entonces no
es `immutable`, crea un `-wal` vacío al encontrar un archivo en WAL sin log. Ese `-wal` no tiene
nada: el archivo del inventario y sus filas quedan como estaban, la siguiente conexión lo usa como
cualquier log vacío y una consulta posterior lo encuentra vacío y lee el archivo entero. Puede pasar en
cualquier apertura de la puerta, también en `status` y en los rechazos que se describen abajo, que
tampoco cambian nada más. Es la única excepción y no autoriza ninguna otra: ninguna consulta MCP
actualiza el inventario a otro esquema, cambia su modo de journal ni añade, cambia o borra una fila.

Un inventario que habría que recuperar antes de poder leerlo se rechaza con `needs_recovery`, sin leer
ni cambiar nada, y `status` dice qué encontró. Puede ser un journal de rollback que dejó una escritura
que no terminó: mientras no se deshace, el archivo puede tener esa escritura a medias. Puede
ser un log con cambios que ninguna conexión mantiene abierto: quien los escribió terminó sin volcarlos,
porque se colgó, lo mataron o se cerró mientras otra conexión leía. O puede ser un log sin su índice.
La puerta no los recupera ni lee esos cambios en memoria: la ventana de DevHQ lo recupera al arrancar,
si es un inventario del esquema actual (lo averigua antes en una copia), y desde entonces la puerta lo
lee como cualquier otro. Para saber si una conexión mantiene
abierto un log, la puerta pregunta lo mismo que SQLite y de la misma manera: si alguien tiene el
bloqueo del byte 128 del índice, que cada conexión conserva mientras vive y que el sistema suelta
cuando su proceso termina, termine como termine.

La puerta solo sirve un inventario en el esquema de este DevHQ (v12) y con su estructura. Lo demás lo
rechaza sin leer ni cambiar nada, también con la puerta cerrada: un inventario más antiguo con
`schema_outdated` (este DevHQ nunca actualiza un inventario al abrirlo; solo la persona lo actualiza,
desde la ventana, con [Back up and upgrade](#actualizar-un-inventario-antiguo), y la puerta no tiene
herramienta para ello), uno más nuevo con `schema_newer`, un archivo SQLite sin las tablas de DevHQ (vacío o de otro
programa) con `not_an_inventory`, uno que habría que recuperar con `needs_recovery`, y lo que no se puede
leer como SQLite (otro tipo de archivo, una carpeta, un archivo dañado o bloqueado) con
`database_error`. Que un archivo declare la versión 12 no basta. La puerta compara sus tablas, columna
por columna, y sus índices, por nombre, con el esquema que crean las migraciones de este DevHQ; si algo
falta o difiere lo rechaza con `schema_mismatch` y dice qué. Las tablas e índices que el esquema no tiene
no los juzga. Esa comparación no es una comprobación de integridad: la puerta nunca lee cada página y
cada fila, y `status` dice que la integridad no se comprobó.

Cada escritura de un agente pregunta primero por el inventario y el acceso con esa misma lectura. Así,
una que el acceso rechaza, o sobre un archivo que no es un inventario actual, no abre nada que pueda
escribir. Después abre el archivo sin crearlo, actualizarlo ni cambiar su modo de journal, y dentro de
su transacción, con el lock de escritura de SQLite, vuelve a comprobar que es un inventario del
esquema actual y que la persona sigue dejando escribir a los agentes: eso decide, en los cambios de
configuración, los registros, `run_check` y cada lectura que guarda `read_copies`. Si entre la lectura
y la escritura otro proceso sustituye el archivo por un inventario más antiguo, la escritura se
rechaza con `schema_outdated` y el archivo queda como estaba; si lo borra, no se vuelve a crear; y si
cierras la puerta en Settings en ese instante, se rechaza con `access_denied`. `run_check` guarda la
observación y la entrada que dice qué agente la pidió en la misma transacción: las dos o ninguna.
Al confirmar, cada escritura de un agente vuelca en el archivo lo que tiene el log y lo deja vacío
antes de cerrar su conexión (`wal_checkpoint(TRUNCATE)`, que espera hasta 5 s a quien aún lo esté
leyendo). SQLite solo vuelca el log cuando se cierra la última conexión, y la de una puerta que lee
no puede hacerlo: sin ese paso, con la ventana cerrada, una escritura que terminaba mientras la puerta
de otra sesión leía dejaba sus cambios en un log que nadie mantenía, y desde entonces toda puerta
rechazaba el inventario con `needs_recovery` hasta que la ventana lo recuperaba al arrancar, sin
perder nada confirmado. El volcado no decide nada, porque la escritura ya está confirmada: si no
termina (alguien siguió leyendo el log más de 5 s), no es un error y el log queda como lo dejaría
SQLite, para la siguiente escritura de un agente o para la ventana. Las escrituras de la ventana no
lo hacen: mientras está abierta, su propia conexión mantiene el log, y lo vuelca al cerrarse.
`backup` no puede copiar dentro de una transacción (`VACUUM INTO` no lo admite), así que comprueba el
esquema y el acceso en la conexión que copia justo antes de copiar, y la verificación de la copia
rechaza otra versión. Lo que no cierra es la recuperación: si en el instante entre la lectura y la
escritura otro proceso deja el archivo a medias, SQLite lo recupera al abrirlo, como en cualquier
escritura, antes de que DevHQ vea qué es. Lo que la lectura sin cambios no cubre está en
[docs/puerta-para-agentes.md](docs/puerta-para-agentes.md).

Hasta dónde llega un agente lo decides tú en Settings → **Agent gate**: **Off** (por defecto, también
tras actualizar), **Read only** o **Read and write**. La elección se guarda en el inventario
(`agent_access`), porque la ventana la cambia y la puerta, otro proceso, la lee en cada llamada: se
aplica de inmediato, también a una sesión ya abierta. No es una barrera de seguridad (el archivo es
tuyo y está en tu disco) sino de intención: ningún agente lee ni escribe en tu inventario sin que lo
hayas decidido. Con la puerta cerrada solo responde `status`, sin decir qué contiene el inventario,
para que el agente pueda explicarte dónde abrirla; las demás herramientas responden
`access_denied`. Settings también muestra el comando de registro con la ruta real del ejecutable,
para copiarlo; DevHQ nunca lo ejecuta ni toca la configuración de Claude Code. Para leer basta Read
only:

| Herramienta    | Qué responde                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| -------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `status`       | Qué archivo abrió, dónde lo guarda de verdad el sistema (`storedAt`, como Settings), qué es ese archivo (`schema`: la versión de esquema que declara, también en `schemaVersion`, o `null` si no es un inventario o no se pudo leer; la que soporta este DevHQ; su estado: `current`, `mismatch`, `outdated`, `newer`, `not_an_inventory`, `needs_recovery`, `unreadable` o `missing`, con qué significa; si tiene la estructura del esquema de este DevHQ, `structure`: `matches`, `differs` o, para otra versión, `not_checked`; y su integridad, `integrity`, que la puerta nunca comprueba: `not_checked`), si hay una ventana de DevHQ abierta sobre él (`windowOpen`), cuántos sistemas tiene, qué estación de trabajo es este equipo (`thisWorkstation`, o por qué no se sabe), qué no puede saber y, si su ejecutable no lo empaquetó el instalador, que sin `--database` abre datos de desarrollo. El acceso, los recuentos y la estación solo se dan de un inventario `current`: de cualquier otro son `null` |
| `attention`    | «¿Qué necesita mi atención?» con el mismo orden que Overview, la cobertura, lo nunca observado y lo que nada supervisa; el estado solo es `covered` si todo tiene evidencia vigente. Aparte, sin ordenar y diciendo que no es salud, `localCopies`: las copias cuya última lectura tenía trabajo que solo existía allí, las que no se encontraron y las de esta estación nunca leídas                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| `list_systems` | Cada aplicación y recurso con su estado de evidencia y última observación, incidencias activas, comprobación HTTP, relaciones y referencias, con la máquina de cada ruta y qué otros sistemas declaran el mismo repositorio (`sameRepositoryAs`): un agente encuentra así su sistema por su carpeta o su repositorio. Cada copia local dice si es de esta estación, su última lectura y lo derivado, y cada sistema el commit más nuevo que encontraron (`localActivity`)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| `get_system`   | Una aplicación o recurso (por ID, por nombre exacto sin distinguir mayúsculas, o por ambos si son del mismo sistema) con sus referencias, sus observaciones, incidencias y eventos cargados, y desde cuándo su historial automático está completo                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| `history`      | Los resultados automáticos anteriores de un sistema, o las lecturas de sus copias locales (`records: "readings"`), de 200 en 200, como la carga bajo demanda de la ventana                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              |

Con Read and write, el agente también hace lo que tú haces en la ventana, pasando por los mismos
casos de uso, con sus validaciones, límites y transacciones:

| Herramienta                                      | Qué hace                                                                                                                                              |
| ------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------- |
| `save_application`, `save_resource`              | Dar de alta (nombre y lifecycle, o nombre y tipo) o editar por ID, con sus componentes y sus referencias                                              |
| `delete_application`, `delete_resource`          | Borrar lo que no tiene historial; lo demás se rechaza con el mismo motivo que en la ventana                                                           |
| `record_observation`, `record_event`             | Registrar una observación (resultado, qué vio, vigencia) o un despliegue, respaldo, mantenimiento o nota                                              |
| `open_incident`, `update_incident`               | Abrir una incidencia o cambiar su estado con una nota                                                                                                 |
| `save_check`, `set_check_paused`, `remove_check` | Configurar la comprobación HTTP(S) de un sistema (200, 5 s y 60 min por defecto, como el formulario), pausar o reanudar su supervisión, o quitarla    |
| `run_check`                                      | Ejecutarla ahora, como Run check: la observación es evidencia de DevHQ (`http`)                                                                       |
| `backup`                                         | Back up now                                                                                                                                           |
| `read_copies`                                    | Leer ahora, como Read, una copia local (por su referencia), las de un sistema o todas las de esta estación: la lectura es evidencia de DevHQ (`read`) |

Una edición es **parcial**: lo que el agente no manda se conserva, para que cambiar la versión nunca
borre los componentes ni las referencias por omisión. La lista de componentes y la de referencias,
cuando se mandan, son la lista completa, como en el formulario; un componente apunta a su recurso, y
una ruta a su máquina, por ID o por nombre exacto, y un recurso puede nombrarse a sí mismo como
máquina de sus rutas desde el alta. Los registros que escribe llevan `source = agent` y su nombre. La
configuración (aplicaciones, recursos, componentes, referencias y comprobaciones) no tiene `source`: cada cambio que hace un agente queda en el
[registro de cambios](#registro-de-cambios) con su nombre, por los mismos casos de uso que la
ventana y **en la misma transacción** que el cambio, después de comprobar ahí, con el lock de
escritura de SQLite, que sigues permitiendo escribir. La respuesta incluye esa entrada (`change`), o
`null` si no cambió nada. `get_system` devuelve todas las de cada sistema, tuyas y de agentes, con
cada campo antes y después, y dice desde cuándo están completas (`changesCompleteSince`). Los
registros no se repiten en ese log: ya dicen quién los escribió. Ejecutar una comprobación deja en
el log qué agente la pidió. Un rechazo no cambia nada ni deja entrada. Las herramientas que sobrescriben o quitan configuración se
declaran destructivas (`destructiveHint`) y `run_check`, que sale de tu computadora, de mundo
abierto (`openWorldHint`), así que el cliente pide confirmación según cómo lo tengas configurado.

**Leer una copia no es configurar DevHQ.** `read_copies` exige Read and write porque guarda
evidencia, como `record_observation`: con Read only un agente ve las lecturas pero nunca provoca una.
No cambia aplicaciones, recursos, referencias ni comprobaciones, así que no deja entrada en el registro
de cambios; la propia lectura guarda qué agente la pidió, y la ventana y la puerta lo muestran, como
lleva su nombre una comprobación que un agente hizo salir de tu computadora. Se declara sin
`readOnlyHint` (escribe evidencia), sin `destructiveHint` (no cambia nada que exista) y sin
`openWorldHint` (nada sale del equipo). Lo que la hace posible sí deja rastro: declarar o mover una copia
local es editar las referencias de un sistema. Qué estación es este equipo solo lo elige la ventana. Con
Read and write, un agente puede declarar como copia local cualquier ruta absoluta de esta estación y hacer
que DevHQ la lea; DevHQ solo dirá si ahí hay una copia y los datos de una lectura, nunca nombres de
archivo, contenidos, autores ni mensajes, que un agente con terminal ya ve. Como el resto de la puerta,
los modos son de intención, no una barrera de seguridad.

Con la ventana de DevHQ abierta, lo que cambia un agente aparece solo, en unos 2 s. Mientras la
ventana vive, su proceso mantiene una conexión SQLite propia y cada 2 s le pregunta
`PRAGMA data_version`, que cambia exactamente cuando otra conexión confirmó algo: la puerta, otro
programa o los comandos de la propia ventana, que abren cada uno la suya. Es una consulta trivial que
no lee ninguna tabla; cerrar la ventana la termina junto con la supervisión y cierra esa conexión, y
con la ventana cerrada no existe. Cuando cambia, la supervisión relee su configuración (una
comprobación que el agente configuró, pausó, reanudó o quitó surte efecto, y una ejecución que pidió
reinicia la espera como Run check) y la ventana relee el inventario sin estado de carga, como tras
una ejecución supervisada. Leer no confirma nada, así que esa relectura no provoca otra; un cambio
hecho en la propia ventana, que ya la relee al guardar, provoca una relectura silenciosa más, de unos
8 ms. Si la conexión falla, se abre otra en la siguiente consulta y eso cuenta como cambio, porque no
se sabe qué pasó entretanto.

`status` dice si hay una ventana de DevHQ abierta sobre ese inventario (`windowOpen`), y con ello si
algo verá lo que escribe y supervisará las comprobaciones. Lo sabe por un lock del sistema: el proceso
de la ventana abre `inventory.sqlite3.window-lock`, junto al inventario, y lo bloquea en modo
compartido mientras vive; la puerta pide un bloqueo exclusivo sin esperar y, si se lo niegan, hay una
ventana. El sistema suelta el lock cuando el proceso termina, de cualquier forma (cerrado, caído o
matado), así que ningún archivo puede quedarse diciendo «abierta»; Windows lo hace según le permiten
sus recursos, y puede tardar un momento después de que el proceso desaparezca. Es compartido para que dos ventanas
sobre el mismo inventario lo tengan las dos. El archivo está vacío, DevHQ no lo borra y restaurar una
copia no lo necesita; sin él (ninguna ventana abrió ese inventario desde que DevHQ lo usa), la ventana
está cerrada, y la puerta nunca lo crea. Si el sistema no puede responder, `windowOpen` es `null` y
`status` lo dice.

Con una ventana abierta, una comprobación que el agente deja supervisada es de la ventana: la nota en
unos 2 s y la ejecuta enseguida si nunca corrió o su última ejecución tiene más de un intervalo. Por
eso `save_check` y `set_check_paused`, cuando la comprobación queda activa y hay una ventana abierta,
lo dicen en una nota (`note`), y la descripción de `run_check` pide no ejecutarla otra vez: en la
prueba con un Claude Code real, a quien se le pidió configurarla y ejecutarla, sin la nota quedaban dos
observaciones con medio segundo de diferencia. Si aun así un agente ejecuta una comprobación en el
mismo instante en que la supervisión de la ventana corre la misma, quedan dos observaciones honestas;
ninguna se pierde ni se inventa.

Las de lectura se declaran de solo lectura (`readOnlyHint`). Cada respuesta dice lo que la ventana diría: una
lista recortada lleva el aviso de su ventana de historial y nada sin evidencia vigente se presenta
como sano. La atención se calcula con la copia en Rust de las reglas de la ventana, atada a ellas por
`contract/attention/`. El estado en vivo de la supervisión (ejecución en curso, siguiente, último
error de DevHQ) solo existe en el proceso de la ventana, y `status` dice que la puerta no lo conoce.
Un fallo de un caso de uso vuelve como resultado con `isError` y el mismo mensaje que vería la
ventana; un método o una herramienta desconocidos, como error de JSON-RPC.

**Los argumentos se comprueban antes de abrir nada.** Cada herramienta publica en `tools/list` su
`inputSchema`, con el que un cliente, y el modelo detrás de él, construye sus llamadas, y la puerta
comprueba cada llamada contra ese mismo esquema antes de abrir el inventario o hacer nada: un
argumento que la herramienta no declara, también dentro de un componente o de una referencia
(`descripton`, `components[0].runOn`, un `type` en `delete_resource`), uno de otro tipo (`"id": 1`),
un valor fuera de su lista o de su rango, y uno obligatorio que falta. Los esquemas usan un
subconjunto pequeño de JSON Schema (`type`, `enum`, `properties`, `required`,
`additionalProperties: false`, `items`, `minimum` y `maximum`) y la puerta lo comprueba entero: un
test recorre todos los esquemas publicados y falla si alguno usa otra palabra clave, o deja un objeto
abierto o un entero sin límites, para que ningún esquema anuncie una regla que la puerta no aplica.
Como en JSON Schema, un número sin parte fraccionaria es un entero: `30.0` vale como `30`, y `30.5`
se rechaza. `null` solo vale donde el esquema lo admite (`version`, `runsOn`, `label` y `machine` de
una referencia, `supervisionIntervalMinutes`, y `before` en `history`), con el significado que da la
descripción; en cualquier otro campo se rechaza en vez de ignorarse. Las reglas entre argumentos que
ese subconjunto no expresa están en la descripción de cada herramienta y se comprueban igual, antes
de abrir nada: nombrar el sistema por `id` o por `name`; en `read_copies`, una referencia o un
sistema con su `type`, no ambos; `before` con observaciones y eventos; nombre y lifecycle, o nombre
y tipo, para dar de alta. Un `id` y un `name` juntos deben nombrar el mismo sistema, y si no, la
llamada se rechaza en vez de tomar el `id` y callar el `name`; esto se comprueba al buscar el
sistema, dentro de la transacción en las escrituras, porque necesita el inventario. Un rechazo es un
resultado con `isError` y el código `invalid_input` cuyo texto dice cada problema con su ruta, qué
se acepta ahí y, si un nombre parece mal escrito, cuál se quiso decir; no se hizo nada. Solo se
comprueban los argumentos de la herramienta: el resto de `params` (`_meta`, y lo que añadan
revisiones posteriores del protocolo) es del protocolo, y la puerta no lo rechaza.

En Windows, una puerta lanzada desde una app empaquetada (MSIX), como la app de escritorio de Claude,
ve `AppData` redirigido (ver [Arquitectura](#arquitectura)) y podría leer y escribir otro inventario,
o no encontrar ninguno: `status` lo dice con `storedAt`, y la puerta no se niega a escribir. Claude Code desde una terminal normal no está empaquetado. El workflow de
Windows instala DevHQ y habla con la puerta del ejecutable instalado como lo hace un cliente MCP:
comprueba que responde por stdin y stdout aunque no tenga consola, que abre el inventario de
`%APPDATA%`, que ve cerrada la ventana que acaba de cerrarse (Windows soltó su lock) y que termina al
cerrarse su entrada.

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

Todo ejecutable que no empaquetó el instalador, empezando por `npm run tauri dev`, guarda datos de
desarrollo en `com.devhq.desktop.dev`, aparte del inventario de la DevHQ instalada, y su ventana se
titula «DevHQ (development data)» (ver [Arquitectura](#arquitectura)).

`preview.html` existe solo en desarrollo: carga la ventana real sobre un proceso nativo
simulado con `@tauri-apps/api/mocks` y datos de ejemplo relativos al reloj, con un recurso
supervisado que tiene más resultados de los que lleva un snapshot. Los comandos de lectura
siguen las mismas reglas de ventana y paginación que Rust (lo prueba
`src/preview/backend.test.ts`), toda escritura se rechaza sin guardar nada y una franja propia
sobre la ventana dice que son datos de ejemplo; la ventana ocupa el resto, así que el aviso no
tapa nada al desplazarse. Sus copias locales cubren una leída en este equipo con trabajo sin
publicar, otra leída en otra estación con HEAD suelto, un rebase a medias, más de 1000 commits sin
publicar, el working tree desconocido y un remoto que no es el repositorio declarado, una nunca leída,
una movida desde su última lectura que entonces no se encontró, con 205 lecturas (más de una página),
una lectura `not_a_copy` en el historial y la copia de una aplicación archivada; Read y Read all se
rechazan como toda escritura. `?scenario=empty`, `?scenario=unavailable` y `?scenario=outdated`
muestran un inventario vacío, uno ilegible y uno de un esquema anterior, con el panel que ofrece
respaldarlo y actualizarlo; la preview rechaza la actualización como toda escritura (lo prueba
`e2e/upgrade.spec.ts`). El bundle de producción se construye desde `index.html` y no contiene nada de
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
a WAL, con un recurso, su observación y su evento y una aplicación con un componente en él. Mi
inventario debe decir que necesita actualizarse, qué tiene (5 filas en 7 tablas) y ofrecer Back up and
upgrade, y Back up now debe rechazarla, sin que la carpeta de datos gane ningún archivo ni el v6 cambie
un byte. Después pulsa Back up and upgrade: la ventana debe mostrar **Inventory upgraded** de la v6 a la
actual, `backups/` debe tener el respaldo `-v6` y su manifiesto, el respaldo debe tener, tabla a tabla y
fila a fila, lo mismo que el v6 original, versión 6 e integridad correcta, el manifiesto debe nombrar
su SHA-256 y el inventario, y la ventana, sin reiniciar, debe servir el inventario en WAL y mostrar el
recurso, su observación y la aplicación. Cierra la ventana, aparta el inventario actualizado y sus
respaldos, vuelve a arrancar DevHQ, que debe crear un inventario nuevo en WAL, y abre Mi inventario, comprueba la ruta que muestra Settings y «Copy path», crea un recurso y
una aplicación con su repositorio como referencia, registra una observación, supervisa una comprobación HTTP contra un servidor propio
en `127.0.0.1` (esa ejecución y la recarga de su evidencia solo llegan a la ventana por el
`Channel` de supervisión), la ejecuta a petición, lee el panel Supervision y pulsa Back up now: el
archivo debe estar en `backups/`, sin `.partial`, con `quick_check` correcto, la versión del
esquema y las filas del inventario, y la ventana debe listarlo sin ninguna copia incompleta (un
antivirus que aún retuviera el `.partial` borrado mientras lo analiza lo dejaría a la vista en la
lista). Luego hace de agente: elige Read and write en Settings → Agent gate, lanza el mismo
ejecutable como puerta (`--mcp --database` con la base del recorrido), le habla MCP como un cliente
que se llama `native-walk`, comprueba que `status` ve la ventana abierta y da de alta una aplicación
con un componente sobre el recurso. Sin tocar la ventana, Applications debe mostrarla sola, y Activity
su Change con la etiqueta «Agent · native-walk». En la copia del respaldo, el registro de cambios
debe tener, como tuyos, el alta del recurso, la de la aplicación y la comprobación, y ninguna marca de
inicio, porque el inventario es nuevo. Con la app sin el vigilante de `data_version`, ese
paso no ve la aplicación en 15 s y falla. Después crea una estación de trabajo, la elige en
Settings › This computer (la puerta debe ver la misma elección) y declara como copia local de la
aplicación una copia hostil que construye con el `git` instalado (`e2e/native/hostile-copy.ts`):
un commit en ningún remoto, un archivo cambiado, uno nuevo y uno cambiado bajo un driver de filtro, y
un programa marcador en `core.fsmonitor`, los drivers de filtro, `diff.external`, `credential.helper`,
`core.sshCommand`, `gpg.program` con `log.showSignature` y cuatro hooks. La lee con Read, con Read all
on this computer y con `read_copies` desde la puerta: la ventana debe mostrar lo que encontró (el
commit sin publicar, «1 changed file · 1 new entry · 1 file DevHQ could not verify», el remoto que
coincide con el repositorio declarado) y, sola, la lectura que pidió el agente; tras cada lectura,
ningún marcador puede haber corrido y ningún archivo de la copia puede haber cambiado, aparecido o
desaparecido (contenido, tamaño y fecha de modificación, `.git` incluido). Al final, `git status`
sobre la misma copia sí ejecuta el fsmonitor: la copia es hostil de verdad. Por último cierra la ventana como
lo hace su botón, nunca por WebDriver: con `WM_DELETE_WINDOW` en X11 (un script de Python que llama
a libX11, porque bajo Xvfb no hay gestor de ventanas que lo envíe) o con `WM_CLOSE` en Windows. El
proceso debe terminar, sin dejar `inventory.sqlite3-wal`, y el archivo debe seguir en WAL; la
puerta, que sigue abierta, debe ver la ventana cerrada en menos de 10 s (el recorrido anota cuánto
tardó) y terminar al cerrarse su entrada. En la base deben quedar las tres lecturas, en la estación elegida y
con la ruta declarada, y solo la última con el agente. El inventario actualizado al principio debe
estar en la versión actual, con integridad correcta y las filas del v6 original, y su respaldo debe
seguir teniendo exactamente las del v6.
Antes de hacer clic espera a que el elemento deje de moverse, porque el contenido que llega tarde
empuja la página, y guarda una captura por paso en `test-results/native/`. `DEVHQ_APP` lo apunta a
otro ejecutable.

Nunca toca un inventario real. Solo maneja un ejecutable que el instalador no empaquetó, que guarda
datos de desarrollo: antes de lanzar nada o mirar ninguna carpeta lee el archivo, sin ejecutarlo, y
exige la marca sin parchear exactamente una vez; sin ella (empaquetado, o un ejecutable cuya marca
no conoce) o repetida, se niega. Vitest lo comprueba (`e2e/native/app.test.ts`) con ejecutables
sintéticos y sustitutos que registran cada proceso y carpeta: ninguno se toca. En
Linux, todas las carpetas XDG apuntan a una carpeta temporal y, si no hay pantalla, arranca su
propio Xvfb. Windows resuelve sus carpetas con la API de carpetas conocidas, que ninguna variable
de entorno redirige: allí solo corre si aún no existen las carpetas `com.devhq.desktop.dev` en
`%APPDATA%` y `%LOCALAPPDATA%` (en un equipo con datos de desarrollo se niega) y al terminar borra
las que creó; las de la DevHQ instalada no las mira. Antes de abrir la ventana pregunta a la puerta
del ejecutable, sin `--database`, dónde guardará su inventario, y debe ser esa carpeta; al cerrarla
comprueba que se titula «DevHQ (development data)». Con la versión síncrona de `watch_http_monitor`, este recorrido falla
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
recorre sobre WebView2. Ese ejecutable no lo empaquetó el instalador: sus datos, sus respaldos y su
perfil de WebView2 deben estar en `com.devhq.desktop.dev`, nada en `com.devhq.desktop`, y su puerta,
lanzada como la lanza un cliente MCP, debe abrir ese inventario y decir que son datos de desarrollo.
Después lleva esos datos a `com.devhq.desktop`, como quien trae un inventario de otro equipo, instala
el instalador en silencio para el usuario actual, abre la app instalada sobre ese inventario (debe
mostrar su ventana titulada «DevHQ», arrancar WebView2 en `%LOCALAPPDATA%\com.devhq.desktop` y
terminar al cerrarla sin dejar `-wal`), comprueba que su puerta abre `%APPDATA%\com.devhq.desktop`
sin crear datos de desarrollo, y la desinstala en silencio: deben quedar el inventario y sus
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
crates y `DEVHQ_CHROMIUM`) y `CLAUDE.md` resume las reglas de trabajo del repositorio; `AGENTS.md`
remite a él a Codex y a los demás agentes que leen ese archivo.

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

Rust expone `list_inventory` (una lectura consistente de inventario con sus referencias, registros,
`httpChecks` y la última lectura de cada copia local), `data_location`, `backup_inventory`, `list_backups`, `save_application`, `save_resource`, `delete_application`,
`delete_resource`, `record_observation`, `older_observations`, `older_events`, `open_incident`,
`update_incident_status`, `record_event`, `save_http_check`, `set_http_check_paused`,
`remove_http_check`, `run_http_check`, `watch_http_monitor`, `this_workstation`,
`set_this_workstation`, `read_copies`, `older_readings`, `inspect_upgrade` y `upgrade_inventory`.
`read_copies` es asíncrono y espera a sus lectores en un hilo aparte, porque cada copia puede tardar
hasta su plazo; `inspect_upgrade` y `upgrade_inventory` también, porque leen el archivo entero, y se
turnan entre sí. Los comandos de
comprobaciones y de borrado son asíncronos y avisan a la supervisión antes de responder, así
que pausar, reanudar, quitar o borrar surten efecto de inmediato; `run_http_check` pasa por la supervisión para
no duplicar una ejecución en vuelo y `watch_http_monitor` abre el `Channel` de estado. Ese último también es
asíncrono porque lanza la tarea que alimenta el `Channel`: Tauri ejecuta un comando síncrono en el hilo
que recibe la petición (el principal), que no tiene runtime de Tokio, y lanzarla desde ahí provocaba un
pánico que en Linux abortaba el proceso al abrir Mi inventario (lo comprueba `npm run e2e:native`). Abre
`inventory.sqlite3` en el directorio de datos de la aplicación que proporciona Tauri.

**Ninguna apertura cambia el esquema del inventario.** DevHQ separa cinco cosas (`inventory.rs`):

- **Crear** un inventario nuevo lo hace solo la ventana, al arrancar, y solo donde no hay ningún
  archivo. Lo construye entero, con todas las migraciones en una transacción, bajo un nombre propio
  junto al definitivo (`inventory.sqlite3.new-<uuid>`), lo fuerza a disco y le da el nombre
  `inventory.sqlite3` con un enlace que el sistema de archivos rechaza si ese nombre ya existe; el
  nombre provisional se borra pase lo que pase. Así `inventory.sqlite3` nunca contiene medio
  inventario, y un archivo que aparezca ahí entretanto (el inventario nuevo de otra ventana que arranca
  a la vez, o cualquier otro) nunca se reemplaza ni se escribe. Un archivo que ya existe, aunque esté
  vacío, nunca se inicializa: DevHQ no lo dejó así, porque nunca nombra medio inventario, y se rechaza
  con `not_an_inventory`. Como los respaldos, crear necesita un sistema de archivos que admita enlaces.
  Tampoco crea uno donde, sin archivo, quedan con su nombre los archivos que SQLite guarda junto a una
  base (`inventory.sqlite3-journal`, `-wal` o `-shm`), tengan lo que tengan: SQLite empareja una base
  con su journal y su log solo por el nombre, así que reproduciría el journal de otra base sobre el
  inventario nuevo, dañándolo, o leería el log de otra como parte de él; y pueden ser lo único que
  queda de las últimas escrituras de un inventario que alguien movió o borró. La ventana lo rechaza con
  `leftover_files`, dice qué archivos encontró y no crea, lee, aplica ni borra nada, hasta que DevHQ
  vuelva a arrancar: hay que devolver a la carpeta el inventario al que pertenecen, que la ventana
  recupera entonces como cualquier otro, o sacarlos de ella. Los mira justo antes de dar el nombre al
  inventario nuevo; si para entonces el nombre ya lo tiene un archivo (el inventario nuevo de otra
  ventana que arranca a la vez, que ya escribe su log), son de ese archivo y no se rechaza nada.
  Si el inventario desaparece con la ventana abierta, ningún comando crea otro: cada uno responde que
  no hay inventario y que la ventana solo crea uno al arrancar. Al volver a abrir DevHQ, como la
  primera vez, crea uno nuevo y vacío.
- **Arrancar** (`start`) es lo que hace la ventana antes de que sus comandos o la supervisión abran
  nada. Crea el inventario si falta. Si es del esquema de este DevHQ, lo pasa a WAL: es el único sitio
  donde DevHQ fija un modo de journal. Si una escritura que no terminó lo dejó necesitando
  recuperación, primero copia sus archivos a una carpeta temporal, deja que SQLite recupere la copia,
  mira qué es y borra la copia; solo si la copia es un inventario del esquema actual deja que SQLite
  recupere el archivo de verdad, conservando todo lo confirmado, y solo si esos archivos siguen siendo
  los que copió: con la conexión que los va a recuperar ya abierta, y antes de que lea nada, vuelve a
  leer el archivo, su journal y su log (`-wal`) y compara sus bytes con los de la copia. Si algo cambió
  entretanto (otro programa los sustituyó, u otra conexión los recuperó antes), esa conexión se cierra
  sin leer, y por tanto sin recuperar nada, y la ventana vuelve a mirar desde el principio lo que haya
  ahora. Cualquier otra cosa (más antigua, más
  nueva o que no es un inventario) se queda con sus archivos como estaban y se rechaza, y así hasta que
  DevHQ vuelva a arrancar: la ventana no vuelve a abrir el archivo, ningún comando lo lee y la
  supervisión no corre.
- **Abrir** para un caso de uso (`open_current`) solo abre un archivo que existe, sin crearlo,
  actualizarlo ni cambiar su modo de journal.
- **Leer o escribir**: cada caso de uso de la ventana, y cada escritura de la puerta, empieza su
  transacción (`begin`, o `begin_write` con el lock de escritura de SQLite) y dentro de ella comprueba
  que el archivo es un inventario del esquema actual, con su estructura, y, si escribe un agente, que
  la persona sigue dejando escribir a los agentes. Lo que lee o escribe es siempre ese esquema aunque
  el archivo haya cambiado desde que se abrió; si no lo es, se rechaza dentro de la transacción, antes
  de leer o escribir nada más, con los códigos de la [puerta](#puerta-para-agentes) (`schema_outdated`,
  `schema_newer`, `schema_mismatch`, `not_an_inventory`).
- **Actualizar** un inventario de un esquema anterior lo hace solo la actualización explícita
  (`upgrade.rs`), cuando la persona pulsa Back up and upgrade en la ventana: con su propia conexión
  (`open_to_upgrade`), bajo el lock de escritura, después de un respaldo verificado y comprobando el
  resultado antes de confirmar (ver [Actualizar un inventario antiguo](#actualizar-un-inventario-antiguo)).
  Las migraciones transaccionales v1→v12 (`PRAGMA user_version`) crean el esquema de un inventario
  nuevo y actualizan uno anterior; ninguna apertura, consulta, respaldo, supervisión ni herramienta MCP
  las aplica. Una versión más nueva se rechaza en vez de adivinarse.

Un inventario más antiguo se queda tal cual hasta que la persona lo actualiza: Mi inventario dice
**Inventory needs a schema upgrade**, lo describe y ofrece Back up and upgrade si puede garantizarlo;
Settings sigue mostrando dónde están el archivo y su carpeta de respaldos, y Back up now lo rechaza sin
crear nada. Uno más nuevo dice **Inventory from a newer DevHQ**, sin reintentar, porque la respuesta no
cambiaría hasta volver a arrancar. Un inventario antiguo que además necesita recuperación (lo deja así
una actualización interrumpida antes del commit, pero también un DevHQ anterior u otro programa que
terminó a mitad de una escritura) se rechaza sin recuperarlo, respaldarlo ni actualizarlo, y sin
presumir que algún respaldo tenga lo mismo: la ventana dice qué conservar y qué comprobar de un
respaldo antes de ponerlo en su lugar (ver [Actualizar un inventario antiguo](#actualizar-un-inventario-antiguo)).

La recuperación de SQLite no es una migración, pero también cambia archivos: deshace el journal de
rollback de una escritura interrumpida, o vuelca en el archivo un log que ninguna conexión mantiene,
y conserva todo lo confirmado. La ventana solo la deja hacerse al arrancar, y sobre un inventario del
esquema actual. No es una garantía absoluta. SQLite empareja una base con su journal y su log solo por
el nombre de los archivos, y nada en ellos dice a qué base pertenecen, así que lo que un programa
ponga junto al inventario, SQLite lo aplica, y DevHQ solo puede mirar antes de que SQLite lo haga. Al
arrancar, la comparación de bytes se hace cuando la conexión ya tiene abierto el archivo de la base,
que en Windows nadie puede renombrar ni borrar mientras tanto (en Unix sí, pero lo que se compara
son los bytes que se van a recuperar); el journal y el log, en cambio, SQLite los abre por su nombre
en la primera lectura, de modo que uno que otro programa ponga en su lugar en el instante entre la
comparación y esa lectura, de microsegundos, se aplicaría. Lo mismo vale para el instante entre que la
ventana comprueba que no quedan archivos sueltos y da el nombre al inventario nuevo. Ningún proceso
de DevHQ crea esos archivos sin la base (la ventana construye el inventario nuevo con otro nombre, y
la puerta nunca crea uno), así que hace falta otro programa que los escriba justo entonces. Y la
conexión de un caso de uso puede escribir, así que si con la ventana abierta otro programa deja a
medias el archivo, o pone otro en su lugar, SQLite lo recupera al abrirlo antes de que DevHQ mire qué
es.

La base usa write-ahead logging (WAL): un lector trabaja sobre el último estado
confirmado sin esperar a nadie y un escritor no espera a los lectores; solo los escritores se
turnan. Importa porque la supervisión escribe desde el runtime asíncrono mientras la ventana
lee, y los comandos síncronos corren en el hilo principal: con el journal de rollback, una
lectura podía congelar la ventana detrás de un commit en curso. El modo queda guardado en el
archivo y `synchronous` sigue en FULL, así que cada evidencia llega al disco antes de que el
comando responda. Al cerrarse la última conexión SQLite vuelca el log en `inventory.sqlite3`,
que queda completo por sí solo. Un inventario actual en journal de rollback, como un respaldo
restaurado, pasa a WAL al arrancar la ventana; si otra conexión lo está usando en ese momento
(SQLite no espera para esto), sigue con su journal de rollback, igual de correcto, hasta el siguiente
arranque. Ninguna consulta ni escritura lo convierte. Settings muestra la
ruta exacta del archivo (`data_location` no abre la base, así que responde aunque no se pueda
leer): con DevHQ abierto se respalda con Back up now (ver [Respaldo](#respaldo)); con DevHQ
cerrado basta copiarlo si a su lado no quedó un `-wal` ni un `-journal`. Si quedó alguno, una
escritura pudo quedar a medias: el archivo no está completo por sí solo hasta recuperarlo y lo último
confirmado puede estar solo en ellos, así que se copian juntos, con el `-shm` y con sus nombres, y no
se pone un respaldo en su lugar dando por hecho que tiene lo mismo. Al arrancar, la ventana recupera
un inventario del esquema actual como se describe arriba, y deja como está uno anterior y dice qué
comprobar (ver «Un inventario antiguo que necesita recuperación» en
[Actualizar un inventario antiguo](#actualizar-un-inventario-antiguo)). Restaurar un respaldo es
otra cosa: con DevHQ cerrado se apartan juntos `inventory.sqlite3` y, si están, su `-wal`, su `-shm`
y su `-journal`, a una carpeta nueva y vacía fuera de la carpeta de datos, sin borrar ni reemplazar
nada, y solo después se copia el respaldo con el nombre `inventory.sqlite3`. Así ni el log ni el
journal del archivo anterior se aplican a la copia, y lo que tienen, incluido lo escrito después del
respaldo, sigue ahí (ver «Restaurar un respaldo» en el mismo apartado). Settings › By hand dice las
dos cosas, sin proponer nunca borrar ninguno de esos archivos.

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

Las carpetas de datos llevan el nombre del identificador de la app, y una sola decisión
(`src-tauri/src/identity.rs`) lo elige para la ventana y para la puerta, así que todo lo que guarda
DevHQ lo sigue: el inventario, sus respaldos y el lock de la ventana en la carpeta de datos, y
`this-workstation.json` y el perfil de WebView2 en la local. Solo el ejecutable que empaquetó el
instalador NSIS usa `com.devhq.desktop`, el de `tauri.conf.json`, que el instalador y su
desinstalador conocen; cualquier otro usa `com.devhq.desktop.dev`, y su ventana se titula
«DevHQ (development data)»:

| Ejecutable                                                               | Datos (inventario, `backups/`, lock)   | Carpeta local (estación, WebView2)     |
| ------------------------------------------------------------------------ | -------------------------------------- | -------------------------------------- |
| El que instaló el instalador NSIS (Windows)                              | `%APPDATA%\com.devhq.desktop`          | `%LOCALAPPDATA%\com.devhq.desktop`     |
| `npm run tauri dev`, una build debug o el release de `target/` (Windows) | `%APPDATA%\com.devhq.desktop.dev`      | `%LOCALAPPDATA%\com.devhq.desktop.dev` |
| Cualquier build en Linux (DevHQ no publica paquete)                      | `~/.local/share/com.devhq.desktop.dev` | la misma                               |

Los distingue una marca que el bundler de Tauri parchea en el ejecutable justo antes de meterlo en
el instalador (`tauri::utils::platform::bundle_type`); el de `target/` la conserva sin parchear.
Solo dice cómo se construyó el archivo. No prueba que esté instalado en ningún sitio: una copia del
ejecutable instalado, lanzada desde cualquier carpeta, usa los datos de la instalación. No decide
dónde guarda el sistema los archivos: un DevHQ lanzado desde una app empaquetada (MSIX) ve `AppData`
redirigido con cualquiera de los dos identificadores. Y `--database` sigue abriendo el archivo que
nombre. Sirve para que una build de desarrollo no abra por defecto el inventario de cada día, ni lo
migre a un esquema que ninguna versión publicada tiene; no es una barrera. Otros paquetes (deb, rpm,
AppImage, msi) usan datos de desarrollo hasta que DevHQ los publique y compruebe la marca que dejan,
y en macOS `bundle_type` responde `App` para todo ejecutable, empaquetado o no, así que allí no hay
marca que leer. La instalación existente sigue en su carpeta: nada mueve, migra ni abre datos de un
identificador a otro, y lo que una build de desarrollo guardó antes con `com.devhq.desktop` (también
en la carpeta de un paquete MSIX) queda donde está. Un test comprueba que la ventana y la puerta
resuelven las mismas carpetas con los dos identificadores, como Tauri; el recorrido nativo, que solo
maneja ejecutables sin empaquetar, que sus datos acaban en `com.devhq.desktop.dev`; y el workflow de
Windows, que el ejecutable instalado usa `com.devhq.desktop`.

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
para agentes; ninguna fila cambia y, sin fila, la puerta está cerrada. La v9 solo añade
`agent_log`, sin claves foráneas y con un índice por sistema; ninguna fila cambia. La v10 lo
convierte en `change_log`, el registro de cambios común: copia cada entrada, con su `rowid`, como
cambio de un agente (`source = agent`) sin campos estructurados (`changes` nulo), borra `agent_log`
y crea los mismos índices. Un CHECK exige que `changes`, cuando existe, sea una lista JSON y que el
nombre del agente esté exactamente cuando el autor es un agente. Añade `change_log_start`, con una
fila como mucho: el momento de la actualización, solo si el inventario ya tenía aplicaciones o
recursos. Ninguna otra fila cambia. La v11 solo añade `system_references`, las
[referencias](#referencias), con sus índices por sistema y por máquina; ninguna fila cambia. Sus CHECK
imponen lo que SQL puede decir: un tipo conocido, un destino y una etiqueta sin espacios alrededor y
dentro de sus límites, que una URL no tenga máquina y una ruta absoluta sí, que un repositorio y un
panel sean URL y que una copia local sea una ruta; Rust comprueba el resto antes de escribir. La v12
solo añade `copy_readings`, las lecturas de [copias locales](#copias-locales), con sus índices por
referencia y por sistema; ninguna fila cambia. Su sistema es `ON DELETE RESTRICT`, como en todo
registro, y la referencia, la máquina y la ruta que leyó se copian sin claves foráneas. Sus CHECK exigen
que una copia no leída diga por qué y nada más, que una leída tenga cada dato obligatorio y su working
tree contado o desconocido con el motivo (nunca ambos, nunca a medias), un HEAD de 40 dígitos
hexadecimales y ningún commit sin publicar sin HEAD. Los
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
cuentas, cloud, sync, telemetría, automation engine, Runner ni background service. DevHQ no busca
qué observar: nunca recorre discos, carpetas ni redes en busca de repositorios o servicios. Lee solo
lo que el inventario nombra (la URL de una comprobación, la ruta de una copia local de esta estación de
trabajo), cuando se le pide o, si es una comprobación supervisada, mientras la ventana está abierta. Leer
nunca ejecuta un programa que nombre lo leído ni escribe en ello: la ruta y lo que contiene son datos no
confiables, aunque los hayas declarado tú.

## Evolución prevista

1. Hecho: una comprobación HTTP(S) a petición con su propio `source`, sin cambiar la
   semántica de vigencia.
2. Hecho: supervisión cancelable de esas comprobaciones, solo con DevHQ abierto.
3. Hecho en Linux: la [puerta para agentes](#puerta-para-agentes), con la que un agente lee y
   mantiene el inventario cuando se lo pides, con su procedencia siempre visible. Lee y escribe con
   el acceso que eliges en Settings, sus cambios aparecen en Activity y la ventana abierta los nota
   sola. Quedan comprobaciones en Windows:
   [docs/puerta-para-agentes.md](docs/puerta-para-agentes.md).
4. Hecho: un [registro de cambios](#registro-de-cambios) común para ti y para los agentes, con el
   autor, cuándo y cada campo antes y después: la configuración deja rastro igual que la evidencia.
5. Hecho: las [referencias](#referencias) de cada aplicación y recurso (repositorio, copias locales,
   documentación, logs y panel), que declaras tú o un agente y DevHQ solo copia.
6. Hecho el lector, las lecturas y la puerta, con sus pruebas en Linux (CI) y en Windows; falta la
   ventana: leer, cuando se lo pides, las
   [copias locales](#copias-locales) de esta estación de trabajo sin ejecutar nada de lo que nombran,
   como evidencia propia: actividad, trabajo que solo existe en una máquina y el commit de HEAD que
   comparará la etapa de despliegues.
7. Después, automatizaciones con resultado e historial, y seguridad con almacén nativo de
   credenciales y evidencias contextualizadas.
8. A largo plazo, una supervisión que siga con la ventana cerrada y avise de lo que necesita
   atención, hecha por DevHQ mismo y no por un agente. Contradice que DevHQ no funcione en segundo plano, así que exige revisar esa regla y
   cuidar lo que consume.

Estos son límites conceptuales, **no módulos vacíos ni integraciones implementadas**.
La dirección es UI → IPC/casos de uso → dominio. SQLite conserva contexto; las credenciales no deben
vivir en componentes React, localStorage ni texto plano en SQLite.

Más allá de estos pasos, [docs/vision.md](docs/vision.md) guarda la dirección a largo plazo (DevHQ
como capa de contexto y memoria de todo el ecosistema de desarrollo, útil también con 50 proyectos)
y las preguntas con que se valoran las funcionalidades y las decisiones de arquitectura. Es una guía,
no una especificación ni un backlog: nada de lo que describe existe hasta que este README lo diga.

Las pruebas cubren navegación, tabs/teclado, filtros, rutas desconocidas, relaciones
compartidas, cronología de la demo, salud desconocida, persistencia tras reapertura,
rechazo de referencias inválidas sin escrituras parciales, restricciones del esquema,
migraciones v1/v2→v3, v3→v4, v4→v5, v5→v6, v6→v7, v7→v8, v8→v9, v9→v10, v10→v11 y v11→v12 (incluida la negativa a confirmar relaciones rotas), la actualización explícita (cada versión de la v1 a la v11, con journal de rollback y en WAL, actualizada con su consentimiento:
cada fila conservada valor a valor, un respaldo idéntico al inventario anterior con su manifiesto, que
restaurado vuelve a ser lo que se vio y se actualiza igual; ningún archivo cambia al mirarlo ni al
rechazar otra versión, otro contenido o un archivo cambiado después de mirarlo; nada se actualiza sin
la estructura exacta de su versión, con un índice que falta, una tabla o un trigger de más, una
relación rota, páginas dañadas o una columna declarada de otra forma, ni siendo v13 o de otro
programa; ninguna migración se confirma sin un respaldo verificado, con una copia dañada, cambiada o con
una fila de más, un nombre de respaldo o de manifiesto ocupado, unos nombres que el sistema no confirma
en disco (el respaldo y su manifiesto se quedan) o una carpeta imposible; una migración
que falla, pierde una fila, cambia un valor o su tipo, abre la puerta a los agentes o supervisa una
comprobación no confirma nada y deja el respaldo; un proceso al que se mata antes del commit deja el
inventario rechazado sin tocarlo y un respaldo cuyo manifiesto nombra el inventario y su SHA-256, que
tiene lo mismo que una copia recuperada aparte de los archivos y que, puesto en su lugar, se actualiza;
uno al que se mata después lo deja servible; un v6 que ya necesitaba recuperación, con un journal o con
cambios confirmados solo en su log y sin respaldo alguno, se rechaza sin tocar ni crear un archivo y sin
presumir un respaldo; tres procesos a la vez hacen un solo respaldo y una sola actualización; y la
ventana, por el manejador real de comandos, sirve el inventario en cuanto la persona lo actualiza),
vigencia y sustitución de observaciones de ambos orígenes, orden de atención, errores de
carga visibles y recuperables, comprobaciones contra un servidor local (estado esperado,
500, redirect, bucle de redirects, tiempo agotado, conexión rechazada, dominio `.invalid`
y fallo TLS), configuraciones inválidas sin escrituras parciales, errores de DevHQ que
no producen observación, la distinción de origen en cada vista real, el contrato IPC de cada comando, el contrato de atención, el respaldo
(las mismas filas que el origen comparadas una a una, sin lo no confirmado y sin esperar a un escritor,
sin reemplazar ni borrar un archivo ajeno, uno a la vez, una copia dañada que se descarta sin nombre
final, y una cuyo nombre el sistema no confirma en disco, que falla y se queda; en Linux, un error al
forzar los nombres a disco se transmite), el borrado (qué lo
impide, la confirmación y que un resultado tardío de un sujeto borrado no se registra) y las
referencias: cada destino que se rechaza y por qué, las máquinas, los duplicados, los ID que se
conservan, el registro de cambios con el campo `references`, el repositorio compartido por igualdad
exacta (en Rust, en la puerta y en la preview), el recurso que guarda copias locales de otros y no
deja de ser estación de trabajo, y el que es máquina de sus propias rutas y se borra con ellas pero no
con las de otros. Las copias locales se prueban con la batería hostil por el lector encerrado y sin
encerrar, que también comprueba que el `git` instalado sí ejecuta lo que nombran; con el lector frente al
`git` instalado en gemelos benignos (ramas, HEAD suelto, sin commits, merge y rebase a medias, stashes,
carpetas nuevas, un driver real y, en Windows, el `core.autocrlf` de la config del sistema); con un
lector encerrado que no puede arrancar ningún programa; con lo que hay en una ruta que no es una copia,
un formato que libgit2 no conoce, una carpeta sin permiso en Linux, un lector que se cuelga, muere o
devuelve algo incoherente o enorme, y remotos con credenciales; y con su esquema: la actualización, los
CHECK, la historia que impide borrar, una copia movida o quitada mientras se lee, la paginación sin huecos
ni repetidos, los planes de consulta, la estación de trabajo de cada equipo por su huella y la puerta, que
lee como agente sin tocar el registro de cambios. La supervisión se
prueba sobre un reloj simulado de Tokio, sin esperas reales: ritmo por intervalo,
continuidad tras reabrir, una sola ejecución por atraso, sin solapes, Run check que se une
a la ejecución en vuelo, pausa, cancelación al quitar o cerrar (también de una ejecución que
ya estaba escribiendo su resultado al cerrar), fallos de DevHQ visibles
que no detienen al resto, configuración ilegible que se reintenta y lo que otra conexión confirma
(una ejecución, una pausa o una comprobación nueva surten efecto en la siguiente consulta, una
lectura no cuenta como cambio y cerrar termina las consultas al instante y cierra su conexión), y el
lock de la ventana: compartido entre dos ventanas, sin crear nada al preguntar, y soltado por el
sistema cuando se mata el proceso que lo tenía; y, con peticiones
reales a un servidor local, que la evidencia programada es idéntica a la manual; y, a través del manejador real de comandos sobre el
runtime simulado de Tauri y desde un hilo sin runtime asíncrono como el principal, que la ventana puede
abrir el `Channel` de estado. Los argumentos de la puerta se prueban con una llamada a cada una de
sus 19 herramientas con un argumento que no declara, rechazada sin buscar siquiera el inventario y,
con la puerta abierta para escribir, sin cambiar una fila; con las llamadas que antes se aceptaban en
silencio (un campo mal escrito o en otra capitalización, `null` donde no cabe, un componente con
`runOn`, un `type` de más y un `id` que contradice al `name` en un borrado, un `id` numérico, `30.5`
como entero) y con las que se aceptan (`30.0`, `id` y `name` del mismo sistema, `null` donde el
esquema lo admite, `_meta` y otros campos del protocolo en `params`). Su apertura del inventario se
prueba con inventarios v6 con journal de rollback y en WAL, v11 con Read only, v12 en WAL con Read only
y con Off, la copia de un respaldo (rollback) con Read only, v13, un archivo vacío, una base SQLite de
otro programa, otra marcada con la versión 12 sin las tablas de DevHQ, otra marcada con la versión 12
con solo `applications` y `resources`, un v12 sin `agent_access`, un v12 con Off cuyo log dejó con
cambios un escritor al que se mata (como lo deja un cierre brusco), ese mismo log sin su índice, un v6
y la copia de un respaldo con el journal de rollback que deja una escritura a la que se mata a medias,
un archivo de texto, una carpeta y nada: `status` dice qué es cada uno sin presentar como sabido lo que
no pudo leer, cada una de las 19 herramientas responde o se rechaza con su código, y la carpeta queda
igual byte a byte y con las mismas fechas de modificación. Por MCP también se prueba la carrera de una
escritura: mientras `run_check` espera a un sistema que aún no responde, el inventario se sustituye por
un v11, se borra o la persona cierra la puerta, y la escritura se rechaza, el v11 queda igual, no se
crea nada y no se guarda ni la observación ni su entrada en el registro. Con otro proceso que mantiene abierto el
inventario, como la ventana, y tiene en su log un cambio que aún no está en el archivo, la puerta lee
ese cambio, rechaza por acceso una escritura y no cambia ningún archivo, ni el índice. Cuando a ese
proceso se le mata, `status` y cada herramienta dicen `needs_recovery` sin cambiar nada, y después de
que la ventana lo recupera el cambio sigue ahí. También se prueban la conexión misma, que rechaza
escribir al leer, nunca crea el archivo y deja en rollback un journal de rollback también al escribir
configuración; la lectura de un archivo sin log, que se repite cuando un escritor cambió el archivo
mientras tanto; la comparación de estructura (una tabla, una columna o un índice que falta o sobra, y un
inventario actualizado desde v6, que tiene la misma estructura que uno nuevo); y el URI con el que la
puerta abre SQLite, con espacios, `#`, `%`, `?` y letras fuera de ASCII en el nombre de la carpeta. La ventana
de historial se prueba por su límite exacto, los eventos que acompañan a cada resultado, el
aviso de lo omitido en cada vista, la paginación de observaciones y de eventos de varios
sistemas a la vez (sin huecos ni duplicados, con empates al mismo milisegundo, resultados
nuevos entre páginas y doce sistemas de historiales desiguales), que la mezcla toma de cada
sistema solo lo que devuelve de él y una fila más, y los planes de consulta, que nunca recorren el historial automático
completo ni buscan un evento automático por su observación, tampoco con estadísticas que otra
herramienta haya calculado sobre el archivo. Que ninguna apertura cambia el esquema se prueba con
cada comando de la ventana, por el manejador real de comandos, sobre un v6 en rollback y en WAL, un
v11, un v13 en rollback y en WAL, un archivo vacío, una base de otro programa y una marcada con la
versión 12 sin su estructura: cada comando que abriría el inventario se rechaza con su código y ningún
archivo de la carpeta cambia, aparece ni desaparece. También con casos de uso que abren de verdad un
v6, un v11 en WAL y un v13 en rollback y en WAL y los rechazan sin cambiar un byte ni su modo de
journal y sin crear `backups/`; con una conexión cuyo archivo pasa a la v11 después de abrirse, que
rechaza leer y escribir sin actualizarlo; con un agente al que se le cierra la puerta después de abrir
su conexión; con un inventario que desaparece con la ventana abierta y que nadie vuelve a crear; con un
archivo vacío que nunca se inicializa, tampoco con un journal al lado; con la ventana que arranca donde
no hay nada y crea un inventario en WAL; con arranques sobre escritores a los que se mata a medias: un
v12 se recupera con lo confirmado, y un v6 y un v13 se rechazan sin cambiar ningún archivo; con el
journal de una escritura que reescribía doscientos recursos, y con el log de otra ya confirmada, que
quedan sin su inventario: la ventana no crea ninguno (sin la protección, el journal dañaba el
inventario nuevo, como dice `PRAGMA integrity_check`, y el log le metía los recursos del anterior),
no cambia ningún byte de lo que quedó y, devuelto el inventario, lo recupera íntegro y con lo
confirmado; y con archivos que se sustituyen mientras la ventana recupera un v12: todos, por un v13
con una escritura sin terminar, después de que la copia resultara actual (el v13 se rechaza y queda
byte a byte como se puso; sin la protección SQLite reproducía su journal), y solo el log, por uno que
lo hace v13, con la conexión que lo va a recuperar ya abierta (se rechaza igual; si se compara antes
de abrirla, SQLite volcaba ese log en el archivo). La concurrencia de SQLite se
prueba sin esperas medidas: una lectura no espera a una escritura en curso, una escritura no espera a
una lectura abierta (que conserva su estado), cuatro arranques simultáneos crean un único inventario
sin dejar nombres provisionales, un inventario actual en rollback no lo convierte ninguna consulta ni
escritura y, ocupado, la ventana arranca sin convertirlo y lo convierte en el siguiente arranque, y al
cerrar queda un único archivo completo. Por encima de todo eso, el recorrido nativo de `npm run e2e:native` prueba la app real,
con su ventana, de punta a punta (ver [Ejecutar](#ejecutar)).
