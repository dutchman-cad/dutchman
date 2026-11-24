### Desktop app

- The in-app update no longer hangs behind a firewall that blocks GitHub: the release check and the Windows and AppImage downloads stop after 15 seconds for the lookup plus 15 per connection attempt, or 30 seconds without data (the check also after 20 seconds in total), says so plainly, leaves the installed version untouched and links to the manual download.
- Dutchman can be closed while an update is still downloading; before, the window refused to close and had to be killed.
