# Security

Tunnelbird imports VPN profiles, which hold private keys and passwords, and
talks to NetworkManager (or, on macOS, to a helper that runs as root). A flaw
here matters: please report it privately, not in a public issue.

## Reporting

- On GitHub: **Security › Report a vulnerability** on this repository (private).
- Or by e-mail: ferhatsnpro@gmail.com

Say what you found, how to reproduce it, and the version
(shown in the About window). You will get an answer within a
few days; this is a one-person project, so a fix can take longer. Once it is
fixed, the report is credited if you wish.

## What counts

- A private key, a password or a tunnel profile written where others can read
  it, shown in a log, or sent anywhere.
- Traffic leaving outside a tunnel that claims to carry everything.
- Anything that lets a normal user run code as root through the macOS helper.
- A package or install script that does not match what the release published.

Tunnelbird does not manage servers, so a problem of a VPN server itself is not
in scope.

## Versions

Only the latest release is supported.
