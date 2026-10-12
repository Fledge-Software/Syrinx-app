# Generated from crates/xtask/src/packaging/registration.rs.
# Do not edit by hand: change the Rust table, then run `cargo xtask write-registration`.
# Every key is under HKEY_CURRENT_USER. Nothing here sets the default browser.

$AtlasProduct = 'Atlas'
$AtlasExecutable = 'atlas-app.exe'
$AtlasInstallFolder = 'Programs\Atlas'
$AtlasDataFolders = @('AtlasProductPreview', 'Atlas\data')
$AtlasOpenUrlSwitch = '--open-url'
$AtlasAcceptsOpenUrl = $false

$AtlasRegistryValues = @(
    @{ Key = 'Software\Clients\StartMenuInternet\Atlas'; Name = ''; Kind = 'String'; Data = 'Atlas' }
    @{ Key = 'Software\Clients\StartMenuInternet\Atlas\DefaultIcon'; Name = ''; Kind = 'String'; Data = '{exe},0' }
    @{ Key = 'Software\Clients\StartMenuInternet\Atlas\shell\open\command'; Name = ''; Kind = 'String'; Data = '"{exe}"' }
    @{ Key = 'Software\Clients\StartMenuInternet\Atlas\Capabilities'; Name = 'ApplicationName'; Kind = 'String'; Data = 'Atlas' }
    @{ Key = 'Software\Clients\StartMenuInternet\Atlas\Capabilities'; Name = 'ApplicationDescription'; Kind = 'String'; Data = 'Atlas web browser' }
    @{ Key = 'Software\Clients\StartMenuInternet\Atlas\Capabilities'; Name = 'ApplicationIcon'; Kind = 'String'; Data = '{exe},0' }
    @{ Key = 'Software\Clients\StartMenuInternet\Atlas\Capabilities\StartMenu'; Name = 'StartMenuInternet'; Kind = 'String'; Data = 'Atlas' }
    @{ Key = 'Software\Clients\StartMenuInternet\Atlas\Capabilities\URLAssociations'; Name = 'http'; Kind = 'String'; Data = 'AtlasURL' }
    @{ Key = 'Software\Clients\StartMenuInternet\Atlas\Capabilities\URLAssociations'; Name = 'https'; Kind = 'String'; Data = 'AtlasURL' }
    @{ Key = 'Software\RegisteredApplications'; Name = 'Atlas'; Kind = 'String'; Data = 'Software\Clients\StartMenuInternet\Atlas\Capabilities' }
    @{ Key = 'Software\Classes\AtlasURL'; Name = ''; Kind = 'String'; Data = 'Atlas URL' }
    @{ Key = 'Software\Classes\AtlasURL'; Name = 'FriendlyTypeName'; Kind = 'String'; Data = 'Atlas URL' }
    @{ Key = 'Software\Classes\AtlasURL'; Name = 'URL Protocol'; Kind = 'String'; Data = '' }
    @{ Key = 'Software\Classes\AtlasURL\Application'; Name = 'ApplicationName'; Kind = 'String'; Data = 'Atlas' }
    @{ Key = 'Software\Classes\AtlasURL\Application'; Name = 'ApplicationDescription'; Kind = 'String'; Data = 'Atlas web browser' }
    @{ Key = 'Software\Classes\AtlasURL\Application'; Name = 'ApplicationIcon'; Kind = 'String'; Data = '{exe},0' }
    @{ Key = 'Software\Classes\AtlasURL\DefaultIcon'; Name = ''; Kind = 'String'; Data = '{exe},0' }
    @{ Key = 'Software\Classes\AtlasURL\shell\open\command'; Name = ''; Kind = 'String'; Data = '"{exe}" --open-url %1' }
    @{ Key = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\Atlas'; Name = 'DisplayName'; Kind = 'String'; Data = 'Atlas' }
    @{ Key = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\Atlas'; Name = 'DisplayVersion'; Kind = 'String'; Data = '{version}' }
    @{ Key = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\Atlas'; Name = 'DisplayIcon'; Kind = 'String'; Data = '{exe},0' }
    @{ Key = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\Atlas'; Name = 'InstallLocation'; Kind = 'String'; Data = '{dir}' }
    @{ Key = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\Atlas'; Name = 'UninstallString'; Kind = 'String'; Data = 'powershell.exe -NoProfile -ExecutionPolicy Bypass -File "{dir}\unregister-atlas.ps1" -Uninstall' }
    @{ Key = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\Atlas'; Name = 'NoModify'; Kind = 'DWord'; Data = 1 }
    @{ Key = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\Atlas'; Name = 'NoRepair'; Kind = 'DWord'; Data = 1 }
)

$AtlasOwnedKeys = @('Software\Clients\StartMenuInternet\Atlas', 'Software\Classes\AtlasURL', 'Software\Microsoft\Windows\CurrentVersion\Uninstall\Atlas')
$AtlasSharedValues = @(
    @{ Key = 'Software\RegisteredApplications'; Name = 'Atlas' }
)
