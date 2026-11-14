# Changelog

## Unreleased
- Linux and macOS preview: Ninho builds and runs there (`Ninho-linux-x64`, `Ninho-macos-arm64`, `Ninho-macos-x64`). Apache, PHP, MySQL, MariaDB, PostgreSQL, Redis and Memcached come from the package manager and run as the user with Ninho's own settings, data and ports (Apache on 8080); Mailpit and Node.js are downloaded. Windows is unchanged.

## 0.6.2 — 2026-11-13
- A UX review of every screen: nothing cut off at the smallest window size, the same words for the same action, no exclamation marks, buttons all in the same three sizes.
- Preferences: services and ports explained, with headers, readable ports, short version numbers and a Download button where nothing is installed; a theme choice; folder pickers.
- A server you download now shows up in Modules at once.
- Clicks a few pixels outside a button still count; the window X is bigger.
- Ports: filter, a Test button, "End program" with a clear confirmation, Ninho servers stopped the normal way.
- Download and update: Redis and PHP no longer offered again when already up to date.
- Databases: whole rows, search past 10 databases, Show more for backups.
- Logs: centered, copy button, 12 px text. Disabled buttons say why.

## 0.6.1 — 2026-11-13
- Faster: the folders of programs and sites are read once a second instead of on every mouse move, so clicks answer at once (a frame went from about 9 ms to under 1 ms).
- Windows and messages can be dragged aside to read what is behind them.
- Everything that opens (windows, menus, lists) grows in with a short animation.
- Theme: one button with a sun and a moon instead of three.
- "Check for a new Ninho" shows its answer in a small window.
- Author shown as phmrmkv.

## 0.6.0 — 2026-11-13
- Welcome window on the first start: pick the language and the Ninho folder, and choose what to download now (Apache, PHP and MySQL are checked by default).
- Self-update: Ninho checks the GitHub releases when it opens (switch it off in Preferences, or use "Check for a new Ninho" in the menu), downloads the right exe for your Windows, checks its SHA-256, swaps itself and restarts.
- Import from XAMPP, WampServer and Laragon: copies your sites into `www` and copies your databases into Ninho's MySQL. The old install is only read, never changed.
- One window at a time: opening Ninho again brings the open window forward, also when it sits in the tray.
- Crash log: if Ninho breaks, the reason goes to `tmp\ninho-crash.log` and a message tells you where the file is.
- All new texts are translated into the 18 languages; so are the messages of the activity log.
- Packaging: winget and Scoop manifests (they work after the first release), CI builds and tests for 64-bit and 32-bit Windows.
- Screen reader support (AccessKit): every button says what it does and on which server. Keyboard: a visible focus ring, Esc closes the window, Ctrl+1/2/3 switch tabs.
- Fixes from a code review: updates install only with a matching SHA-256; the importer runs the old MySQL on a copy of its data; restarting after an update stops the servers and starts them again.
- Website: https://phmrmkv.github.io/Ninho/
- Project files: CONTRIBUTING, SECURITY, CODE_OF_CONDUCT, issue and pull request templates.
- Website for GitHub Pages in `docs/index.html`: https://phmrmkv.github.io/Ninho/

## 0.5.0
- 18 languages: English, Português (Brasil), Español, Français, Deutsch, Italiano, Nederlands, Polski, Русский, Українська, Türkçe, Bahasa Indonesia, Tiếng Việt, 日本語, 한국어, 简体中文, 繁體中文, हिन्दी. First start follows the Windows language; CJK and Devanagari use the fonts Windows already has.
- Portable and installable: "Install on this PC" (Preferences) copies Ninho to your user programs folder, adds it to the Start menu and to Windows Installed apps, with no administrator rights; uninstalling keeps your servers, databases and sites.
- 64-bit and 32-bit Windows builds (Ninho-x64.exe, Ninho-x86.exe), built and published by GitHub Actions. On 32-bit Windows the tools come in their 32-bit builds where one exists.
- Create project from templates: Drupal, CodeIgniter, Slim, Vite + React / Vue, static HTML, a ZIP or a git repository, and your own templates.
- Check my environment, and an MCP server for AI coding agents (`Ninho.exe mcp`).

## 0.4.0
- Light and dark themes (or follow Windows), built on semantic color tokens with contrast checked in both; dark text on the teal accent.
- One button recipe: fixed heights (32 for row actions, 36 by default, 40 for the main action), radius 8, same text sizes.
- Clearer wording: verb + object (Start server, Open site, View log, Configure), nothing under 12px.
- Tray icon next to the clock with a status dot (green, orange, grey) and a menu; closing the window keeps Ninho in the tray; option to open hidden.
- PostgreSQL and MariaDB modules: newest version from their official sources, data folder created on first start, polite stop, config, log and Open (psql / Adminer).
- QR code of a shared link, to open it on a phone.
- Live log viewer with filter.
- Command line: `status` (and `--json`), `start`, `stop`, `reload`.

## 0.3.0
- Apache in one click: the newest Win64 build from Apache Lounge, set up for the chosen PHP (as a module), the sites folder and the .test sites. Changing the PHP version applies at the next Apache start.
- Sites tab: every folder of www with its kind (Laravel, WordPress, Symfony, Node.js, PHP, HTML), its .test address, and Open / Folder / Terminal / VS Code.
- PHP version per site: each site can run alone on its own PHP server with its own PHP version (Laravel and Symfony from public/).
- Share a site: a public https link through a Cloudflare quick tunnel (no account), with Copy; cloudflared is downloaded on first use.
- HTTPS for the .test sites: a local certificate authority (trusted for your user, Windows asks), a certificate per site, Apache on 443.
- Xdebug: download the build that matches the PHP and turn it on or off from the PHP Config menu.
- Databases window: list the databases of each MySQL, back one up, restore a backup, import a .sql file.
- FEATURES.md: what the other local servers offer and what Ninho already does.

## 0.2.0
- New control panel: module table with PID(s), Port(s), Start/Stop, Admin, Config and Logs; tool buttons on the side;
  activity list with the time of each line.
- Config menu per module: switch version, open the config file (`httpd.conf`, `my.ini`, `php.ini`...), turn PHP
  extensions on and off, change port, open folder.
- Admin: Apache/PHP/Nginx open their page, Mailpit its inbox, Redis its command line, MySQL HeidiSQL or Adminer.
- *Download and update*: newest stable PHP, MySQL 8.4 LTS, Node.js LTS, Mailpit and Redis from their official sites,
  installed beside the versions already there. `Ninho.exe --install php nodejs mailpit` does the same without a window.
- Ports tab: every listening port with program, PID and reach; test a port; stop a program (Windows' own processes are
  protected).
- More than one server of each kind (MySQL with its own data folder, PHP, Redis, Memcached, Mailpit).
- When another program holds a module's port, Ninho says who and offers a free port in one click.
- Portable: settings in `ninho.json` beside `Ninho.exe`; new logo, exe and window icon.
- Stopping MySQL by force only stops the server on that port, never every `mysqld.exe`.

## 0.1.0
- First version: Apache, MySQL, Nginx, Redis, Memcached, PHP server and Mailpit from one window; automatic `.test`
  virtual hosts; quick projects; create database; preferences in Portuguese and English.
