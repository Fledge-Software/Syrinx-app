# Generated from crates/xtask/src/packaging/registration.rs.
# Do not edit by hand: change the Rust table, then run `cargo xtask write-registration`.
# Every key is under HKEY_CURRENT_USER. Nothing here sets the default browser.

$SyrinxProduct = 'Syrinx'
$SyrinxRegistryName = 'SyrinxBrowser'
$SyrinxExecutable = 'syrinx.exe'
$SyrinxInstallFolder = 'Programs\Syrinx'
$SyrinxDataFolder = 'Syrinx\data'
$SyrinxPreviewDataFolder = 'SyrinxProductPreview'
$SyrinxPackageFiles = @('install-syrinx.ps1', 'register-syrinx.ps1', 'syrinx-apply.exe', 'syrinx-common.ps1', 'syrinx-registration.ps1', 'syrinx.exe', 'unregister-syrinx.ps1')
$SyrinxLinkMarker = '--'
$SyrinxAcceptsOpenUrl = $true
$SyrinxOwnerMarkerName = 'SyrinxPackagingOwner'
$SyrinxOwnerMarkerData = 'syrinx-packaging-v1'
$SyrinxUninstallKey = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\SyrinxBrowser'

$SyrinxRegistryValues = @(
    @{ Key = 'Software\Clients\StartMenuInternet\SyrinxBrowser'; Name = 'SyrinxPackagingOwner'; Kind = 'String'; Data = 'syrinx-packaging-v1' }
    @{ Key = 'Software\Clients\StartMenuInternet\SyrinxBrowser'; Name = ''; Kind = 'String'; Data = 'Syrinx' }
    @{ Key = 'Software\Clients\StartMenuInternet\SyrinxBrowser\DefaultIcon'; Name = ''; Kind = 'String'; Data = '{exe},0' }
    @{ Key = 'Software\Clients\StartMenuInternet\SyrinxBrowser\shell\open\command'; Name = ''; Kind = 'String'; Data = '"{exe}"' }
    @{ Key = 'Software\Clients\StartMenuInternet\SyrinxBrowser\Capabilities'; Name = 'ApplicationName'; Kind = 'String'; Data = 'Syrinx' }
    @{ Key = 'Software\Clients\StartMenuInternet\SyrinxBrowser\Capabilities'; Name = 'ApplicationDescription'; Kind = 'String'; Data = 'Syrinx web browser' }
    @{ Key = 'Software\Clients\StartMenuInternet\SyrinxBrowser\Capabilities'; Name = 'ApplicationIcon'; Kind = 'String'; Data = '{exe},0' }
    @{ Key = 'Software\Clients\StartMenuInternet\SyrinxBrowser\Capabilities\StartMenu'; Name = 'StartMenuInternet'; Kind = 'String'; Data = 'SyrinxBrowser' }
    @{ Key = 'Software\Clients\StartMenuInternet\SyrinxBrowser\Capabilities\URLAssociations'; Name = 'http'; Kind = 'String'; Data = 'SyrinxBrowserURL' }
    @{ Key = 'Software\Clients\StartMenuInternet\SyrinxBrowser\Capabilities\URLAssociations'; Name = 'https'; Kind = 'String'; Data = 'SyrinxBrowserURL' }
    @{ Key = 'Software\Classes\SyrinxBrowserURL'; Name = 'SyrinxPackagingOwner'; Kind = 'String'; Data = 'syrinx-packaging-v1' }
    @{ Key = 'Software\Classes\SyrinxBrowserURL'; Name = ''; Kind = 'String'; Data = 'Syrinx URL' }
    @{ Key = 'Software\Classes\SyrinxBrowserURL'; Name = 'FriendlyTypeName'; Kind = 'String'; Data = 'Syrinx URL' }
    @{ Key = 'Software\Classes\SyrinxBrowserURL\Application'; Name = 'ApplicationName'; Kind = 'String'; Data = 'Syrinx' }
    @{ Key = 'Software\Classes\SyrinxBrowserURL\Application'; Name = 'ApplicationDescription'; Kind = 'String'; Data = 'Syrinx web browser' }
    @{ Key = 'Software\Classes\SyrinxBrowserURL\Application'; Name = 'ApplicationIcon'; Kind = 'String'; Data = '{exe},0' }
    @{ Key = 'Software\Classes\SyrinxBrowserURL\DefaultIcon'; Name = ''; Kind = 'String'; Data = '{exe},0' }
    @{ Key = 'Software\Classes\SyrinxBrowserURL\shell\open\command'; Name = ''; Kind = 'String'; Data = '"{exe}" -- "%1"' }
    @{ Key = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\SyrinxBrowser'; Name = 'SyrinxPackagingOwner'; Kind = 'String'; Data = 'syrinx-packaging-v1' }
    @{ Key = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\SyrinxBrowser'; Name = 'DisplayName'; Kind = 'String'; Data = 'Syrinx' }
    @{ Key = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\SyrinxBrowser'; Name = 'DisplayVersion'; Kind = 'String'; Data = '{version}' }
    @{ Key = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\SyrinxBrowser'; Name = 'DisplayIcon'; Kind = 'String'; Data = '{exe},0' }
    @{ Key = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\SyrinxBrowser'; Name = 'InstallLocation'; Kind = 'String'; Data = '{dir}' }
    @{ Key = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\SyrinxBrowser'; Name = 'UninstallString'; Kind = 'String'; Data = 'powershell.exe -NoProfile -ExecutionPolicy RemoteSigned -File "{dir}\unregister-syrinx.ps1" -Uninstall -PauseAtEnd' }
    @{ Key = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\SyrinxBrowser'; Name = 'NoModify'; Kind = 'DWord'; Data = 1 }
    @{ Key = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\SyrinxBrowser'; Name = 'NoRepair'; Kind = 'DWord'; Data = 1 }
)

$SyrinxOwnedKeys = @('Software\Clients\StartMenuInternet\SyrinxBrowser', 'Software\Classes\SyrinxBrowserURL', 'Software\Microsoft\Windows\CurrentVersion\Uninstall\SyrinxBrowser')
$SyrinxSharedValues = @(
    @{ Key = 'Software\RegisteredApplications'; Name = 'SyrinxBrowser'; Kind = 'String'; Data = 'Software\Clients\StartMenuInternet\SyrinxBrowser\Capabilities' }
)
