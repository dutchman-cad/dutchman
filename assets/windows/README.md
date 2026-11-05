# Windows installer

`dutchman.wxs` is the per-user x64 package. Release builds install WiX 4.0.6 as a CI tool,
render the app SVG with the `ico` example, and call `scripts/windows-msi.ps1`. No WiX runtime
is shipped. The fixed UpgradeCode identifies this product across major upgrades. Keep it
and the component identities stable. Windows Installer versions have three fields capped
at 255.255.65535; the build refuses versions it cannot represent.

The package owns its three executables, licence notices, bundled skills and docs, installed
AGENTS.md, HKCU registration, Start menu shortcut and user PATH entry. The guide fragment is
generated at build time by `scripts/package-docs.py`, including removal rows for its folders. It never owns settings or logs. Its `InstallDir` registry value, together
with the current executable's expected per-user location, identifies MSI copies to the
updater; see `docs/desktop.md` for the full check and download disclosure.

The release workflow builds and exercises installation, an upgrade from a synthetic lower
version and over a second build of the same version, downgrade refusal, and uninstall on
windows-latest, for every release candidate and for packaging and update code pull requests.
`MajorUpgrade` sets `AllowSameVersionUpgrades`, because release candidates of one release all carry
its version. `scripts/test-windows-msi.ps1` stops on failed assertions and keeps the
MSI logs as a separate workflow artifact. The release workflow checks MSI checksums and
creates versionless copies alongside the ZIP and its checksum.

The Windows job also applies the release ZIP over a portable folder while a renamed
`cmd.exe` waits on stdin at `dutchman-gui.exe`. It checks the three new programs, bundled
guides and `.old` cleanup without needing a GUI renderer.
