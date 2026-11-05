### Reports

- The anonymous report now lists each blocked feature's timeline ordinal and a short variant
  (`extrude, to a plane`, `hole, counterbore, through all`, `fillet, 2 radius groups`), in
  timeline order, with no names or sizes. The JSON report's history features carry the same
  `ordinal` and `variant`.
- A face, edge or body selection Dutchman cannot map now says which form it was, such as "a
  face from an earlier history state is not supported yet" or "a native edge reference is not
  supported yet", instead of "not resolved".
