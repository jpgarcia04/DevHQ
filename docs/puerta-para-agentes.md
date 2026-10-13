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

Ninguna apertura actualiza un inventario: ni la ventana, ni la supervisión, ni un respaldo, ni una
herramienta de la puerta (README, «Arquitectura»). Cada escritura de un agente comprueba el esquema y
el acceso dentro de su propia transacción. Lo que falta es la actualización misma, un paso explícito
en la ventana:

- Un inventario anterior a la v12 no lo sirve ninguna parte de este DevHQ. La ventana dice que
  necesita actualizarse, pero aún no ofrece hacerlo: hasta entonces solo lo abre un DevHQ anterior,
  que lo actualiza al abrirlo y sin respaldo previo. Por eso esta versión no debe instalarse sobre un
  inventario anterior a la v12.
- El respaldo que debe preceder a la actualización (verificado, con la versión de origen, antes de
  confirmar las migraciones), su manifiesto y la recuperación manual que lo acompañe.
- Un inventario que la puerta rechaza con `needs_recovery` y que, recuperado, no es del esquema actual:
  la ventana lo deja como está, con sus archivos sin recuperar, y la actualización explícita tendrá
  que decidir cuándo recuperarlo.
- `backup` comprueba esquema y acceso en la conexión que copia justo antes de `VACUUM INTO`, que no
  admite una transacción: si en ese instante la persona cierra la puerta, la copia sigue. No escribe el
  inventario, y su verificación rechaza una copia de otra versión.
- Con la ventana abierta, una conexión de un caso de uso puede escribir: si otro programa deja a medias
  el archivo o pone otro en su lugar, SQLite lo recupera al abrirlo antes de que DevHQ mire qué es.
- SQLite empareja una base con su journal y su log solo por el nombre. Al arrancar, la ventana no crea
  un inventario junto a los que quedaron sin su base y solo recupera los archivos cuyos bytes comparó
  con la copia recuperada, ya abierta la conexión; pero un journal o un log que otro programa ponga en
  el instante entre esa comprobación y la primera lectura de SQLite se aplicaría. Cerrarlo del todo
  pediría que ningún otro programa pudiera escribir en la carpeta mientras tanto.

## Lo que la lectura sin cambios no cubre

La puerta lee sin escribir en el inventario ni cambiar lo que contiene, y rechaza con `needs_recovery`
lo que habría que recuperar (README, «Puerta para agentes»). Lo que eso no evita:

- Mientras una consulta lee, unos milisegundos, la última conexión que se cierra, como la ventana al
  cerrarla, no puede volcar su log en el archivo: SQLite solo lo hace si nadie más tiene abierta la
  base. El log se queda, sin conexión que lo mantenga, y la puerta rechaza el inventario con
  `needs_recovery` hasta que la ventana vuelve a abrirse y lo recupera. No se pierde nada confirmado.
  Cualquier lector que no vuelque el log tiene el mismo efecto; volcarlo es justo el cambio que la
  puerta no hace.
- Si la última conexión se cierra entre que la puerta mira los archivos y SQLite abre el log, un
  instante de microsegundos, SQLite crea un `-wal` vacío junto al inventario: abrir un archivo en WAL sin
  log lo crea, y solo `immutable` lo evita, que no sirve para una base que alguien tiene abierta. No
  cambia el contenido, y la siguiente apertura lo usa. Es el único archivo que una lectura de la puerta
  puede crear: por eso el README promete que la lectura no cambia el contenido, no que deje intactos
  todos los archivos.
- Si quien mantiene el log termina justo después de que la puerta lo vea abierto, SQLite lee sus cambios
  en memoria, sin cambiar ningún archivo, en vez de que la puerta lo rechace. Windows suelta los
  bloqueos de un proceso muerto «cuando los recursos lo permiten»: durante ese momento la puerta lo ve
  aún abierto y lee igual, sin escribir.
- La comparación de estructura mira tablas, columnas e índices por nombre y declaración, no las
  restricciones (`CHECK`, claves foráneas) ni el contenido: un archivo que la pasa puede estar dañado,
  y por eso `status` dice que la integridad no se comprobó.

## Por probar con otros clientes

Solo Claude Code se ha probado como cliente. Codex u otro cliente MCP por stdio deberían funcionar
igual (la puerta solo usa `initialize`, `ping`, `tools/list` y `tools/call`), pero nadie lo ha visto.

Desde que la puerta comprueba los argumentos contra los esquemas que publica, un cliente que mande
claves que una herramienta no declara, o `null` en un campo opcional que no lo admite (como hacen
algunos clientes con todo lo que el modelo deja vacío), recibe `invalid_input` donde antes la puerta lo
ignoraba. Ningún cliente real se ha probado aún contra la puerta estricta, tampoco Claude Code.
