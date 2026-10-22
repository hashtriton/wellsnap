; WellSnap NSIS hooks (bundle.windows.nsis.installerHooks in tauri.windows.conf.json). Saved as UTF-8 with a
; BOM: NSIS reads a file without one in the system code page, and the privacy page has Russian text.
;
; Privacy page: the first page of an interactive install. It shows what WellSnap sends and two switches for
; crash reports and statistics (the SignPath Foundation terms ask for the policy and for these options during
; installation). The switches start from the app's current state, which the app keeps in HKCU\Software\WellSnap
; as `CrashReports` and `UsageStats`; without them both are on, as in a new install. A switch the user changed
; goes to the app as `InstallerCrashReports` / `InstallerUsageStats` (DWORD 0 or 1), applied once at the next
; start (install_privacy.rs). An unchanged switch is not passed: after an uninstall the key is gone while the
; settings stay, and a switch shown as on must not turn telemetry back on. Passive (/P) and silent installs,
; updates included, skip the page and change nothing.
;
; On uninstall: if WellSnap turned off "Use the Print screen key to open screen capture", put the previous
; value back exactly like printscreen::restore() does, then remove HKCU\Software\WellSnap, the cache of
; snapshot-set files (screenshots that may still sit there for up to a day) and the telemetry folder.
; Runs after the files are removed, so an uninstall cancelled at the "app is running" prompt changes nothing.
; Skipped in update mode (/UPDATE), when the uninstaller runs only to replace the files.

!include LogicLib.nsh
!include nsDialogs.nsh

; This file is included before the template declares its pages and variables ($PassiveMode among them), so
; the page comes first and reads /P from the command line itself.
Var PrivacyCrash
Var PrivacyUsage
Var PrivacyCrashBefore
Var PrivacyUsageBefore
Var PrivacyCrashBox
Var PrivacyUsageBox
Var PrivacyPassed

Page custom PrivacyPage PrivacyPageLeave

Function PrivacyPage
  ClearErrors
  ${GetOptions} $CMDLINE "/P" $0
  ${IfNot} ${Errors}
    Abort
  ${EndIf}
  ; Once per run: Back keeps what the user ticked.
  ${If} $PrivacyCrashBefore == ""
    StrCpy $PrivacyCrashBefore 1
    StrCpy $PrivacyUsageBefore 1
    ClearErrors
    ReadRegDWORD $0 HKCU "Software\WellSnap" "CrashReports"
    ${IfNot} ${Errors}
      StrCpy $PrivacyCrashBefore $0
    ${EndIf}
    ClearErrors
    ReadRegDWORD $0 HKCU "Software\WellSnap" "UsageStats"
    ${IfNot} ${Errors}
      StrCpy $PrivacyUsageBefore $0
    ${EndIf}
    StrCpy $PrivacyCrash $PrivacyCrashBefore
    StrCpy $PrivacyUsage $PrivacyUsageBefore
  ${EndIf}

  ${If} $LANGUAGE = 1049
    !insertmacro MUI_HEADER_TEXT "Конфиденциальность" "Что WellSnap отправляет и как это выключить"
    StrCpy $1 "WellSnap ничего не загружает в интернет: снимки, комментарии, буфер обмена, имена файлов и заголовки окон остаются на этом компьютере.$\r$\n$\r$\nЧтобы находить ошибки, программа может отправлять отчёты о сбоях в Sentry (Европа) и раз в день анонимную статистику в PostHog (США): сколько сделано снимков, какие инструменты использовались, версии программы и системы, страна. Вместо имени случайный номер установки.$\r$\n$\r$\nДо окончания первой настройки ничего не отправляется. Выбор можно поменять в «Настройки > Конфиденциальность»."
    StrCpy $2 "Отправлять отчёты о сбоях"
    StrCpy $3 "Отправлять анонимную статистику"
    StrCpy $4 "Политика конфиденциальности"
  ${Else}
    !insertmacro MUI_HEADER_TEXT "Privacy" "What WellSnap sends and how to turn it off"
    StrCpy $1 "WellSnap uploads nothing: screenshots, comments, the clipboard, file names and window titles stay on this computer.$\r$\n$\r$\nTo find bugs, the app can send crash reports to Sentry (Europe) and, once a day, anonymous statistics to PostHog (USA): how many screenshots were taken, which tools were used, the app and system versions and the country. A random install number instead of your name.$\r$\n$\r$\nNothing is sent before the first setup is finished. You can change this later under Settings > Privacy."
    StrCpy $2 "Send crash reports"
    StrCpy $3 "Send anonymous usage statistics"
    StrCpy $4 "Privacy policy"
  ${EndIf}

  nsDialogs::Create 1018
  Pop $0
  ${If} $0 == error
    Abort
  ${EndIf}
  ${NSD_CreateLabel} 0 0 100% 90u $1
  Pop $0
  ${NSD_CreateCheckbox} 0 94u 100% 12u $2
  Pop $PrivacyCrashBox
  ${If} $PrivacyCrash <> 0
    ${NSD_Check} $PrivacyCrashBox
  ${EndIf}
  ${NSD_CreateCheckbox} 0 108u 100% 12u $3
  Pop $PrivacyUsageBox
  ${If} $PrivacyUsage <> 0
    ${NSD_Check} $PrivacyUsageBox
  ${EndIf}
  ${NSD_CreateLink} 0 126u 100% 10u $4
  Pop $0
  ${NSD_OnClick} $0 PrivacyOpenPolicy
  nsDialogs::Show
FunctionEnd

Function PrivacyPageLeave
  ${NSD_GetState} $PrivacyCrashBox $0
  ${If} $0 == ${BST_CHECKED}
    StrCpy $PrivacyCrash 1
  ${Else}
    StrCpy $PrivacyCrash 0
  ${EndIf}
  ${NSD_GetState} $PrivacyUsageBox $0
  ${If} $0 == ${BST_CHECKED}
    StrCpy $PrivacyUsage 1
  ${Else}
    StrCpy $PrivacyUsage 0
  ${EndIf}
  StrCpy $PrivacyPassed 1
FunctionEnd

Function PrivacyOpenPolicy
  Pop $0
  ${If} $LANGUAGE = 1049
    ExecShell "open" "https://github.com/hashtriton/wellsnap/blob/main/PRIVACY.ru.md"
  ${Else}
    ExecShell "open" "https://github.com/hashtriton/wellsnap/blob/main/PRIVACY.md"
  ${EndIf}
FunctionEnd

!macro NSIS_HOOK_POSTINSTALL
  ${If} $PrivacyPassed = 1
    ${If} $PrivacyCrash <> $PrivacyCrashBefore
      WriteRegDWORD HKCU "Software\WellSnap" "InstallerCrashReports" $PrivacyCrash
    ${EndIf}
    ${If} $PrivacyUsage <> $PrivacyUsageBefore
      WriteRegDWORD HKCU "Software\WellSnap" "InstallerUsageStats" $PrivacyUsage
    ${EndIf}
  ${EndIf}
!macroend

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
    ; Unsent crash reports and minidumps (telemetry/paths.rs); an update keeps them for the next start.
    RMDir /r "$LOCALAPPDATA\${BUNDLEID}\telemetry"
    ClearErrors
    Pop $1
    Pop $0
  ${EndIf}
!macroend
