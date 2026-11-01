### Command line and install

- New `dutchman diagnose --narrow FILE` runs the whole narrowing by itself: it ranks the parts,
  isolates a failing part, alone and then with its neighbours, bisects its history to the first
  failing feature, and records the failure's signature (kind of feature, options, profile shape,
  and error code; never names, paths, or real dimensions). Each step has its own limits, it says
  `step 3 of 5: narrowing part 17 (feature 41 of 80)` as it goes, and running it again resumes
  after a crash or a reboot.
- New `dutchman diagnose --same-failure small.f3d` converts your small rebuild and says whether
  it fails the same way, and `--make-report` then makes one zip with a prefilled issue link.
  Nothing is sent: the zip stays on your machine until you send it.
- `dutchman convert FILE --part N --stop-at-feature K` converts a part with its history cut after
  feature K.
