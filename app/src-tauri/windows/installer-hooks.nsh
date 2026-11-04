; Lunatic Princess hooks for Tauri's NSIS installer template (bundle.windows.nsis.installerHooks).
; Keep this file ASCII: it is a build script, like build.ps1.
;
; Everything the installed app writes stays in the folder picked on the directory page. A "data" folder next to the
; exe makes the app use it as its data directory (lp-core paths.rs, rule 2), so settings.json, session.dat (the login
; cookies), the WebView2 profile and the default download folder (data\Music) all live in $INSTDIR\data.

!macro NSIS_HOOK_POSTINSTALL
  ; Kept as is on a reinstall or an upgrade, so the login and settings survive.
  CreateDirectory "$INSTDIR\data"
!macroend

; "Delete app data" on the uninstall confirm page. Tauri itself only clears %APPDATA% / %LOCALAPPDATA%\<identifier>,
; which this app never uses. Downloaded music is never deleted: data\Music and data\ are only removed when empty.
; Nothing happens on a silent uninstall (the box cannot be ticked) or when the uninstaller runs for an upgrade.
!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
    Delete "$INSTDIR\data\settings.json"
    Delete "$INSTDIR\data\settings.json.tmp"
    Delete "$INSTDIR\data\session.dat"
    Delete "$INSTDIR\data\session.dat.tmp"
    RMDir /r "$INSTDIR\data\webview"
    RMDir /r "$INSTDIR\data\logs"
    RMDir "$INSTDIR\data\Music"
    RMDir "$INSTDIR\data"
    RMDir "$INSTDIR"
  ${EndIf}
!macroend
