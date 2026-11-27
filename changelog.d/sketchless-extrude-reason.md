### History

- An extrude whose profile sketch the source does not hold, and whose body a later feature changes, now stops with the reason: the source stores that body only as the design ends, so neither a stored result nor a profile read from its faces can be proven. It no longer reports "native profiles are not mapped yet" alone.
