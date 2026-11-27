### History

- Reading a Fusion design made of many independent components no longer runs for hours without finishing: cadmpeg is pinned to its 0.3.1 commit plus cadmpeg#1646, which tests profile-region containment with a bounded sweep instead of comparing every pair of curve pieces.
