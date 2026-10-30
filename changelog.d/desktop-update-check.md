### Desktop app

- Release builds check for stable updates at startup and hourly using GitHub's conditional
  releases feed. A header button offers release notes and the appropriate download or package
  manager command, with persistent skip and three-day reminder choices and update settings.
- Homebrew and Scoop updates wait for the new package version. Homebrew updates in the
  background with live output and restart controls; Scoop confirms a close, update, and reopen
  hand-off so Windows can replace the executable, and reports failed updates once on restart.
  Failures retain a copyable command. The Homebrew cask is `dutchman-cad/tap/dutchman-convert`.
