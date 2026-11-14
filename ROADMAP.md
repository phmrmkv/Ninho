# Ninho roadmap

What the popular local development tools do best, and what Ninho takes from each. Ninho's own code is written from
these ideas; no code or assets come from these tools.

| Tool | Best at | What Ninho takes | Status |
|---|---|---|---|
| **Laragon** | Fast Windows experience: portable folder, automatic `.test` virtual hosts, *Quick app*, terminal with the tools on the PATH | Portable `Ninho.exe`, auto virtual hosts, quick projects (WordPress, Laravel, Symfony), Terminal with PHP/MySQL/Node on the PATH, hosts file editor | ✅ 0.1 – 0.2 |
| **XAMPP** | The control panel everyone knows: one row per module with PID, port, Start, Admin, Config, Logs; Netstat; Shell | Module table with PID(s), Port(s), Start/Stop, Admin, Config, Logs; Ports tab (netstat with program names); Terminal | ✅ 0.2 |
| **WampServer** | Tray menu to switch PHP/MySQL versions and toggle PHP extensions; colored status icon | Config menu per module: version switch, open the config file, PHP extensions on/off | ✅ 0.2 · tray icon in 0.3 |
| **FlyEnv** | Many runtimes and services, several versions side by side, installs from official sources | *Download and update* from official sites, versions side by side, more than one server of each kind | ✅ 0.2 · PostgreSQL, MariaDB in 0.4 · MongoDB, Caddy in 0.6.3 |
| **Laravel Herd** | One-click sites list, PHP version per site, HTTPS per site, built-in mail | Sites tab with open / folder / PHP version per site; HTTPS for `.test` | ✅ 0.3 |
| **MAMP** | Simplicity: start page on localhost with phpinfo and links | Start page on `http://localhost` listing sites, phpinfo, Adminer, Mailpit | 🟡 sites list in 0.3; start page on localhost ⏳ |
| **DDEV** | Per-project config file, database snapshots, share a site | `ninho.project.json` per site (PHP/Node version, database); database backup and restore in one click | 🟡 backup and restore ✅ 0.3 · project file ⏳ |
| **Docker Desktop** | Isolation and any service through containers | Show Docker containers and their ports in the Ports tab; run a service from a Docker image when Docker is present | 🟡 containers and their ports in the Ports tab in 0.6.3 · run a service from an image ⏳ |

## Done
- 0.3: tray icon, Sites tab with PHP version per site, HTTPS for `.test` sites with a local certificate authority, database backup and restore, Xdebug, share a site.
- 0.4: light and dark themes, PostgreSQL and MariaDB, QR code for shared links, live log viewer, command line.
- 0.5: 18 languages, portable and installable, 64-bit and 32-bit builds, project templates, environment check, MCP server.
- 0.6 (unreleased): welcome window, self-update, import from XAMPP, WampServer and Laragon, one window at a time, crash log, winget and Scoop manifests, website.

## Next
- Start page on `http://localhost` (sites, phpinfo, Adminer, Mailpit).
- `ninho.project.json` per site (PHP / Node version, database).
- MySQL root password change from the Config menu.
- Digital signature of `Ninho.exe`, and first release on winget and Scoop.
- Screen reader support and full keyboard navigation.

### Linux and macOS
The window (egui) already runs on Linux and macOS. What is Windows-only today is the server management, and each piece needs its own version per system:
- installing tools: official tarballs where they exist (MySQL, MariaDB, PostgreSQL, Node.js, Mailpit), the package manager or Homebrew for the rest (PHP, Apache, Nginx);
- processes and ports: `ss` / `lsof` instead of `netstat` / `tasklist`, signals instead of `taskkill`;
- the hosts file at `/etc/hosts` with `sudo` or `pkexec`, certificates in the system store or the browser;
- tray icon and start at login per system.
Plan: put each of these behind a small per-system module, Windows first and unchanged, then Linux, then macOS.
