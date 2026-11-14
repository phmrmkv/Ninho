# Ninho feature checklist

Everything the popular local development tools offer, gathered from their official sites and docs (November 2026),
and where Ninho stands. Ninho's code is its own; this list only tracks ideas.

✅ done · 🟡 partly done · ⬜ planned · ➖ out of scope (containers, cloud services)

Tools: **LAR** Laragon · **FLY** FlyEnv · **DOC** Docker Desktop · **DDEV** DDEV · **HERD** Laravel Herd ·
**MAMP** MAMP · **XAMPP** XAMPP · **WAMP** WampServer

## Services and runtimes
- ✅ Apache, installed and configured in one click with the chosen PHP (LAR, FLY, XAMPP, WAMP, MAMP, DDEV)
- 🟡 Nginx: runs and gets the `.test` sites; install by hand for now (LAR, FLY, HERD, MAMP, DDEV)
- ✅ Caddy, newest from GitHub: serves the `.test` sites on port 2015 with PHP through php-cgi workers (FLY, DDEV)
- ⬜ FrankenPHP (FLY, DDEV)
- ✅ PHP, newest stable from windows.php.net (all)
- ✅ MySQL 8.4 LTS · ✅ MariaDB LTS (LAR, FLY, XAMPP, WAMP, MAMP, DDEV, HERD Pro)
- ✅ PostgreSQL, newest version, without pgAdmin (LAR, FLY, DDEV, HERD Pro)
- ✅ MongoDB Community, newest major release from MongoDB's download feed, data in `data\mongodb`; stops through mongosh (LAR, FLY, HERD Pro)
- ✅ MongoDB Shell (mongosh), opened by *Admin* on MongoDB (LAR, FLY)
- ✅ Redis (FLY, DDEV, HERD Pro)
- 🟡 Memcached: runs; install by hand (FLY)
- ✅ Node.js LTS on the PATH (LAR, FLY, HERD, DDEV)
- ⬜ Python, Go and other runtimes (LAR, FLY, MAMP)
- ⬜ Meilisearch, Elasticsearch, RabbitMQ (FLY, HERD Pro, DDEV)
- ⬜ MinIO / S3-compatible storage (FLY, HERD Pro)
- ➖ FTP server (FLY, XAMPP)
- 🟡 Custom services: more than one server of each kind side by side ✅; Procfile-style own commands ⬜ (LAR, DDEV, DOC, FLY)
- ➖ Kubernetes, Linux/Windows containers (DOC)

## Versions
- ✅ Switch PHP version from the panel (LAR, FLY, HERD, WAMP, MAMP)
- ✅ PHP version per site (own PHP server per site, Sites tab) (FLY, HERD, DDEV, WAMP)
- ✅ Node.js versions side by side (newest of each LTS line) and a version per project through `.nvmrc` (FLY, HERD, DDEV, LAR)
- ✅ Several versions of each service side by side (LAR, WAMP)
- ✅ Install a new version in one click from official sites (LAR, FLY, HERD, WAMP)
- ⬜ Profiles to switch whole stacks (LAR)
- 🟡 Database version per project: one MySQL server per project, each with its own data folder ✅ (DDEV, FLY, HERD Pro)
- ✅ PHP extensions on/off · ⬜ add an extension DLL (LAR, HERD)
- ✅ Open `php.ini`, `my.ini`, `httpd.conf` from the panel (HERD, XAMPP, WAMP)

## Sites and domains
- ✅ Automatic `*.test` virtual hosts for every folder in `www` (LAR, FLY, HERD, DDEV)
- ✅ Configurable domain pattern (`{name}.test`) (LAR, FLY, DDEV)
- ⬜ Create / edit a virtual host from the interface (WAMP, MAMP PRO, FLY)
- ✅ One host per project (MAMP PRO, LAR, FLY, HERD, DDEV)
- ⬜ Domain aliases per site (HERD, DDEV, LAR)
- ✅ "Parked" sites folder: every subfolder is a site (HERD, LAR)
- ⬜ Reverse proxy per site (FLY, HERD)
- ⬜ Own DNS for `*.test` (HERD, FLY)
- ✅ Hosts file kept up to date, and editable from the menu (DDEV, XAMPP, WAMP)
- ✅ Port conflict detection naming the program, with a free port in one click (DDEV, HERD)
- ✅ Every listening port of the PC in one view, stop a program safely — *Ninho only*
- ✅ Configurable port per service (FLY, HERD, DDEV, XAMPP)

## HTTPS
- ✅ HTTPS for the .test sites in one click (Config of Apache) (LAR, FLY, HERD, DDEV)
- ✅ Local certificate authority, trusted for your user only (Windows asks) (DDEV, FLY, LAR)
- ⬜ Certificate list with expiry (LAR, HERD)
- 🟡 HTTPS certificates also valid for localhost and 127.0.0.1 ✅ · LAN address ⬜ (LAR)
- ⬜ Certificate diagnosis (DDEV)

