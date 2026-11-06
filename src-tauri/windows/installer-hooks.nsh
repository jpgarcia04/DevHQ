; Hooks DevHQ adds to tauri-bundler's NSIS template (tauri.conf.json, bundle.windows.nsis).
;
; DevHQ keeps the person's inventory in %USERPROFILE%\DevHQ, which the uninstaller never names, so no
; uninstall deletes it. "Delete the application data" deletes $APPDATA\${BUNDLEID} and
; $LOCALAPPDATA\${BUNDLEID}: the WebView2 profile, and the folder where an older DevHQ kept the
; inventory. That folder may still hold it, waiting to be moved, or its backups; then the box is
; ignored and nothing of the application data is deleted. Deleting an inventory is never part of
; uninstalling: the person deletes the folder, if ever, as an operation of its own.
;
; The template reads the box when its confirmation page is left, before Section Uninstall, and this
; hook runs first thing in that section, before the template deletes anything.

!macro NSIS_HOOK_PREUNINSTALL
  ${If} $DeleteAppDataCheckboxState = 1
    ${If} ${FileExists} "$APPDATA\${BUNDLEID}\inventory.sqlite3"
    ${OrIf} ${FileExists} "$APPDATA\${BUNDLEID}\inventory.sqlite3-wal"
    ${OrIf} ${FileExists} "$APPDATA\${BUNDLEID}\inventory.sqlite3-journal"
    ${OrIf} ${FileExists} "$APPDATA\${BUNDLEID}\backups\*.*"
      StrCpy $DeleteAppDataCheckboxState 0
      DetailPrint "Kept $APPDATA\${BUNDLEID}: it holds an inventory or its backups."
    ${EndIf}
  ${EndIf}
!macroend
