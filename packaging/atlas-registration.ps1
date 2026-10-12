# Generated from crates/xtask/src/packaging/registration.rs.
# Do not edit by hand: change the Rust table, then run `cargo xtask write-registration`.
# Every key is under HKEY_CURRENT_USER. Nothing here sets the default browser.

$AtlasProduct = 'Atlas'
$AtlasRegistryName = 'AtlasBrowser'
$AtlasExecutable = 'atlas-app.exe'
$AtlasInstallFolder = 'Programs\Atlas'
$AtlasDataFolder = 'Atlas\data'
$AtlasPreviewDataFolder = 'AtlasProductPreview'
$AtlasPackageFiles = @('atlas-app.exe', 'atlas-common.ps1', 'atlas-registration.ps1', 'install-atlas.ps1', 'register-atlas.ps1', 'unregister-atlas.ps1')
$AtlasLinkMarker = '--'
$AtlasAcceptsOpenUrl = $false
$AtlasOwnerMarkerName = 'AtlasPackagingOwner'
$AtlasOwnerMarkerData = 'atlas-packaging-v1'
$AtlasUninstallKey = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\AtlasBrowser'

$AtlasRegistryValues = @(
    @{ Key = 'Software\Clients\StartMenuInternet\AtlasBrowser'; Name = 'AtlasPackagingOwner'; Kind = 'String'; Data = 'atlas-packaging-v1' }
    @{ Key = 'Software\Clients\StartMenuInternet\AtlasBrowser'; Name = ''; Kind = 'String'; Data = 'Atlas' }
    @{ Key = 'Software\Clients\StartMenuInternet\AtlasBrowser\DefaultIcon'; Name = ''; Kind = 'String'; Data = '{exe},0' }
    @{ Key = 'Software\Clients\StartMenuInternet\AtlasBrowser\shell\open\command'; Name = ''; Kind = 'String'; Data = '"{exe}"' }
    @{ Key = 'Software\Clients\StartMenuInternet\AtlasBrowser\Capabilities'; Name = 'ApplicationName'; Kind = 'String'; Data = 'Atlas' }
    @{ Key = 'Software\Clients\StartMenuInternet\AtlasBrowser\Capabilities'; Name = 'ApplicationDescription'; Kind = 'String'; Data = 'Atlas web browser' }
    @{ Key = 'Software\Clients\StartMenuInternet\AtlasBrowser\Capabilities'; Name = 'ApplicationIcon'; Kind = 'String'; Data = '{exe},0' }
    @{ Key = 'Software\Clients\StartMenuInternet\AtlasBrowser\Capabilities\StartMenu'; Name = 'StartMenuInternet'; Kind = 'String'; Data = 'AtlasBrowser' }
    @{ Key = 'Software\Clients\StartMenuInternet\AtlasBrowser\Capabilities\URLAssociations'; Name = 'http'; Kind = 'String'; Data = 'AtlasBrowserURL' }
    @{ Key = 'Software\Clients\StartMenuInternet\AtlasBrowser\Capabilities\URLAssociations'; Name = 'https'; Kind = 'String'; Data = 'AtlasBrowserURL' }
    @{ Key = 'Software\Classes\AtlasBrowserURL'; Name = 'AtlasPackagingOwner'; Kind = 'String'; Data = 'atlas-packaging-v1' }
    @{ Key = 'Software\Classes\AtlasBrowserURL'; Name = ''; Kind = 'String'; Data = 'Atlas URL' }
    @{ Key = 'Software\Classes\AtlasBrowserURL'; Name = 'FriendlyTypeName'; Kind = 'String'; Data = 'Atlas URL' }
    @{ Key = 'Software\Classes\AtlasBrowserURL\Application'; Name = 'ApplicationName'; Kind = 'String'; Data = 'Atlas' }
    @{ Key = 'Software\Classes\AtlasBrowserURL\Application'; Name = 'ApplicationDescription'; Kind = 'String'; Data = 'Atlas web browser' }
    @{ Key = 'Software\Classes\AtlasBrowserURL\Application'; Name = 'ApplicationIcon'; Kind = 'String'; Data = '{exe},0' }
    @{ Key = 'Software\Classes\AtlasBrowserURL\DefaultIcon'; Name = ''; Kind = 'String'; Data = '{exe},0' }
    @{ Key = 'Software\Classes\AtlasBrowserURL\shell\open\command'; Name = ''; Kind = 'String'; Data = '"{exe}" -- "%1"' }
    @{ Key = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\AtlasBrowser'; Name = 'AtlasPackagingOwner'; Kind = 'String'; Data = 'atlas-packaging-v1' }
    @{ Key = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\AtlasBrowser'; Name = 'DisplayName'; Kind = 'String'; Data = 'Atlas' }
    @{ Key = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\AtlasBrowser'; Name = 'DisplayVersion'; Kind = 'String'; Data = '{version}' }
    @{ Key = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\AtlasBrowser'; Name = 'DisplayIcon'; Kind = 'String'; Data = '{exe},0' }
    @{ Key = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\AtlasBrowser'; Name = 'InstallLocation'; Kind = 'String'; Data = '{dir}' }
    @{ Key = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\AtlasBrowser'; Name = 'UninstallString'; Kind = 'String'; Data = 'powershell.exe -NoProfile -ExecutionPolicy RemoteSigned -File "{dir}\unregister-atlas.ps1" -Uninstall -PauseAtEnd' }
    @{ Key = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\AtlasBrowser'; Name = 'NoModify'; Kind = 'DWord'; Data = 1 }
    @{ Key = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\AtlasBrowser'; Name = 'NoRepair'; Kind = 'DWord'; Data = 1 }
)

$AtlasOwnedKeys = @('Software\Clients\StartMenuInternet\AtlasBrowser', 'Software\Classes\AtlasBrowserURL', 'Software\Microsoft\Windows\CurrentVersion\Uninstall\AtlasBrowser')
$AtlasSharedValues = @(
    @{ Key = 'Software\RegisteredApplications'; Name = 'AtlasBrowser'; Kind = 'String'; Data = 'Software\Clients\StartMenuInternet\AtlasBrowser\Capabilities' }
)