## Databases
- ➖ phpMyAdmin (Adminer covers it) (LAR, FLY, XAMPP, WAMP, DDEV)
- ✅ Adminer, served by the installed PHP (WAMP, DDEV)
- ⬜ pgAdmin, Redis viewer (FLY)
- ✅ Open HeidiSQL when installed (DDEV, HERD, LAR)
- ✅ Create the database when creating a project (LAR, HERD)
- ✅ Import a `.sql` file · ✅ export (backup) — Databases window (DDEV)
- ✅ Database snapshots: backup and restore in one click (DDEV)
- ⬜ Automatic MySQL backups (LAR)
- ⬜ Switch a Laravel app's database/cache/queue by editing `.env` (HERD Pro)

## Mail and debugging
- ✅ Mailpit mail catcher with its inbox (LAR, FLY, DDEV)
- ➖ Own mail catcher (HERD Pro) — Mailpit covers it
- ➖ Mercury mail server (XAMPP)
- ✅ Xdebug: download the matching build and turn it on/off (Config of PHP) (DDEV, HERD, WAMP)
- ⬜ Xdebug started by the browser extension (HERD Pro)
- ⬜ Profiler (XHProf) (DDEV, HERD)
- ✅ Live log viewer inside Ninho: follows the file, filter, warnings and errors in color (LAR, HERD, DDEV, FLY)
- ✅ Open config files from the panel (FLY, LAR, WAMP, XAMPP)
- ⬜ Dumps and request insights (HERD Pro)
- ✅ Check my environment: Visual C++, ports (also Windows-reserved ranges), Apache config test, PHP and its extensions, MySQL login, hosts file, HTTPS, disk space — each with what to do (DDEV, WAMP)

## Sharing
- ✅ Share a site through a public https link (Cloudflare quick tunnel, no account), copy the link (LAR, HERD, DDEV, FLY)
- ⬜ Region / subdomain / password on the link (LAR, HERD)
- ✅ QR code of the shared link, to open it on a phone (LAR, DDEV)
- ⬜ Open on a phone in the same network (MAMP, LAR)

## Projects and templates
- ✅ WordPress, Laravel, Symfony, Drupal, CodeIgniter, Slim, static HTML, Vite + React / Vue in one click (LAR, MAMP PRO, FLY, DDEV, HERD)
- ✅ Own project templates in `etc/ninho-templates.json` (LAR)
- ✅ Project from a ZIP URL or a git repository (LAR)
- ⬜ Per-project config file `ninho.yml` (HERD, DDEV)
- ⬜ Project hooks (DDEV, HERD)
- ➖ Git-branch workspaces (HERD)
- ➖ Pull/push from hosting providers (DDEV)
- ✅ Open the project in VS Code (HERD, LAR)
- ✅ MCP server for AI agents: `Ninho.exe mcp` with status, start, stop, restart, list_sites, read_log (FLY, HERD, DOC)

## Interface and usability
- ✅ Control panel with start/stop per module, PID, port, Admin, Config, Logs (all)
- ✅ Tray icon with a status dot (green / orange / grey) and menu; closing the window keeps Ninho in the tray (LAR, HERD, WAMP, XAMPP, FLY, DOC)
- ✅ Start all / stop all (LAR, DOC)
- ⬜ Start groups (FLY)
- ✅ Sites tab with kind of project, address, PHP version and actions · ⬜ favorites and search (HERD)
- ✅ Terminal with the tools on the PATH (per site: its PHP version) (LAR, FLY, DDEV)
- ⬜ Global shortcuts (HERD)
- ✅ Start with Windows · open hidden in the tray (HERD)
- ✅ Updates: tools from official sites; Ninho updates itself from GitHub releases, with a SHA-256 check (HERD, LAR, WAMP)
- ✅ 18 languages, first start in the Windows language
- ✅ Welcome window on the first start: language, Ninho folder, what to download now
- ✅ One window at a time: opening Ninho again brings the open one forward, also from the tray
- ✅ Crash log in `tmp\ninho-crash.log`, with a message telling where it is
- ✅ Light and dark themes, or follow Windows — *design tokens with contrast checked in both*

## Platform and portability
- ✅ Portable, no installer: one `Ninho.exe` and a `ninho.json` beside it (LAR, FLY)
- ✅ Isolated: no Windows services (LAR, HERD, FLY)
- ✅ Portable and installable: Install on this PC (Start menu, Installed apps, no admin), uninstall keeps the data
- ✅ Windows 64-bit and 32-bit builds, built and tested by GitHub Actions
- ✅ Import from XAMPP, WampServer and Laragon: copies the sites and the databases, the old install is only read
- 🟡 winget and Scoop packages: manifests ready, they go live after the first release
- ⬜ Install a module as a Windows service (XAMPP)
- 🟡 Linux and macOS: preview builds (servers from the package manager, run as the user on Ninho's own ports; Nginx, HTTPS and Xdebug still Windows-only) (FLY, DOC, DDEV, HERD, MAMP)
- ➖ Containers (DOC, DDEV)
- ✅ Free and open source: MIT or GPL-3.0, at your choice (FLY, DDEV)

## Command line
- ✅ `Ninho.exe status`, `start [id…]`, `stop [id…]`, `reload`, `--install tool…` (LAR, HERD, DDEV, FLY)
- ✅ `status --json` (HERD)
- ✅ PHP / Composer / Node with the site's PHP version in its terminal (HERD, DDEV, LAR)
- ✅ `Ninho.exe reload` rewrites the sites and restarts Apache (LAR, HERD)
- ⬜ Framework commands (wp, artisan) (DDEV, HERD)
