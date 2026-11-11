; Zeo Player 的 Windows 安装包（release.yml 在 runner 上用 iscc 编译：
;   iscc /DAppVersion=<version> /DSourceDir=<packaged dir> scripts/setup.iss
; 产物 SetupOutput\ZeoPlayer-Setup.exe——per-user 安装到 LocalAppData，
; 不要管理员权限，自更新（`zeo-updater` 的原地替换）才能写进去。

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef SourceDir
  #define SourceDir ".nsh\out\ZeoPlayer"
#endif

[Setup]
AppName=Zeo Player
AppVersion={#AppVersion}
AppPublisher=subframe7536
AppPublisherURL=https://github.com/subframe7536/zeo-player-nsh
AppSupportURL=https://github.com/subframe7536/zeo-player-nsh/issues
DefaultDirName={autopf}\ZeoPlayer
DefaultGroupName=Zeo Player
; per-user 安装：自更新的安装校验（`can_install`）要求用户可写。
PrivilegesRequired=lowest
OutputDir=.nsh\out
OutputBaseFilename=ZeoPlayer-Setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
ArchitecturesInstallIn64BitMode=x64compatible
UninstallDisplayIcon={app}\ZeoPlayer.exe

[Files]
; nsh package 的整目录（exe + nsh/ 资源含更新配置），结构原样保留。
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs

[Icons]
Name: "{group}\Zeo Player"; Filename: "{app}\ZeoPlayer.exe"
Name: "{group}\卸载 Zeo Player"; Filename: "{uninstallexe}"
Name: "{autodesktop}\Zeo Player"; Filename: "{app}\ZeoPlayer.exe"; Tasks: desktopicon

[Tasks]
Name: "desktopicon"; Description: "创建桌面快捷方式"; GroupDescription: "附加任务："

[Run]
Filename: "{app}\ZeoPlayer.exe"; Description: "启动 Zeo Player"; Flags: nowait postinstall skipifsilent
