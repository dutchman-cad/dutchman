### History

- An extrude whose sketch profile Fusion does not record is now kept when exactly one region of
  its sketch, rebuilt in SolidCraft, matches the original. Incomplete measurements, unproven
  region selections, a tie or no match keep it out.
- The last fillet or chamfer of a part is kept the same way when its edges cannot be found
  exactly on the rebuilt body.
- The JSON report marks such features `inferred`, never exact, with what they were measured
  against.
