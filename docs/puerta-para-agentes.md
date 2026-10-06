# Puerta para agentes: lo pendiente

La puerta está construida y probada en Linux: `devhq --mcp` con sus herramientas de lectura y
escritura, el acceso que eliges en Settings → Agent gate, `agent_log` en Activity, la ventana abierta
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

## Por probar con otros clientes

Solo Claude Code se ha probado como cliente. Codex u otro cliente MCP por stdio deberían funcionar
igual (la puerta solo usa `initialize`, `ping`, `tools/list` y `tools/call`), pero nadie lo ha visto.
