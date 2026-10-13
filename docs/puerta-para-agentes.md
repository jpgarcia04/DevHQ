# Puerta para agentes: lo pendiente

La puerta está construida y probada en Linux: `devhq --mcp` con sus herramientas de lectura y
escritura, el acceso que eliges en Settings → Agent gate, sus cambios en el registro común y en Activity, la ventana abierta
que nota sola lo que cambia un agente y `status` diciendo si hay una ventana abierta. Su
comportamiento está en el README («Puerta para agentes»), que es la especificación. Aquí queda solo lo
que falta comprobar; cuando algo se resuelve, pasa al README en el mismo commit y sale de
aquí.

## Por comprobar en Windows

El workflow de Windows ya prueba, con el `devhq.exe` instalado, que la puerta responde por stdin y
stdout aunque el ejecutable no tenga consola, que abre el inventario de `%APPDATA%`, que ve cerrada la
ventana que acaba de cerrarse y que termina al cerrarse su entrada; y con la build de prueba, en el
recorrido nativo sobre WebView2, que la ventana nota sola lo que escribe la puerta y que `status` la ve
abierta. Lo que solo se ve en la laptop:

- Claude Code de verdad, registrado con `claude mcp add --scope user`, lanzando el `devhq.exe`
  instalado desde una terminal normal: una sesión que lee y escribe con la ventana abierta.
- Una puerta lanzada desde la app de escritorio de Claude (MSIX): qué inventario abre, qué dice
  `storedAt` y si `windowOpen` ve la ventana del DevHQ instalado. El lock está junto al inventario, y
  Windows también redirige ese archivo.

## Lo que queda para la actualización explícita

Las consultas y los cambios de configuración de la puerta abren el inventario tal como lo encuentran
y solo sirven el esquema actual. Lo demás espera a la actualización explícita de inventarios, que hará
que ninguna apertura actualice sola:

- Los registros (`record_observation`, `open_incident`, `update_incident`, `record_event`),
  `run_check`, `backup` y `read_copies` comprueban el archivo antes de empezar, pero luego pasan por
  los casos de uso de la ventana, que lo abren con `inventory::open`: un inventario en rollback pasa a
  WAL al escribir, y si entre la comprobación y la escritura otro proceso sustituye el archivo por un
  inventario más antiguo o lo borra, esa apertura lo actualiza o crea uno vacío.
- Esas mismas escrituras comprueban el acceso antes de abrir su propia transacción, no dentro de ella
  como los cambios de configuración: si la persona cierra la puerta en Settings justo en ese
  instante, la escritura ya admitida sigue.
- Un inventario más antiguo no sirve a ningún agente hasta que la ventana lo abre, y la ventana lo
  actualiza sin preguntar y sin respaldo previo; Back up now, sobre uno antiguo, también actualiza
  antes de copiar. Lo mismo vale para uno que la puerta rechaza con `needs_recovery`: la ventana lo
  recupera al abrirlo y, si además es antiguo, lo actualiza sin respaldo.

## Lo que la lectura sin cambios no cubre

La puerta lee sin cambiar ningún archivo del inventario y rechaza con `needs_recovery` lo que habría
que recuperar (README, «Puerta para agentes»). Lo que eso no evita:

- Mientras una consulta lee, unos milisegundos, la última conexión que se cierra, como la ventana al
  cerrarla, no puede volcar su log en el archivo: SQLite solo lo hace si nadie más tiene abierta la
  base. El log se queda, sin conexión que lo mantenga, y la puerta rechaza el inventario con
  `needs_recovery` hasta que la ventana vuelve a abrirse y lo recupera. No se pierde nada confirmado.
  Cualquier lector que no vuelque el log tiene el mismo efecto; volcarlo es justo el cambio que la
  puerta no hace.
- Si la última conexión se cierra entre que la puerta mira los archivos y SQLite abre el log, un
  instante de microsegundos, SQLite crea un `-wal` vacío junto al inventario: abrir un archivo en WAL sin
  log lo crea, y solo `immutable` lo evita, que no sirve para una base que alguien tiene abierta. No
  cambia el contenido, y la siguiente apertura lo usa.
- Si quien mantiene el log termina justo después de que la puerta lo vea abierto, SQLite lee sus cambios
  en memoria, sin cambiar ningún archivo, en vez de que la puerta lo rechace. Windows suelta los
  bloqueos de un proceso muerto «cuando los recursos lo permiten»: durante ese momento la puerta lo ve
  aún abierto y lee igual, sin escribir.

## Por probar con otros clientes

Solo Claude Code se ha probado como cliente. Codex u otro cliente MCP por stdio deberían funcionar
igual (la puerta solo usa `initialize`, `ping`, `tools/list` y `tools/call`), pero nadie lo ha visto.

Desde que la puerta comprueba los argumentos contra los esquemas que publica, un cliente que mande
claves que una herramienta no declara, o `null` en un campo opcional que no lo admite (como hacen
algunos clientes con todo lo que el modelo deja vacío), recibe `invalid_input` donde antes la puerta lo
ignoraba. Ningún cliente real se ha probado aún contra la puerta estricta, tampoco Claude Code.
