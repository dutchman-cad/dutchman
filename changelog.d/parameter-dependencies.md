### History

- Merged designs that reuse a parameter name keep their own references: an expression follows
  the parameters it depends on to their new names instead of naming another design's
  parameter of the same name. Such renames are reported as information, and only names
  SolidCraft cannot use are reported as changed.
