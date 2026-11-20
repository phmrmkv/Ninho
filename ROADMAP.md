# Ninho roadmap

What the popular local development tools do best, and what Ninho takes from each. Ninho's own code is written from
these ideas; no code or assets come from these tools.

| Tool | Best at | What Ninho takes | Status |
|---|---|---|---|
| **Laragon** | Fast Windows experience: portable folder, automatic `.test` virtual hosts, *Quick app*, terminal with the tools on the PATH | Portable `Ninho.exe`, auto virtual hosts, quick projects (WordPress, Laravel, Symfony), Terminal with PHP/MySQL/Node on the PATH, hosts file editor | ✅ 0.1 – 0.2 |
| **XAMPP** | The control panel everyone knows: one row per module with PID, port, Start, Admin, Config, Logs; Netstat; Shell | Module table with PID(s), Port(s), Start/Stop, Admin, Config, Logs; Ports tab (netstat with program names); Terminal | ✅ 0.2 |
| **WampServer** | Tray menu to switch PHP/MySQL versions and toggle PHP extensions; colored status icon | Config menu per module: version switch, open the config file, PHP extensions on/off | ✅ 0.2 · tray icon in 0.3 |
| **FlyEnv** | Many runtimes and services, several versions side by side, installs from official sources | *Download and update* from official sites, versions side by side, more than one server of each kind | ✅ 0.2 · PostgreSQL, MariaDB in 0.4 · MongoDB, Caddy in 0.7 |
| **Laravel Herd** | One-click sites list, PHP version per site, HTTPS per site, built-in mail | Sites tab with open / folder / PHP version per site; HTTPS for `.test` | ✅ 0.3 |
| **MAMP** | Simplicity: start page on localhost with phpinfo and links | Start page on `http://localhost` listing sites, phpinfo, Adminer, Mailpit | ✅ sites list in 0.3 · start page on localhost in 0.7 |
| **DDEV** | Per-project config file, database snapshots, share a site | `ninho.project.json` per site (PHP/Node version, database); database backup and restore in one click | ✅ backup and restore in 0.3 · project file in 0.7 |
| **Docker Desktop** | Isolation and any service through containers | Show Docker containers and their ports in the Ports tab; run a service from a Docker image when Docker is present | 🟡 containers and their ports in the Ports tab (with *Stop container*) in 0.7 · run a service from an image ⏳ |

## Done
- 0.3: tray icon, Sites tab with PHP version per site, HTTPS for `.test` sites with a local certificate authority, database backup and restore, Xdebug, share a site.
- 0.4: light and dark themes, PostgreSQL and MariaDB, QR code for shared links, live log viewer, command line.
- 0.5: 18 languages, portable and installable, 64-bit and 32-bit builds, project templates, environment check, MCP server.
- 0.6 (0.6.0 to 0.6.2): welcome window, self-update, import from XAMPP, WampServer and Laragon, one window at a time, crash log, winget and Scoop manifests, website, screen reader support (AccessKit) and keyboard navigation (focus ring, Esc, Ctrl+1/2/3), a UX review of every screen.
- 0.7: start page on `http://localhost` (sites, running servers, Mailpit, Adminer, phpinfo), `ninho.project.json` per site, MySQL / MariaDB root password change from the Config menu, MongoDB and Caddy, Docker containers in the Ports tab, tray menu in the chosen language, Linux and macOS preview builds.

## Next
- Digital signature of `Ninho.exe`.
- First release on winget (manifests of 0.6.2 sent to winget-pkgs, waiting for Microsoft's review) and on Scoop.
- Run a service from a Docker image when Docker is present.

### Linux and macOS
**Preview since the `feat-linux-macos` work:** everything that differs per system sits behind `src/platform`
(`windows.rs` keeps the Windows behavior unchanged, `unix.rs` does the same jobs on Linux and macOS), and CI builds
and tests on Ubuntu and macOS. Working there now:
- servers from the package manager (Apache, PHP, MySQL, MariaDB, PostgreSQL, Redis, Memcached), linked into
  `<root>/bin` and run as the user with Ninho's own config, data, sockets and logs; Mailpit, Node.js and cloudflared
  from their official Linux / macOS builds; a hint with the `apt` / `dnf` / `brew` command for what is missing;
- processes and ports with `ss` (Linux) and `lsof` (macOS), `kill` with a polite TERM first;
- one Ninho at a time (a local socket), the desktop's language, folder and file dialogs (zenity / kdialog / Finder),
  terminals, `xdg-open` / `open`;
- tray icon (D-Bus StatusNotifier on Linux, menu bar on macOS), start at login (autostart / LaunchAgent);
- Install on this PC on Linux (`~/.local/share/ninho` + menu entry), self-update with `Ninho-linux-x64` and
  `Ninho-macos-arm64` / `Ninho-macos-x64`.

Still to do:
- Nginx with its own config (and PHP-FPM) on Linux and macOS;
- HTTPS for the `.test` sites: the local authority in the system store (`update-ca-certificates`, the macOS keychain)
  and Apache on 8443;
- the hosts file through `pkexec` / an authorization prompt instead of showing the `sudo` command;
- PHP extensions and Xdebug for the system PHP (its `conf.d` folder), MySQL under Ubuntu's AppArmor profile;
- importing from XAMPP for Linux (`/opt/lampp`) and MAMP;
- signed and notarized macOS build, `.deb` / `.rpm` / AppImage / Homebrew cask.
