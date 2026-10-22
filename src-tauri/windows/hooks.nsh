; WellSnap NSIS hooks (bundle.windows.nsis.installerHooks in tauri.windows.conf.json).
;
; On uninstall: if WellSnap turned off "Use the Print screen key to open screen capture", put the previous
; value back exactly like printscreen::restore() does, then remove HKCU\Software\WellSnap and the cache of
; snapshot-set files (screenshots that may still sit there for up to a day).
; Runs after the files are removed, so an uninstall cancelled at the "app is running" prompt changes nothing.
; Skipped in update mode (/UPDATE), when the uninstaller runs only to replace the files.

!include LogicLib.nsh

!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $UpdateMode <> 1
    Push $0
    Push $1
    ClearErrors
    ReadRegDWORD $0 HKCU "Software\WellSnap" "PrintScreenTakenOver"
    ${IfNot} ${Errors}
      ClearErrors
      ReadRegDWORD $1 HKCU "Software\WellSnap" "PrevPrintScreenKeyForSnippingEnabled"
      ${If} ${Errors}
        ; The value did not exist before WellSnap changed it.
        DeleteRegValue HKCU "Control Panel\Keyboard" "PrintScreenKeyForSnippingEnabled"
      ${Else}
        WriteRegDWORD HKCU "Control Panel\Keyboard" "PrintScreenKeyForSnippingEnabled" $1
      ${EndIf}
      ; WM_SETTINGCHANGE to HWND_BROADCAST with SMTO_ABORTIFHUNG and 1 s per window, as printscreen.rs does:
      ; the built-in SendMessage /TIMEOUT uses SMTO_NORMAL and would wait on hung windows.
      System::Call 'user32::SendMessageTimeoutW(p 0xFFFF, i 0x1A, p 0, w "Control Panel\Keyboard", i 0x2, i 1000, *p .r1)'
    ${EndIf}
    DeleteRegKey HKCU "Software\WellSnap"
    ; app_cache_dir()/paste in lib.rs; settings in the config folder stay unless the user ticks "delete app data".
    SetShellVarContext current
    RMDir /r "$LOCALAPPDATA\${BUNDLEID}\paste"
    ClearErrors
    Pop $1
    Pop $0
  ${EndIf}
!macroend
