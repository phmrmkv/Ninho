# Security Policy

## Reporting a vulnerability

Please **do not** open a public issue for a security problem.

Report it privately with GitHub's private vulnerability reporting: open the
[Security tab](https://github.com/paulohmaciel/Ninho/security) of the repository and choose
**Report a vulnerability**. If that button is missing, open a normal issue that only says you need to report a
security problem, without any details, and the maintainer will arrange a private channel.

Please include:

- the Ninho version, whether it is the x64 or x86 build, and your Windows version;
- what you found and the steps to reproduce it;
- the impact you think it has.

## What is in scope

- Ninho itself: the app, its command line and MCP server, how it writes configuration, the hosts file and
  certificates, how it downloads and verifies files, and its release files.
- The local certificate authority and `.test` HTTPS handling.
- The release pipeline and the hashes published in `SHA256SUMS.txt`.

## What is out of scope

- The servers Ninho downloads (Apache, PHP, MySQL, MariaDB, PostgreSQL, Node.js, Redis, Mailpit, Nginx,
  cloudflared and others) are upstream projects. Report their bugs to them; we will update the versions Ninho offers.
- Servers you start on purpose and expose to the network or the internet (for example a shared site) are your
  responsibility, though a safer default in Ninho is always welcome as a suggestion.
- Problems that need an attacker who already has administrator rights on your PC.

## What to expect

This is a volunteer project, so these are goals, not promises:

- an answer within about 7 days;
- a fix or a plan within about 30 days for confirmed problems, sooner for serious ones;
- credit in the release notes if you want it, after the fix is out.

Only the latest release gets security fixes.
